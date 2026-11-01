//! AS2 publication capabilities and input state. Capability query encoding and
//! the separation of host facts from player policy are adapted from Ruffle's
//! MIT system_properties.rs; see docs/AS2-PLATFORM-BATCH.md for the pinned source.
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserFacts {
    language: String,
    platform: String,
    protocol: String,
    screen_width: u32,
    screen_height: u32,
    color_depth: u32,
    audio: bool,
    mp3: bool,
    video: bool,
    composition: bool,
    printing: bool,
}

pub struct ScriptAS2Platform {
    capabilities: Value,
    server_string: String,
    sandbox_type: &'static str,
    caps_lock: bool,
    num_lock: bool,
    has_ime: bool,
    ime_enabled: bool,
    ime_mode: String,
    ime_composition: String,
    allowed_domains: Vec<String>,
    allowed_insecure_domains: Vec<String>,
    policy_files: Vec<String>,
}

impl ScriptAS2Platform {
    pub fn new(facts_json: &str) -> Result<Self, String> {
        if facts_json.len() > 8192 {
            return Err("AS2 browser facts exceed the 8192-byte publication limit".into());
        }
        let facts: BrowserFacts = serde_json::from_str(facts_json)
            .map_err(|error| format!("Invalid AS2 browser facts: {error}"))?;
        let language = language_code(&facts.language);
        let os = if facts.platform.len() > 256 {
            "unknown"
        } else {
            facts.platform.as_str()
        };
        // Report this publication's facilities, never a fabricated Adobe player.
        // In particular a browser capture API does not imply an AS2 encoder.
        let mut capabilities = json!({
            "avHardwareDisable": true, "hasAccessibility": false,
            "hasAudio": facts.audio, "hasAudioEncoder": false,
            "hasEmbeddedVideo": false, "hasIME": facts.composition,
            "hasMP3": facts.audio && facts.mp3, "hasPrinting": facts.printing,
            "hasScreenBroadcast": false, "hasScreenPlayback": false,
            "hasStreamingAudio": facts.audio, "hasStreamingVideo": facts.video,
            "hasVideoEncoder": false, "isDebugger": false,
            "language": language, "localFileReadDisable": true,
            "manufacturer": "NeoFlash", "os": os, "pixelAspectRatio": 1,
            "playerType": "NeoFlashHTML5", "screenColor": if facts.color_depth <= 1 {"bw"} else {"color"},
            "screenDPI": 96, "screenResolutionX": facts.screen_width,
            "screenResolutionY": facts.screen_height,
            "version": concat!("NEO ", env!("CARGO_PKG_VERSION")),
        });
        let fields = [
            ("A", "hasAudio"),
            ("SA", "hasStreamingAudio"),
            ("SV", "hasStreamingVideo"),
            ("EV", "hasEmbeddedVideo"),
            ("MP3", "hasMP3"),
            ("AE", "hasAudioEncoder"),
            ("VE", "hasVideoEncoder"),
            ("ACC", "hasAccessibility"),
            ("PR", "hasPrinting"),
            ("SP", "hasScreenPlayback"),
            ("SB", "hasScreenBroadcast"),
            ("DEB", "isDebugger"),
            ("V", "version"),
            ("M", "manufacturer"),
            ("COL", "screenColor"),
            ("AR", "pixelAspectRatio"),
            ("OS", "os"),
            ("L", "language"),
            ("IME", "hasIME"),
            ("PT", "playerType"),
            ("AVD", "avHardwareDisable"),
            ("LFD", "localFileReadDisable"),
            ("DP", "screenDPI"),
        ];
        let mut pairs = Vec::with_capacity(fields.len() + 1);
        for (key, name) in fields {
            let value = &capabilities[name];
            let text = match value {
                Value::Bool(true) => "t".into(),
                Value::Bool(false) => "f".into(),
                Value::String(text) => text.clone(),
                value => value.to_string(),
            };
            pairs.push(format!("{key}={}", encode_component(&text)));
        }
        pairs.push(format!("R={}x{}", facts.screen_width, facts.screen_height));
        let server_string = pairs.join("&");
        capabilities["serverString"] = Value::String(server_string.clone());
        Ok(Self {
            capabilities,
            server_string,
            sandbox_type: if facts.protocol == "file:" {
                "localWithNetwork"
            } else {
                "remote"
            },
            caps_lock: false,
            num_lock: false,
            has_ime: facts.composition,
            ime_enabled: facts.composition,
            ime_mode: "ALPHANUMERIC_HALF".into(),
            ime_composition: String::new(),
            allowed_domains: Vec::new(),
            allowed_insecure_domains: Vec::new(),
            policy_files: Vec::new(),
        })
    }

    pub fn capabilities_json(&self) -> String {
        self.capabilities.to_string()
    }
    pub fn server_string(&self) -> &str {
        &self.server_string
    }
    pub fn sandbox_type(&self) -> &str {
        self.sandbox_type
    }
    pub fn key_toggle(&mut self, code: u32, enabled: bool) {
        match code {
            20 => self.caps_lock = enabled,
            144 => self.num_lock = enabled,
            _ => {}
        }
    }
    pub fn is_toggled(&self, code: u32) -> bool {
        match code {
            20 => self.caps_lock,
            144 => self.num_lock,
            _ => false,
        }
    }
    pub fn reset(&mut self) {
        self.caps_lock = false;
        self.num_lock = false;
        self.ime_enabled = self.has_ime;
        self.ime_mode = "ALPHANUMERIC_HALF".into();
        self.ime_composition.clear();
        self.allowed_domains.clear();
        self.allowed_insecure_domains.clear();
        self.policy_files.clear();
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 1_056_768 {
            return Err("AS2 platform command exceeds 1 MiB".into());
        }
        let q: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let register_domains = |list: &mut Vec<String>, items: &Value| -> Result<usize, String> {
            let values = items
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 32)
                .ok_or("allowDomain requires 1..=32 domain strings")?;
            for item in values {
                let d = item
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
                    .ok_or("Domain entry requires 1..=256 printable characters")?;
                if !list.iter().any(|existing| existing == d) {
                    if list.len() >= 64 {
                        return Err("At most 64 allowed domains per publication".into());
                    }
                    list.push(d.to_owned());
                }
            }
            Ok(list.len())
        };
        let value = match q["op"].as_str().unwrap_or("") {
            "ime_get_enabled" => json!(self.ime_enabled),
            "ime_set_enabled" => {
                let enabled = q["enabled"]
                    .as_bool()
                    .ok_or("System.IME.setEnabled requires a Boolean")?;
                self.ime_enabled = enabled;
                json!(true)
            }
            "ime_get_mode" => json!(self.ime_mode),
            "ime_set_mode" => {
                let mode = q["mode"]
                    .as_str()
                    .ok_or("System.IME.setConversionMode requires a mode String")?;
                let valid = matches!(
                    mode,
                    "ALPHANUMERIC_FULL"
                        | "ALPHANUMERIC_HALF"
                        | "CHINESE"
                        | "JAPANESE_HIRAGANA"
                        | "JAPANESE_KATAKANA_FULL"
                        | "JAPANESE_KATAKANA_HALF"
                        | "KOREAN"
                        | "UNKNOWN"
                );
                if valid && self.ime_enabled {
                    self.ime_mode = mode.to_owned();
                    json!(true)
                } else {
                    json!(false)
                }
            }
            "ime_set_composition" => {
                let text = q["composition"]
                    .as_str()
                    .filter(|s| s.len() <= 4096)
                    .ok_or(
                        "System.IME.setCompositionString requires a String of at most 4096 bytes",
                    )?;
                if self.ime_enabled {
                    self.ime_composition = text.to_owned();
                    json!(true)
                } else {
                    json!(false)
                }
            }
            "ime_do_conversion" => {
                if self.ime_enabled {
                    json!({"ok": true, "text": self.ime_composition})
                } else {
                    json!({"ok": false, "text": ""})
                }
            }
            "allow_domain" => json!(register_domains(&mut self.allowed_domains, &q["domains"])?),
            "allow_insecure_domain" => json!(register_domains(
                &mut self.allowed_insecure_domains,
                &q["domains"]
            )?),
            "load_policy_file" => {
                let url = q["url"]
                    .as_str()
                    .filter(|s| {
                        !s.is_empty()
                            && s.len() <= 4096
                            && !s.chars().any(char::is_control)
                            && (s.starts_with("http://")
                                || s.starts_with("https://")
                                || s.starts_with("xmlsocket://"))
                    })
                    .ok_or("System.security.loadPolicyFile requires an http://, https:// or xmlsocket:// URL")?;
                if !self.policy_files.iter().any(|existing| existing == url) {
                    if self.policy_files.len() >= 64 {
                        return Err("At most 64 policy file URLs per publication".into());
                    }
                    self.policy_files.push(url.to_owned());
                }
                json!(true)
            }
            "set_clipboard" => {
                q["text"]
                    .as_str()
                    .filter(|s| s.len() <= 1_048_576)
                    .ok_or("System.setClipboard requires a String of at most 1 MiB")?;
                json!(true)
            }
            "show_settings" => {
                let tab = q["tab"]
                    .as_i64()
                    .filter(|t| (0..=3).contains(t))
                    .ok_or("System.showSettings tabID requires an integer from 0 through 3")?;
                json!(tab)
            }
            _ => return Err("Unknown AS2 platform operation".into()),
        };
        Ok(value.to_string())
    }
}

fn language_code(language: &str) -> String {
    let normalized = language.to_ascii_lowercase().replace('_', "-");
    let mut parts = normalized.split('-');
    let base = parts.next().unwrap_or_default();
    if base == "zh" {
        return if parts.any(|part| matches!(part, "tw" | "hk" | "mo" | "hant")) {
            "zh-TW"
        } else {
            "zh-CN"
        }
        .into();
    }
    if (2..=3).contains(&base.len()) && base.bytes().all(|byte| byte.is_ascii_lowercase()) {
        base.into()
    } else {
        "xu".into()
    }
}

fn encode_component(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}

/// Browser PrintJob uses an explicit virtual CSS page, not printer device facts.
#[derive(Default)]
pub struct ScriptAS2Print {
    started: bool,
    sent: bool,
    failed: bool,
    pages: u32,
    bytes: u64,
}
impl ScriptAS2Print {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 8192 {
            return Err("PrintJob command exceeds 8 KiB".into());
        }
        let q: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let number = |name: &str| -> Result<f64, String> {
            q["area"][name]
                .as_f64()
                .filter(|x| x.is_finite() && x.abs() <= 1e6)
                .ok_or_else(|| {
                    format!("Print area {name} requires a finite coordinate within ±1000000")
                })
        };
        let value = match q["op"].as_str().unwrap_or("") {
            "start" => {
                if self.sent || self.started {
                    return Err("PrintJob has already started or been sent".into());
                }
                self.started = q["opened"] == true;
                json!(self.started)
            }
            "properties" => {
                if !self.started {
                    return Err(
                        "PrintJob page properties are available after successful start()".into(),
                    );
                }
                json!({"orientation":"portrait","paperWidth":595,"paperHeight":842,"pageWidth":523,"pageHeight":770,"margin":36})
            }
            "plan" => {
                if !self.started || self.sent || self.failed {
                    Value::Null
                } else {
                    if self.pages >= 8 {
                        return Err("PrintJob is limited to eight pages".into());
                    }
                    let left = number("xMin")?;
                    let top = number("yMin")?;
                    let right = number("xMax")?;
                    let bottom = number("yMax")?;
                    if right <= left || bottom <= top {
                        return Err("Print area must have positive width and height".into());
                    }
                    let x = left.floor();
                    let y = top.floor();
                    let right = right.ceil();
                    let bottom = bottom.ceil();
                    json!({"x":x,"y":y,"width":(right-x).min(523.),"height":(bottom-y).min(770.),"page":self.pages+1})
                }
            }
            "commit" => {
                if !self.started || self.sent || self.failed {
                    json!(false)
                } else {
                    let bytes = q["bytes"]
                        .as_u64()
                        .filter(|n| *n > 0 && *n <= 4 * 1024 * 1024)
                        .ok_or("Print page PNG must contain 1 byte through 4 MiB")?;
                    if self.pages >= 8 || self.bytes + bytes > 32 * 1024 * 1024 {
                        return Err("PrintJob exceeds its eight-page / 32 MiB limit".into());
                    }
                    self.pages += 1;
                    self.bytes += bytes;
                    json!(true)
                }
            }
            "fail" => {
                self.failed = true;
                json!(false)
            }
            "send" => {
                if !self.started || self.sent || self.pages == 0 {
                    return Err(
                        "PrintJob.send requires a started job with at least one captured page"
                            .into(),
                    );
                }
                self.sent = true;
                json!({"pages":self.pages})
            }
            _ => return Err("Unknown PrintJob operation".into()),
        };
        Ok(json!({"value":value}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as2_platform_family() {
        let mut platform = ScriptAS2Platform::new(r#"{"language":"zh-Hant-HK","platform":"Mac & OS","protocol":"https:","screenWidth":1512,"screenHeight":982,"colorDepth":24,"audio":true,"mp3":true,"video":true,"composition":true,"printing":true}"#).unwrap();
        let values: Value = serde_json::from_str(&platform.capabilities_json()).unwrap();
        assert_eq!(values.as_object().unwrap().len(), 26);
        assert_eq!(values["language"], "zh-TW");
        assert_eq!(values["manufacturer"], "NeoFlash");
        assert_eq!(values["screenResolutionX"], 1512);
        assert_eq!(values["hasVideoEncoder"], false);
        assert!(platform.server_string().contains("OS=Mac%20%26%20OS"));
        assert!(platform.server_string().contains("A=t&SA=t&SV=t"));
        assert_eq!(platform.sandbox_type(), "remote");
        platform.key_toggle(20, true);
        platform.key_toggle(144, true);
        assert!(platform.is_toggled(20) && platform.is_toggled(144));
        assert!(!platform.is_toggled(13));
        assert_eq!(
            platform.execute(r#"{"op":"ime_get_enabled"}"#).unwrap(),
            "true"
        );
        assert_eq!(
            platform
                .execute(r#"{"op":"ime_set_mode","mode":"JAPANESE_HIRAGANA"}"#)
                .unwrap(),
            "true"
        );
        assert_eq!(
            platform.execute(r#"{"op":"ime_get_mode"}"#).unwrap(),
            "\"JAPANESE_HIRAGANA\""
        );
        assert_eq!(
            platform
                .execute(r#"{"op":"ime_set_composition","composition":"かな"}"#)
                .unwrap(),
            "true"
        );
        let conv: Value =
            serde_json::from_str(&platform.execute(r#"{"op":"ime_do_conversion"}"#).unwrap())
                .unwrap();
        assert_eq!(conv, json!({"ok":true,"text":"かな"}));
        assert_eq!(
            platform
                .execute(r#"{"op":"allow_domain","domains":["example.com"]}"#)
                .unwrap(),
            "1"
        );
        assert_eq!(
            platform
                .execute(r#"{"op":"load_policy_file","url":"https://example.com/crossdomain.xml"}"#)
                .unwrap(),
            "true"
        );
        assert_eq!(
            platform
                .execute(r#"{"op":"set_clipboard","text":"copied"}"#)
                .unwrap(),
            "true"
        );
        assert_eq!(
            platform
                .execute(r#"{"op":"show_settings","tab":2}"#)
                .unwrap(),
            "2"
        );
        platform.reset();
        assert!(!platform.is_toggled(20));
        assert_eq!(
            platform.execute(r#"{"op":"ime_get_mode"}"#).unwrap(),
            "\"ALPHANUMERIC_HALF\""
        );
        assert_eq!(language_code("en-US"), "en");
        assert_eq!(language_code("zh-CN"), "zh-CN");
        assert_eq!(language_code(""), "xu");
        assert!(ScriptAS2Platform::new(r#"{"screenWidth":-1}"#).is_err());
        let mut print = ScriptAS2Print::new();
        let mut call = |q: Value| -> Value {
            serde_json::from_str(&print.execute(&q.to_string()).unwrap()).unwrap()
        };
        assert_eq!(call(json!({"op":"plan"}))["value"], Value::Null);
        assert_eq!(call(json!({"op":"start","opened":false}))["value"], false);
        assert_eq!(call(json!({"op":"start","opened":true}))["value"], true);
        assert_eq!(call(json!({"op":"properties"}))["value"]["pageWidth"], 523);
        let plan = call(json!({"op":"plan","area":{"xMin":-10,"yMin":5,"xMax":900,"yMax":1000}}));
        assert_eq!(
            plan["value"],
            json!({"x":-10.,"y":5.,"width":523.,"height":770.,"page":1})
        );
        assert_eq!(call(json!({"op":"commit","bytes":1000}))["value"], true);
        assert_eq!(call(json!({"op":"send"}))["value"]["pages"], 1);
        assert_eq!(call(json!({"op":"plan"}))["value"], Value::Null);
    }
}
