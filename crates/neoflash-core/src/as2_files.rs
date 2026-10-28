//! AS2 file-dialog filters and bounded multipart upload assembly.
//! Owned implementation of the Adobe AS2 FileReference and RFC 7578 contracts.
//! URI form decoding reuses the existing owned Rust string kernel.
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
#[error("AS2 FileReference: {0}")]
pub struct FileError(pub String);
fn error(value: &str) -> FileError {
    FileError(value.into())
}
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

pub fn file_name(name: &str) -> Result<&str, FileError> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '"'))
    {
        return Err(error(
            "filename must be a plain basename of 1..255 bytes without controls, quotes or path separators",
        ));
    }
    Ok(name)
}
pub fn command(input: &str) -> Result<Value, FileError> {
    if input.len() > 16 * 1024 {
        return Err(error("file options exceed 16 KiB"));
    }
    let value: Value = serde_json::from_str(input).map_err(|e| FileError(e.to_string()))?;
    match value["op"].as_str() {
        Some("name") => {
            let name = file_name(
                value["name"]
                    .as_str()
                    .ok_or_else(|| error("filename must be a string"))?,
            )?;
            let extension = name
                .rfind('.')
                .filter(|i| *i > 0 && *i + 1 < name.len())
                .map(|i| &name[i..])
                .unwrap_or("");
            Ok(json!({"name":name,"type":extension}))
        }
        Some("filters") => {
            let Some(filters) = value.get("types").filter(|v| !v.is_null()) else {
                return Ok(json!(""));
            };
            let filters = filters
                .as_array()
                .ok_or_else(|| error("browse types must be an array"))?;
            if filters.len() > 32 {
                return Err(error("browse supports at most 32 filter groups"));
            }
            let mut extensions = Vec::new();
            for filter in filters {
                if filter
                    .get("macType")
                    .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
                {
                    return Err(error(
                        "Macintosh creator/type codes are unavailable in browser file dialogs",
                    ));
                }
                let pattern = filter["extension"]
                    .as_str()
                    .ok_or_else(|| error("each browse filter requires an extension string"))?;
                if pattern.len() > 1024 {
                    return Err(error("browse extension filter exceeds 1024 bytes"));
                }
                for token in pattern.split(';').map(str::trim).filter(|v| !v.is_empty()) {
                    if token == "*" || token == "*.*" {
                        return Ok(json!(""));
                    }
                    let ext = token.strip_prefix("*.").ok_or_else(|| {
                        error("file filters support *.extension patterns separated by semicolons")
                    })?;
                    if ext.is_empty()
                        || ext.len() > 32
                        || !ext
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    {
                        return Err(error("invalid file-filter extension"));
                    }
                    let ext = format!(".{}", ext.to_ascii_lowercase());
                    if !extensions.contains(&ext) {
                        extensions.push(ext);
                    }
                    if extensions.len() > 128 {
                        return Err(error("browse supports at most 128 extensions"));
                    }
                }
            }
            Ok(json!(extensions.join(",")))
        }
        _ => Err(error("unknown file command")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Multipart {
    pub boundary: String,
    pub filename: String,
    pub field: String,
    pub content_type: String,
    #[serde(default)]
    pub post_data: String,
}
fn form_decode(text: &str) -> Result<String, FileError> {
    use crate::as3_strings::{As3StringRequest, As3StringResult, as3_string_execute};
    let value = as3_string_execute(As3StringRequest {
        operation: "decodeURIComponent".into(),
        receiver: text.replace('+', " ").encode_utf16().collect(),
        strings: Vec::new(),
        numbers: Vec::new(),
        arg_count: 0,
        null_pattern: false,
    })
    .map_err(|e| FileError(e.to_string()))?;
    let As3StringResult::String(value) = value else {
        return Err(error("unexpected URI decoder result"));
    };
    String::from_utf16(&value).map_err(|_| error("invalid form Unicode"))
}
fn field_name(name: &str) -> Result<(), FileError> {
    if name.is_empty()
        || name.len() > 128
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\\'))
    {
        return Err(error(
            "multipart field name must contain 1..128 safe characters",
        ));
    }
    Ok(())
}
pub fn multipart(input: &str, bytes: &[u8]) -> Result<Vec<u8>, FileError> {
    if bytes.len() > MAX_FILE_BYTES || input.len() > 72 * 1024 {
        return Err(error("upload exceeds 8 MiB file / 64 KiB form-data budget"));
    }
    let spec: Multipart = serde_json::from_str(input).map_err(|e| FileError(e.to_string()))?;
    file_name(&spec.filename)?;
    field_name(&spec.field)?;
    if !(12..=70).contains(&spec.boundary.len())
        || !spec
            .boundary
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(error("invalid multipart boundary"));
    }
    if spec.content_type.is_empty()
        || spec.content_type.len() > 128
        || spec.content_type.chars().any(|c| c.is_control())
    {
        return Err(error("invalid upload MIME type"));
    }
    if spec.post_data.len() > 64 * 1024 {
        return Err(error("postData exceeds 64 KiB"));
    }
    if bytes
        .windows(spec.boundary.len())
        .any(|part| part == spec.boundary.as_bytes())
    {
        return Err(error(
            "multipart boundary collides with file data; retry upload",
        ));
    }
    let mut fields = vec![("Filename".to_owned(), spec.filename.clone())];
    for part in spec.post_data.split('&').filter(|v| !v.is_empty()) {
        if fields.len() > 64 {
            return Err(error("postData supports at most 64 fields"));
        }
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        let name = form_decode(name)?;
        field_name(&name)?;
        if name == spec.field || matches!(name.as_str(), "Filename" | "Upload") {
            return Err(error(
                "postData cannot replace reserved Filename, Upload or file-data fields",
            ));
        }
        let value = form_decode(value)?;
        if value.len() > 16 * 1024 {
            return Err(error("one postData value exceeds 16 KiB"));
        }
        fields.push((name, value));
    }
    fields.push(("Upload".into(), "Submit Query".into()));
    let mut output = Vec::with_capacity(bytes.len() + spec.post_data.len() + 2048);
    for (name, value) in fields {
        if value.contains(&spec.boundary) {
            return Err(error(
                "multipart boundary collides with form data; retry upload",
            ));
        }
        output.extend_from_slice(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n",
                spec.boundary, name, value
            )
            .as_bytes(),
        );
    }
    output.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",spec.boundary,spec.field,spec.filename,spec.content_type).as_bytes());
    output.extend_from_slice(bytes);
    output.extend_from_slice(format!("\r\n--{}--\r\n", spec.boundary).as_bytes());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_file_transfer_family() {
        assert_eq!(
            command(r#"{"op":"filters","types":[{"extension":"*.TXT;*.png;*.txt"}]}"#).unwrap(),
            ".txt,.png"
        );
        assert!(command(r#"{"op":"filters","types":[{"extension":"../*"}]}"#).is_err());
        assert_eq!(
            command(r#"{"op":"name","name":"fixture.txt"}"#).unwrap()["type"],
            ".txt"
        );
        assert!(file_name("../../bad").is_err());
        let spec = json!({"boundary":"NeoFlashBoundary123","filename":"fixture.txt","field":"Filedata","content_type":"text/plain","post_data":"label=AS2+fixture&mark=%E2%9C%93"});
        let body = multipart(&spec.to_string(), b"hello\0binary").unwrap();
        let text = String::from_utf8(body.clone()).unwrap();
        assert!(text.contains("name=\"label\"\r\n\r\nAS2 fixture"));
        assert!(text.contains("name=\"mark\"\r\n\r\n✓"));
        assert!(body.windows(12).any(|v| v == b"hello\0binary"));
        assert!(text.ends_with("--NeoFlashBoundary123--\r\n"));
        assert!(multipart(&spec.to_string(), b"NeoFlashBoundary123").is_err());
        let mut bad = spec;
        bad["post_data"] = json!("Filename=spoofed");
        assert!(multipart(&bad.to_string(), b"hello").is_err());
    }
}
