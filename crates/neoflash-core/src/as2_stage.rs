//! AS2 Stage layout policy. Browser hosts supply viewport/fullscreen facts only.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MenuItem {
    caption: String,
    enabled: bool,
    visible: bool,
    separator_before: bool,
}
impl MenuItem {
    fn parse(value: Value) -> Result<Self, String> {
        let item: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if item.caption.trim().is_empty()
            || item.caption.encode_utf16().count() > 100
            || item.caption.chars().any(char::is_control)
        {
            return Err("ContextMenuItem caption requires 1–100 UTF-16 units with visible text and no control characters".into());
        }
        Ok(item)
    }
}
const MENU_BUILTINS: &[&str] = &["play", "rewind", "forward_back", "print"];
fn menu_plan(value: &Value) -> Result<Value, String> {
    let values = value["items"]
        .as_array()
        .ok_or("ContextMenu.customItems requires an Array")?;
    if values.len() > 15 {
        return Err("ContextMenu supports at most 15 custom items".into());
    }
    let flags = value["builtInItems"]
        .as_object()
        .ok_or("ContextMenu built-in flags are missing")?;
    if flags
        .keys()
        .any(|key| !MENU_BUILTINS.contains(&key.as_str()))
        || MENU_BUILTINS
            .iter()
            .any(|key| !flags.get(*key).is_some_and(Value::is_boolean))
    {
        return Err("ContextMenu builtInItems supports Boolean play/rewind/forward_back/print; zoom/quality/loop have no browser contract in this profile".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut rows = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let item = MenuItem::parse(value.clone())?;
        let key: String = item
            .caption
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if !seen.insert(key) || !item.visible {
            continue;
        }
        rows.push(json!({"action":"custom","index":index,"caption":item.caption,"enabled":item.enabled,"separatorBefore":item.separator_before}));
    }
    let mut separator = !rows.is_empty();
    for (flag, action, caption) in [
        ("play", "play", "Play"),
        ("play", "stop", "Stop"),
        ("rewind", "rewind", "Rewind"),
        ("forward_back", "previous", "Previous frame"),
        ("forward_back", "next", "Next frame"),
        ("print", "print", "Print publication…"),
    ] {
        if flags[flag] == true {
            rows.push(json!({"action":action,"caption":caption,"enabled":true,"separatorBefore":separator}));
            separator = false;
        }
    }
    Ok(Value::Array(rows))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Clone)]
pub struct StageState {
    width: f64,
    height: f64,
    align: String,
    mode: String,
    menu: bool,
    source: Option<Rect>,
}
impl StageState {
    pub fn new(width: f64, height: f64) -> Result<Self, String> {
        if ![width, height]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0 && *v <= 8192.0)
        {
            return Err("Stage dimensions must be positive and at most 8192".into());
        }
        Ok(Self {
            width,
            height,
            align: String::new(),
            mode: "showAll".into(),
            menu: true,
            source: None,
        })
    }
    fn state(&self) -> Value {
        json!({"align":self.align,"scaleMode":self.mode,"showMenu":self.menu,"fullScreenSourceRect":self.source,"wmodeGPU":false})
    }
    pub fn command(&mut self, input: &str) -> Result<Value, String> {
        if input.len() > 65536 {
            return Err("Stage request exceeds 65536 bytes".into());
        }
        let v: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        match v["op"].as_str().unwrap_or("") {
            "menu_item" => Ok(serde_json::to_value(MenuItem::parse(v["value"].clone())?)
                .map_err(|e| e.to_string())?),
            "menu_plan" => menu_plan(&v),
            "menu_builtin" => {
                let name = v["name"].as_str().ok_or("Missing context-menu flag")?;
                if !MENU_BUILTINS.contains(&name) {
                    return Err("ContextMenu builtInItems supports play/rewind/forward_back/print; zoom/quality/loop are unavailable".into());
                }
                Ok(json!(
                    v["value"]
                        .as_bool()
                        .ok_or("Context-menu flags require Boolean")?
                ))
            }
            "get" => Ok(self.state()),
            "set" => {
                let mut next = self.clone();
                let value = &v["value"];
                match v["name"].as_str().unwrap_or("") {
                    "align" => {
                        let a = value
                            .as_str()
                            .ok_or("Stage.align requires String")?
                            .to_ascii_uppercase();
                        if !matches!(
                            a.as_str(),
                            "" | "T"
                                | "B"
                                | "L"
                                | "R"
                                | "TL"
                                | "TR"
                                | "BL"
                                | "BR"
                                | "LT"
                                | "RT"
                                | "LB"
                                | "RB"
                        ) {
                            return Err(
                                "Stage.align accepts T/B/L/R corner combinations or empty center"
                                    .into(),
                            );
                        }
                        next.align = if a.len() == 2 && matches!(a.as_bytes()[0], b'L' | b'R') {
                            a.chars().rev().collect()
                        } else {
                            a
                        };
                    }
                    "scaleMode" => {
                        let mode = value.as_str().ok_or("Stage.scaleMode requires String")?;
                        if !matches!(mode, "showAll" | "noBorder" | "exactFit" | "noScale") {
                            return Err(
                                "Stage.scaleMode requires showAll/noBorder/exactFit/noScale".into(),
                            );
                        }
                        next.mode = mode.into();
                    }
                    "showMenu" => {
                        next.menu = value.as_bool().ok_or("Stage.showMenu requires Boolean")?
                    }
                    "fullScreenSourceRect" => {
                        next.source = if value.is_null() {
                            None
                        } else {
                            let r: Rect =
                                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                            if ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
                                || r.x < 0.0
                                || r.y < 0.0
                                || r.width <= 0.0
                                || r.height <= 0.0
                                || r.x + r.width > self.width
                                || r.y + r.height > self.height
                            {
                                return Err("Full-screen source must be a positive rectangle inside the authored Stage".into());
                            }
                            Some(r)
                        };
                    }
                    "wmodeGPU" => return Err("Stage.wmodeGPU is read-only".into()),
                    _ => return Err("Unknown AS2 Stage property".into()),
                }
                if next.mode == "noScale" && next.align != "TL" {
                    return Err(
                        "This Stage noScale profile requires align='TL' before scaleMode='noScale'"
                            .into(),
                    );
                }
                *self = next;
                Ok(self.state())
            }
            "layout" => {
                let vw = v["width"].as_f64().ok_or("Missing viewport width")?;
                let vh = v["height"].as_f64().ok_or("Missing viewport height")?;
                if ![vw, vh]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.0 && *v <= 16384.0)
                {
                    return Err("Viewport dimensions must be within (0,16384]".into());
                }
                let fullscreen = v["fullscreen"].as_bool().unwrap_or(false);
                let source = fullscreen.then_some(self.source.as_ref()).flatten();
                let (w, h) = if self.mode == "noScale" && source.is_none() {
                    (vw, vh)
                } else {
                    (self.width, self.height)
                };
                let (sx, sy, ox, oy) = if let Some(rect) = source {
                    let s = (vw / rect.width).min(vh / rect.height);
                    (
                        s,
                        s,
                        (vw - rect.width * s) / 2.0 - rect.x * s,
                        (vh - rect.height * s) / 2.0 - rect.y * s,
                    )
                } else {
                    let (sx, sy) = match self.mode.as_str() {
                        "noScale" => (1.0, 1.0),
                        "exactFit" => (vw / w, vh / h),
                        "noBorder" => {
                            let s = (vw / w).max(vh / h);
                            (s, s)
                        }
                        _ => {
                            let s = (vw / w).min(vh / h);
                            (s, s)
                        }
                    };
                    let ox = if self.align.contains('L') {
                        0.0
                    } else if self.align.contains('R') {
                        vw - w * sx
                    } else {
                        (vw - w * sx) / 2.0
                    };
                    let oy = if self.align.contains('T') {
                        0.0
                    } else if self.align.contains('B') {
                        vh - h * sy
                    } else {
                        (vh - h * sy) / 2.0
                    };
                    (sx, sy, ox, oy)
                };
                Ok(
                    json!({"scale":sx,"scaleY":sy,"stageWidth":w*sx,"stageHeight":h*sy,"left":ox,"top":oy,"spaceWidth":vw,"spaceHeight":vh,"logicalWidth":w,"logicalHeight":h}),
                )
            }
            _ => Err("Unknown AS2 Stage request".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_layout_family() {
        let mut s = StageState::new(800.0, 600.0).unwrap();
        let view = s
            .command(r#"{"op":"layout","width":1200,"height":600}"#)
            .unwrap();
        assert_eq!(view["left"], 200.0);
        assert_eq!(view["scale"], 1.0);
        s.command(r#"{"op":"set","name":"scaleMode","value":"exactFit"}"#)
            .unwrap();
        let view = s
            .command(r#"{"op":"layout","width":1200,"height":600}"#)
            .unwrap();
        assert_eq!(view["scale"], 1.5);
        assert_eq!(view["scaleY"], 1.0);
        assert!(
            s.command(r#"{"op":"set","name":"scaleMode","value":"noScale"}"#)
                .is_err()
        );
        assert_eq!(s.state()["scaleMode"], "exactFit");
        s.command(r#"{"op":"set","name":"align","value":"lt"}"#)
            .unwrap();
        s.command(r#"{"op":"set","name":"scaleMode","value":"noScale"}"#)
            .unwrap();
        let view = s
            .command(r#"{"op":"layout","width":1200,"height":700}"#)
            .unwrap();
        assert_eq!(view["logicalWidth"], 1200.0);
        assert_eq!(view["stageHeight"], 700.0);
        s.command(r#"{"op":"set","name":"fullScreenSourceRect","value":{"x":100,"y":100,"width":400,"height":300}}"#).unwrap();
        let view = s
            .command(r#"{"op":"layout","width":1200,"height":900,"fullscreen":true}"#)
            .unwrap();
        assert_eq!(view["scale"], 3.0);
        assert_eq!(view["left"], -300.0);
        assert!(s.command(r#"{"op":"set","name":"fullScreenSourceRect","value":{"x":799,"y":0,"width":2,"height":1}}"#).is_err());
        let menu = json!({"op":"menu_plan","items":[
            {"caption":"Resume","enabled":true,"visible":true,"separatorBefore":false},
            {"caption":"Re sume!","enabled":true,"visible":true,"separatorBefore":false},
            {"caption":"Hidden","enabled":true,"visible":false,"separatorBefore":false},
            {"caption":"Disabled","enabled":false,"visible":true,"separatorBefore":true}
        ],"builtInItems":{"play":true,"rewind":false,"forward_back":false,"print":false}});
        let rows = s.command(&menu.to_string()).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 4);
        assert_eq!(rows[1]["index"], 3);
        assert_eq!(rows[1]["enabled"], false);
        assert_eq!(rows[2]["action"], "play");
        assert!(
            s.command(r#"{"op":"menu_builtin","name":"quality","value":true}"#)
                .is_err()
        );
        assert_eq!(s.command(r#"{"op":"get"}"#).unwrap()["wmodeGPU"], false);
        assert!(
            s.command(r#"{"op":"set","name":"wmodeGPU","value":true}"#)
                .is_err()
        );
    }
}
