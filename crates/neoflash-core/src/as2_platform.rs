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
        platform.reset();
        assert!(!platform.is_toggled(20));
        assert_eq!(language_code("en-US"), "en");
        assert_eq!(language_code("zh-CN"), "zh-CN");
        assert_eq!(language_code(""), "xu");
        assert!(ScriptAS2Platform::new(r#"{"screenWidth":-1}"#).is_err());
    }
}
