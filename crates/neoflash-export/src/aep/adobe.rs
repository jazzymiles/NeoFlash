//! Ask a separate local After Effects process to serialize the editable project with Adobe's writer.
use super::*;

#[cfg(target_os = "macos")]
pub(super) fn save(script: &str, project: &Path, xml: Option<&Path>) -> Result<(), String> {
    use std::ffi::{CStr, CString, c_char};
    unsafe extern "C" {
        fn nf_ae_write_project(
            token: *const c_char,
            command: *const c_char,
            artifact: *const c_char,
            script_error: *const c_char,
            error: *mut c_char,
            capacity: usize,
        ) -> i32;
    }
    let parent = project
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let scratch = tempfile::Builder::new()
        .prefix("neoflash-adobe-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let builder = scratch.path().join("build.jsx");
    fs::write(&builder, script).map_err(|e| e.to_string())?;
    let target = |suffix: &str| {
        tempfile::Builder::new()
            .prefix(".neoflash-adobe-")
            .suffix(suffix)
            .tempfile_in(&parent)
            .map(|f| f.into_temp_path())
            .map_err(|e| e.to_string())
    };
    let binary = target(".aep")?;
    let xml_temp = xml.map(|_| target(".aepx")).transpose()?;
    let token = scratch.path().file_name().unwrap().to_string_lossy();
    let fields = serde_json::json!({"file":binary.to_str(),"xml":xml_temp.as_ref().and_then(|p|p.to_str()),"folder":parent.to_str()});
    let js_path = |p: &Path| serde_json::to_string(&p.to_string_lossy()).expect("JSON string");
    let error_path = scratch.path().join("error.txt");
    let command = format!(
        r#"(function() {{
        if ($.getenv("NEOFLASH_AE_SESSION") !== {nonce} || !app.project || app.project.file || app.project.numItems) return;
        app.beginSuppressDialogs();
        try {{
            $.global.__neoFlashRequest = {fields};
            var result = $.evalFile(new File({builder}));
            if (result instanceof Error) throw result;
        }} catch (e) {{
            try {{ var log = new File({err}); if (log.open("w")) {{log.write(String(e)); log.close();}} }} catch (_) {{}}
        }} finally {{ delete $.global.__neoFlashRequest; app.endSuppressDialogs(false); }}
    }})();"#,
        nonce = serde_json::to_string(&token).unwrap(),
        builder = js_path(&builder),
        err = js_path(&error_path)
    );
    let token = CString::new(token.as_bytes()).map_err(|e| e.to_string())?;
    let command = CString::new(command).map_err(|e| e.to_string())?;
    let artifact = xml_temp.as_deref().unwrap_or(&binary);
    let artifact =
        CString::new(artifact.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
    let script_error =
        CString::new(error_path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
    let mut message = [0 as c_char; 768];
    // Both C strings and the output buffer remain live until the synchronous Cocoa call returns.
    if unsafe {
        nf_ae_write_project(
            token.as_ptr(),
            command.as_ptr(),
            artifact.as_ptr(),
            script_error.as_ptr(),
            message.as_mut_ptr(),
            message.len(),
        )
    } != 0
    {
        return Err(unsafe { CStr::from_ptr(message.as_ptr()) }
            .to_string_lossy()
            .into_owned());
    }
    if let Ok(message) = fs::read_to_string(error_path) {
        return Err(message);
    }
    let mut staged = vec![(binary.to_path_buf(), project.to_owned())];
    let prefix = |path: &Path, size: usize| {
        use std::io::Read;
        let mut data = vec![];
        fs::File::open(path)
            .and_then(|file| file.take(size as u64).read_to_end(&mut data))
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(data)
    };
    let header = prefix(&binary, 12)?;
    if !header.starts_with(b"RIFX") || header.get(8..12) != Some(b"Egg!") {
        return Err("Adobe did not save a native project in the isolated export session.".into());
    }
    if let (Some(temp), Some(path)) = (&xml_temp, xml) {
        if !String::from_utf8_lossy(&prefix(temp, 1024)?)
            .contains("http://www.adobe.com/products/aftereffects")
        {
            return Err("Adobe did not save a native XML project.".into());
        }
        staged.push((temp.to_path_buf(), path.to_owned()));
    }
    crate::commit_staged_files_atomically(&staged).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn save(_script: &str, _project: &Path, _xml: Option<&Path>) -> Result<(), String> {
    Err("The native Adobe writer is available on macOS.".into())
}
