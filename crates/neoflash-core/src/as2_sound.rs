//! AS2-only channel matrices and bounded ID3 text metadata.
//! Matrix orientation/pan behavior adapts Ruffle (MIT), commit
//! 07d3e430631333f80f754df01a968bdcd80d98c5, sound.rs/display_object.rs.
//! ID3 parsing is an owned implementation of id3.org v1/v2.3/v2.4 layouts.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, thiserror::Error)]
#[error("AS2 Sound: {0}")]
pub struct SoundError(pub String);
fn error(message: &str) -> SoundError {
    SoundError(message.into())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundMatrix {
    pub ll: f64,
    pub lr: f64,
    pub rl: f64,
    pub rr: f64,
    pub volume: f64,
}
impl Default for SoundMatrix {
    fn default() -> Self {
        Self {
            ll: 100.0,
            lr: 0.0,
            rl: 0.0,
            rr: 100.0,
            volume: 100.0,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformCommand {
    #[serde(default)]
    pub state: SoundMatrix,
    pub patch: Option<Map<String, Value>>,
    pub pan: Option<f64>,
    pub volume: Option<f64>,
}
#[derive(Serialize)]
pub struct TransformResult {
    pub state: SoundMatrix,
    pub pan: f64,
    pub gains: [f64; 4],
}
fn percent(value: f64, min: f64, max: f64) -> Result<f64, SoundError> {
    if value.is_finite() && value >= min && value <= max {
        Ok(value.trunc())
    } else {
        Err(error(
            "channel/volume percentages must be finite values in 0..100; pan in -100..100",
        ))
    }
}
pub fn transform(command: TransformCommand) -> Result<TransformResult, SoundError> {
    let mut state = command.state;
    for value in [
        &mut state.ll,
        &mut state.lr,
        &mut state.rl,
        &mut state.rr,
        &mut state.volume,
    ] {
        *value = percent(*value, 0.0, 100.0)?;
    }
    if let Some(pan) = command.pan {
        let pan = percent(pan, -100.0, 100.0)?;
        state.ll = if pan >= 0.0 { 100.0 - pan } else { 100.0 };
        state.rr = if pan >= 0.0 { 100.0 } else { 100.0 + pan };
        state.lr = 0.0;
        state.rl = 0.0;
    }
    if let Some(patch) = command.patch {
        for (key, value) in patch {
            let target = match key.as_str() {
                "ll" => &mut state.ll,
                "lr" => &mut state.lr,
                "rl" => &mut state.rl,
                "rr" => &mut state.rr,
                _ => return Err(error("transform accepts ll, lr, rl and rr only")),
            };
            *target = percent(
                value
                    .as_f64()
                    .ok_or_else(|| error("transform coefficients must be numbers"))?,
                0.0,
                100.0,
            )?;
        }
    }
    if let Some(volume) = command.volume {
        state.volume = percent(volume, 0.0, 100.0)?;
    }
    Ok(TransformResult {
        state,
        pan: if state.ll != 100.0 {
            100.0 - state.ll
        } else {
            state.rr - 100.0
        },
        gains: [state.ll, state.lr, state.rl, state.rr].map(|v| v * state.volume / 10000.0),
    })
}

/// Each object is a metadata snapshot, in tag-arrival order (v2, then v1).
/// Binary/non-text frames are intentionally outside this text-only profile.
#[derive(Debug, Serialize)]
pub struct Metadata {
    pub snapshots: Vec<Map<String, Value>>,
    pub warnings: Vec<String>,
}
const MAX_TAG: usize = 1024 * 1024;
const MAX_TEXT: usize = 16 * 1024;
fn sync_size(bytes: &[u8]) -> Result<usize, SoundError> {
    if bytes.len() != 4 || bytes.iter().any(|n| n & 128 != 0) {
        return Err(error("invalid ID3 synchsafe size"));
    }
    Ok(bytes.iter().fold(0usize, |n, b| (n << 7) | usize::from(*b)))
}
fn latin(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| char::from(*b))
        .collect::<String>()
        .trim_end()
        .into()
}
fn text(bytes: &[u8], encoding: u8) -> Result<String, SoundError> {
    if bytes.len() > MAX_TEXT {
        return Err(error("ID3 text frame exceeds 16 KiB"));
    }
    let value = match encoding {
        0 => latin(bytes),
        3 => std::str::from_utf8(bytes)
            .map_err(|_| error("invalid ID3 UTF-8 text"))?
            .split('\0')
            .next()
            .unwrap_or("")
            .to_owned(),
        1 | 2 => {
            let (bytes, little) = if encoding == 1 {
                if bytes.starts_with(&[255, 254]) {
                    (&bytes[2..], true)
                } else if bytes.starts_with(&[254, 255]) {
                    (&bytes[2..], false)
                } else {
                    return Err(error("ID3 UTF-16 text requires a byte-order marker"));
                }
            } else {
                (bytes, false)
            };
            if bytes.len() % 2 != 0 {
                return Err(error("truncated ID3 UTF-16 text"));
            }
            let words: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if little {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .take_while(|v| *v != 0)
                .collect();
            String::from_utf16(&words).map_err(|_| error("invalid ID3 UTF-16 text"))?
        }
        _ => return Err(error("unsupported ID3 text encoding")),
    };
    Ok(value.trim_end_matches('\0').to_owned())
}
fn after_description(bytes: &[u8], encoding: u8) -> Result<&[u8], SoundError> {
    let width = if encoding == 1 || encoding == 2 { 2 } else { 1 };
    let offset = bytes
        .chunks_exact(width)
        .position(|c| c.iter().all(|b| *b == 0))
        .ok_or_else(|| error("unterminated ID3 comment description"))?;
    Ok(&bytes[(offset + 1) * width..])
}
fn insert(fields: &mut Map<String, Value>, id: &str, value: String) {
    let alias = match id {
        "TIT2" => Some("songname"),
        "TPE1" => Some("artist"),
        "TALB" => Some("album"),
        "TYER" | "TDRC" => Some("year"),
        "TRCK" => Some("track"),
        "TCON" => Some("genre"),
        "COMM" => Some("comment"),
        _ => None,
    };
    if let Some(alias) = alias {
        fields.insert(alias.into(), value.clone().into());
    }
    if id == "COMM" {
        fields
            .entry(id)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(value.into());
    } else {
        fields.insert(id.into(), value.into());
    }
}
pub fn metadata(bytes: &[u8]) -> Result<Metadata, SoundError> {
    if bytes.len() > crate::MAX_RESOURCE_BYTES {
        return Err(error("metadata input exceeds 16 MiB"));
    }
    let mut result = Metadata {
        snapshots: Vec::new(),
        warnings: Vec::new(),
    };
    let mut fields = Map::new();
    if bytes.starts_with(b"ID3") {
        if bytes.len() < 10 {
            return Err(error("truncated ID3 header"));
        }
        let version = bytes[3];
        if !(3..=4).contains(&version) {
            return Err(error("only ID3v2.3 and v2.4 text tags are supported"));
        }
        let size = sync_size(&bytes[6..10])?;
        if size > MAX_TAG || size + 10 > bytes.len() {
            return Err(error("ID3 tag is truncated or exceeds 1 MiB"));
        }
        let flags = bytes[5];
        if flags & 0xc0 != 0 {
            return Err(error(
                "ID3 unsynchronization and extended headers are outside this profile",
            ));
        }
        let body = &bytes[10..10 + size];
        let mut offset = 0;
        let mut frames = 0;
        while offset < body.len() && body[offset] != 0 {
            if frames >= 256 {
                return Err(error("ID3 tag exceeds 256 frames"));
            }
            frames += 1;
            let header = body
                .get(offset..offset + 10)
                .ok_or_else(|| error("truncated ID3 frame header"))?;
            if !header[..4]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                return Err(error("invalid ID3 frame identifier"));
            }
            let id = std::str::from_utf8(&header[..4]).unwrap();
            let size = if version == 4 {
                sync_size(&header[4..8])?
            } else {
                u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize
            };
            offset += 10;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| error("invalid ID3 frame size"))?;
            let data = body
                .get(offset..end)
                .ok_or_else(|| error("truncated ID3 frame"))?;
            offset = end;
            if header[9] != 0 {
                result.warnings.push(format!("ID3 {id}: compressed, encrypted, grouped or unsynchronized frame is unsupported"));
                continue;
            }
            if (id.starts_with('T') && id != "TXXX") || id == "COMM" {
                let encoding = *data.first().ok_or_else(|| error("empty ID3 text frame"))?;
                let data = if id == "COMM" {
                    after_description(
                        data.get(4..)
                            .ok_or_else(|| error("truncated ID3 comment"))?,
                        encoding,
                    )?
                } else {
                    &data[1..]
                };
                insert(&mut fields, id, text(data, encoding)?);
            } else if id.starts_with('W') && id != "WXXX" {
                if data.len() > MAX_TEXT {
                    return Err(error("ID3 URL frame exceeds 16 KiB"));
                }
                fields.insert(id.into(), latin(data).into());
            } else {
                result.warnings.push(format!(
                    "ID3 {id}: binary/user-defined frame is outside the text metadata profile"
                ));
            }
        }
        result.snapshots.push(fields.clone());
    }
    if bytes.len() >= 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
        let tag = &bytes[bytes.len() - 128..];
        for (name, start, end) in [
            ("songname", 3, 33),
            ("artist", 33, 63),
            ("album", 63, 93),
            ("year", 93, 97),
            ("comment", 97, if tag[125] == 0 { 125 } else { 127 }),
        ] {
            let value = latin(&tag[start..end]);
            if !value.is_empty() {
                fields.insert(name.into(), value.into());
            }
        }
        if tag[125] == 0 && tag[126] != 0 {
            fields.insert("track".into(), tag[126].to_string().into());
        }
        if tag[127] != 255 {
            fields.insert("genre".into(), tag[127].to_string().into());
        }
        result.snapshots.push(fields);
    }
    Ok(result)
}

pub fn resource_path(project: &crate::Project, id: &str) -> Result<Option<String>, SoundError> {
    let Some(resource) = project.resources.iter().find(|r| r.id == id) else {
        return Ok(None);
    };
    if !matches!(
        resource.media_type.as_str(),
        "audio/mpeg" | "audio/mp3" | "audio/wav" | "audio/x-wav" | "audio/wave"
    ) {
        return Err(error(
            "attachSound requires a bundled MP3 or WAV resource ID",
        ));
    }
    Ok(Some(resource.path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_sound_data_family() {
        let result = transform(TransformCommand {
            state: SoundMatrix::default(),
            patch: Some(
                serde_json::from_value(serde_json::json!({"ll":50,"lr":50,"rl":50,"rr":50}))
                    .unwrap(),
            ),
            pan: Some(-30.0),
            volume: Some(20.0),
        })
        .unwrap();
        assert_eq!(result.gains, [0.1; 4]);
        assert_eq!(result.pan, 50.0);
        let pan = transform(TransformCommand {
            state: result.state,
            patch: None,
            pan: Some(-25.0),
            volume: None,
        })
        .unwrap();
        assert_eq!(
            [pan.state.ll, pan.state.rr, pan.state.lr],
            [100.0, 75.0, 0.0]
        );
        assert_eq!(pan.state.volume, 20.0);
        assert!(
            transform(TransformCommand {
                state: SoundMatrix::default(),
                patch: None,
                pan: Some(101.0),
                volume: None
            })
            .is_err()
        );
        let mut body = Vec::new();
        for (id, value) in [
            (b"TIT2", b"\x03NeoFlash".as_slice()),
            (b"TPE1", b"\x03Original author".as_slice()),
            (b"COMM", b"\x03eng\0metadata check".as_slice()),
        ] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(value.len() as u32).to_be_bytes());
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(value);
        }
        let mut bytes = b"ID3\x03\0\0\0\0\0\0".to_vec();
        bytes[9] = body.len() as u8;
        bytes.extend(body);
        bytes.extend([0; 128]);
        let start = bytes.len() - 128;
        bytes[start..start + 3].copy_from_slice(b"TAG");
        bytes[start + 3..start + 10].copy_from_slice(b"V1 name");
        bytes[start + 126] = 4;
        bytes[start + 127] = 255;
        let meta = metadata(&bytes).unwrap();
        assert_eq!(meta.snapshots.len(), 2);
        assert_eq!(meta.snapshots[0]["songname"], "NeoFlash");
        assert_eq!(meta.snapshots[0]["COMM"][0], "metadata check");
        assert_eq!(meta.snapshots[1]["songname"], "V1 name");
        assert_eq!(meta.snapshots[1]["track"], "4");
        bytes[5] = 128;
        assert!(metadata(&bytes).is_err());
        assert!(metadata(b"ID3").is_err());
        assert!(metadata(b"RIFF").unwrap().snapshots.is_empty());
        assert_eq!(text(&[255, 254, 65, 0, 0, 0], 1).unwrap(), "A");
    }
}
