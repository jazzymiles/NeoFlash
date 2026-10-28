//! AS2 non-mask transitions over existing display properties and Rust easing.
use crate::as_easing::ease;
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Clone, Deserialize)]
struct Point {
    x: f64,
    y: f64,
}
#[derive(Clone, Deserialize)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Clone, Deserialize)]
struct Appearance {
    #[serde(rename = "_x")]
    x: f64,
    #[serde(rename = "_y")]
    y: f64,
    #[serde(rename = "_xscale")]
    sx: f64,
    #[serde(rename = "_yscale")]
    sy: f64,
    #[serde(rename = "_alpha")]
    alpha: f64,
    #[serde(rename = "_rotation")]
    rotation: f64,
}
#[derive(Deserialize)]
struct Config {
    kind: String,
    direction: u8,
    duration: f64,
    family: String,
    mode: String,
    base: Appearance,
    outer: Rect,
    viewport: Vec<Point>,
    #[serde(default = "default_point", rename = "startPoint")]
    start_point: u8,
    #[serde(default)]
    dimension: u8,
    #[serde(default = "default_degrees")]
    degrees: f64,
    #[serde(default)]
    ccw: bool,
}
fn default_point() -> u8 {
    5
}
fn default_degrees() -> f64 {
    360.0
}
pub struct ScriptAS2Transition {
    config: Config,
    started: f64,
    from_x: f64,
    from_y: f64,
}
impl ScriptAS2Transition {
    pub fn new(input: &str, now: f64) -> Result<Self, String> {
        if input.len() > 32768 {
            return Err("Transition configuration exceeds 32 KiB".into());
        }
        let c: Config = serde_json::from_str(input)
            .map_err(|e| format!("Invalid transition configuration: {e}"))?;
        if !matches!(
            c.kind.as_str(),
            "Fade" | "Fly" | "Rotate" | "Zoom" | "Squeeze"
        ) {
            return Err(
                "Only Fade, Fly, Rotate, Zoom and Squeeze transitions are supported".into(),
            );
        }
        if ![1, 2].contains(&c.direction)
            || !c.duration.is_finite()
            || c.duration <= 0.0
            || c.duration > 3600.0
            || !now.is_finite()
        {
            return Err("Transition needs IN/OUT and duration in (0, 3600] seconds".into());
        }
        let b = &c.base;
        let rect = &c.outer;
        if ![
            b.x,
            b.y,
            b.sx,
            b.sy,
            b.alpha,
            b.rotation,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            c.degrees,
        ]
        .iter()
        .all(|x| x.is_finite())
            || !(1..=9).contains(&c.start_point)
            || c.dimension > 1
            || c.degrees.abs() > 9999.0
        {
            return Err("Transition appearance or type parameters are invalid".into());
        }
        if c.viewport.len() != 4
            || c.viewport
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            return Err("Transition viewport requires four finite parent-space corners".into());
        }
        ease(
            &c.family,
            &c.mode,
            0.0,
            0.0,
            1.0,
            c.duration,
            f64::NAN,
            f64::NAN,
        )?;
        let left = c.viewport.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let right = c
            .viewport
            .iter()
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let top = c.viewport.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let bottom = c
            .viewport
            .iter()
            .map(|p| p.y)
            .fold(f64::NEG_INFINITY, f64::max);
        let column = (c.start_point - 1) % 3;
        let row = (c.start_point - 1) / 3;
        let from_x = match column {
            0 => b.x + left - rect.x - rect.width - 1.0,
            2 => b.x + right - rect.x + 1.0,
            _ => b.x,
        };
        let from_y = match row {
            0 => b.y + top - rect.y - rect.height - 1.0,
            2 => b.y + bottom - rect.y + 1.0,
            _ => b.y,
        };
        Ok(Self {
            config: c,
            started: now,
            from_x,
            from_y,
        })
    }
    pub fn sample(&self, now: f64) -> Result<Value, String> {
        if !now.is_finite() {
            return Err("Transition clock must be finite".into());
        }
        let c = &self.config;
        let t = ((now - self.started) / 1000.0).clamp(0.0, c.duration);
        let done = t >= c.duration;
        let amount = if done {
            1.0
        } else {
            ease(
                &c.family,
                &c.mode,
                t,
                0.0,
                1.0,
                c.duration,
                f64::NAN,
                f64::NAN,
            )?
        };
        let p = if c.direction == 1 {
            amount
        } else {
            1.0 - amount
        };
        let b = &c.base;
        let patch = match c.kind.as_str() {
            "Fade" => json!({"_alpha":b.alpha*p}),
            "Zoom" => json!({"_xscale":b.sx*p,"_yscale":b.sy*p}),
            "Squeeze" => {
                if c.dimension == 0 {
                    json!({"_xscale":b.sx*p})
                } else {
                    json!({"_yscale":b.sy*p})
                }
            }
            "Rotate" => json!({"_rotation":b.rotation+(p-1.0)*c.degrees*if c.ccw{-1.0}else{1.0}}),
            "Fly" => {
                json!({"_x":self.from_x+(b.x-self.from_x)*p,"_y":self.from_y+(b.y-self.from_y)*p})
            }
            _ => unreachable!(),
        };
        Ok(json!({"patch":patch,"done":done,"direction":c.direction,"progress":p}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_transitions_family() {
        let base = json!({"kind":"Fade","direction":1,"duration":2.0,"family":"Linear","mode":"easeNone","base":{"_x":100.0,"_y":50.0,"_xscale":100.0,"_yscale":80.0,"_alpha":90.0,"_rotation":10.0},"outer":{"x":100.0,"y":50.0,"width":60.0,"height":40.0},"viewport":[{"x":0.0,"y":0.0},{"x":800.0,"y":0.0},{"x":0.0,"y":600.0},{"x":800.0,"y":600.0}],"startPoint":1,"dimension":1,"degrees":720.0});
        for kind in ["Fade", "Fly", "Rotate", "Zoom", "Squeeze"] {
            let mut config = base.clone();
            config["kind"] = json!(kind);
            let t = ScriptAS2Transition::new(&config.to_string(), 100.0).unwrap();
            assert_eq!(t.sample(100.0).unwrap()["done"], false);
            assert_eq!(t.sample(2100.0).unwrap()["done"], true);
            assert_eq!(t.sample(1100.0).unwrap()["progress"], 0.5);
        }
        let fade = ScriptAS2Transition::new(&base.to_string(), 0.0).unwrap();
        assert_eq!(fade.sample(1000.0).unwrap()["patch"]["_alpha"], 45.0);
        let mut out = base.clone();
        out["direction"] = json!(2);
        assert_eq!(
            ScriptAS2Transition::new(&out.to_string(), 0.0)
                .unwrap()
                .sample(2000.0)
                .unwrap()["progress"],
            0.0
        );
        out["duration"] = json!(0);
        assert!(ScriptAS2Transition::new(&out.to_string(), 0.0).is_err());
    }
}
