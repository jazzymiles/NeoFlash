//! Scripted easing curves adapted into owned Rust calculations.
//! Penner equations: Copyright © 2001 Robert Penner. BSD-3-Clause.
//! Source pin, adaptations and retained notice: docs/RUNTIME-EASING-PROVENANCE.md.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Flash's (time, beginning, change, duration) signature. Optional parameters
/// are overshoot for Back, or amplitude/period for Elastic; NaN means omitted.
#[allow(clippy::too_many_arguments)]
pub fn ease(
    family: &str,
    mode: &str,
    time: f64,
    begin: f64,
    change: f64,
    duration: f64,
    parameter: f64,
    period: f64,
) -> Result<f64, String> {
    if [time, begin, change, duration]
        .iter()
        .any(|n| !n.is_finite())
        || duration <= 0.0
    {
        return Err(
            "Easing requires finite time/begin/change and a positive finite duration".into(),
        );
    }
    if (!parameter.is_finite() && !parameter.is_nan()) || (!period.is_finite() && !period.is_nan())
    {
        return Err("Optional easing parameters must be finite or omitted".into());
    }
    let mut t = time / duration;
    let direction = match mode {
        "easeIn" => 0,
        "easeOut" => 1,
        "easeInOut" => 2,
        "easeNone" if family == "Linear" => 0,
        _ => return Err(format!("Unsupported easing direction '{mode}'")),
    };
    let ratio = match family {
        "Linear" => t,
        "Quadratic" | "Cubic" | "Quartic" | "Quintic" => {
            let power = match family {
                "Quadratic" => 2,
                "Cubic" => 3,
                "Quartic" => 4,
                _ => 5,
            };
            match direction {
                0 => t.powi(power),
                1 => 1.0 - (1.0 - t).powi(power),
                _ if t < 0.5 => (t * 2.0).powi(power) * 0.5,
                _ => 1.0 - (2.0 - t * 2.0).powi(power) * 0.5,
            }
        }
        "Sine" => match direction {
            0 => 1.0 - (t * FRAC_PI_2).cos(),
            1 => (t * FRAC_PI_2).sin(),
            _ => (1.0 - (PI * t).cos()) * 0.5,
        },
        "Exponential" => match direction {
            0 => {
                if t == 0.0 {
                    0.0
                } else {
                    2.0_f64.powf(10.0 * (t - 1.0))
                }
            }
            1 => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * t)
                }
            }
            _ if t == 0.0 || t == 1.0 => t,
            _ if t < 0.5 => 0.5 * 2.0_f64.powf(10.0 * (t * 2.0 - 1.0)),
            _ => 0.5 * (2.0 - 2.0_f64.powf(-10.0 * (t * 2.0 - 1.0))),
        },
        "Circular" => match direction {
            0 => 1.0 - (1.0 - t * t).sqrt(),
            1 => {
                t -= 1.0;
                (1.0 - t * t).sqrt()
            }
            _ if t < 0.5 => {
                t *= 2.0;
                (1.0 - (1.0 - t * t).sqrt()) * 0.5
            }
            _ => {
                t = t * 2.0 - 2.0;
                ((1.0 - t * t).sqrt() + 1.0) * 0.5
            }
        },
        "Back" => {
            let mut s = if parameter.is_nan() {
                1.70158
            } else {
                parameter
            };
            match direction {
                0 => t * t * ((s + 1.0) * t - s),
                1 => {
                    t -= 1.0;
                    t * t * ((s + 1.0) * t + s) + 1.0
                }
                _ => {
                    s *= 1.525;
                    t *= 2.0;
                    if t < 1.0 {
                        0.5 * t * t * ((s + 1.0) * t - s)
                    } else {
                        t -= 2.0;
                        0.5 * (t * t * ((s + 1.0) * t + s) + 2.0)
                    }
                }
            }
        }
        "Bounce" => match direction {
            0 => 1.0 - bounce_out(1.0 - t),
            1 => bounce_out(t),
            _ if t < 0.5 => (1.0 - bounce_out(1.0 - t * 2.0)) * 0.5,
            _ => bounce_out(t * 2.0 - 1.0) * 0.5 + 0.5,
        },
        "Elastic" => {
            // The original equations use change-valued amplitude, so return the
            // complete value here instead of normalizing amplitude in the host.
            if time == 0.0 {
                return Ok(begin);
            }
            if time == duration {
                return finite_result(begin + change);
            }
            let p = if period.is_nan() || period == 0.0 {
                duration * if direction == 2 { 0.45 } else { 0.3 }
            } else {
                period
            };
            if p <= 0.0 {
                return Err("Elastic period must be positive or omitted".into());
            }
            let mut a = if parameter.is_nan() { 0.0 } else { parameter };
            let s = if a == 0.0 || a < change.abs() {
                a = change;
                p / 4.0
            } else {
                p / TAU * (change / a).asin()
            };
            let value = match direction {
                0 => {
                    t -= 1.0;
                    -(a * 2.0_f64.powf(10.0 * t) * ((t * duration - s) * TAU / p).sin()) + begin
                }
                1 => {
                    a * 2.0_f64.powf(-10.0 * t) * ((t * duration - s) * TAU / p).sin()
                        + change
                        + begin
                }
                _ => {
                    t = t * 2.0 - 1.0;
                    if t < 0.0 {
                        -0.5 * a * 2.0_f64.powf(10.0 * t) * ((t * duration - s) * TAU / p).sin()
                            + begin
                    } else {
                        0.5 * a * 2.0_f64.powf(-10.0 * t) * ((t * duration - s) * TAU / p).sin()
                            + change
                            + begin
                    }
                }
            };
            return finite_result(value);
        }
        _ => return Err(format!("Unsupported easing family '{family}'")),
    };
    finite_result(begin + change * ratio)
}
fn finite_result(value: f64) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Easing produced a non-finite result outside the current profile".into())
    }
}
fn bounce_out(mut t: f64) -> f64 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        t -= 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        t -= 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        t -= 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

/// Programmatic CustomEase points omit the fixed (0,0)/(1,1) endpoints.
/// Cubic coefficient evaluation and bounded subdivision are adapted from
/// gre/bezier-easing v2.1.0 (MIT); see the provenance document.
pub fn custom_ease(
    points: &[f64],
    time: f64,
    begin: f64,
    change: f64,
    duration: f64,
) -> Result<f64, String> {
    if [time, begin, change, duration]
        .iter()
        .any(|x| !x.is_finite())
        || duration <= 0.0
        || time < 0.0
        || time > duration
    {
        return Err(
            "CustomEase requires finite values, positive duration and time within 0..duration"
                .into(),
        );
    }
    if points.is_empty() {
        return finite_result(begin + change * time / duration);
    }
    if points.len() > 382
        || points.len() % 6 != 4
        || points
            .iter()
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return Err("CustomEase points require 2, 5, 8, ... up to 191 normalized finite control/anchor points".into());
    }
    let mut curve = Vec::with_capacity(points.len() / 2 + 2);
    curve.push([0.0, 0.0]);
    curve.extend(points.as_chunks::<2>().0.iter().copied());
    curve.push([1.0, 1.0]);
    for index in (0..curve.len() - 1).step_by(3) {
        let [a, b, c, d] = [
            curve[index],
            curve[index + 1],
            curve[index + 2],
            curve[index + 3],
        ];
        if d[0] <= a[0] || b[0] < a[0] || b[0] > d[0] || c[0] < a[0] || c[0] > d[0] {
            return Err("CustomEase anchors must advance in x and controls must stay inside their segment's x range".into());
        }
    }
    let x = time / duration;
    if x == 0.0 {
        return Ok(begin);
    }
    if x == 1.0 {
        return finite_result(begin + change);
    }
    for index in (0..curve.len() - 1).step_by(3) {
        let [a, b, c, d] = [
            curve[index],
            curve[index + 1],
            curve[index + 2],
            curve[index + 3],
        ];
        if x > d[0] {
            continue;
        }
        let mut lo = 0.0;
        let mut hi = 1.0;
        // Fixed bounded work avoids divergent Newton iterations at flat tangents.
        for _ in 0..40 {
            let mid = (lo + hi) * 0.5;
            if cubic_coordinate(mid, a[0], b[0], c[0], d[0]) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return finite_result(
            begin + change * cubic_coordinate((lo + hi) * 0.5, a[1], b[1], c[1], d[1]),
        );
    }
    Err("CustomEase curve does not cover the requested time".into())
}

fn cubic_coordinate(t: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    (((d - a + 3.0 * (b - c)) * t + 3.0 * (a - 2.0 * b + c)) * t + 3.0 * (b - a)) * t + a
}

/// Publication-independent scripted tween state. Host object references, clocks
/// and dispatching stay at the transport boundary; time/value changes stay here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptTweenState {
    pub begin: f64,
    pub finish: f64,
    pub duration: f64,
    pub time: f64,
    pub position: f64,
    pub fps: f64,
    pub use_seconds: bool,
    pub looping: bool,
    pub is_playing: bool,
    pub family: String,
    pub mode: String,
    #[serde(default)]
    pub custom_points: Vec<f64>,
    #[serde(skip)]
    last_clock: f64,
}
impl ScriptTweenState {
    pub fn new(value: &serde_json::Value, now: f64) -> Result<Self, String> {
        let number = |name: &str| {
            value[name]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("Tween {name} must be finite"))
        };
        let begin = number("begin")?;
        let finish = number("finish")?;
        let duration = number("duration")?;
        let fps = number("fps")?;
        if duration <= 0.0 || !(0.0..=240.0).contains(&fps) || fps == 0.0 || !now.is_finite() {
            return Err(
                "Tween duration must be positive; FPS must be greater than 0 and at most 240"
                    .into(),
            );
        }
        let family = value["family"]
            .as_str()
            .ok_or("Tween easing family is missing")?
            .to_owned();
        let mode = value["mode"]
            .as_str()
            .ok_or("Tween easing mode is missing")?
            .to_owned();
        let custom_points: Vec<f64> = if value["customPoints"].is_null() {
            Vec::new()
        } else {
            serde_json::from_value(value["customPoints"].clone())
                .map_err(|_| "Invalid CustomEase point data")?
        };
        let position = if family == "Custom" {
            custom_ease(&custom_points, 0.0, begin, finish - begin, duration)?
        } else {
            ease(
                &family,
                &mode,
                0.0,
                begin,
                finish - begin,
                duration,
                f64::NAN,
                f64::NAN,
            )?
        };
        Ok(Self {
            begin,
            finish,
            duration,
            time: 0.0,
            position,
            fps,
            use_seconds: value["useSeconds"].as_bool().unwrap_or(false),
            looping: false,
            is_playing: true,
            family,
            mode,
            custom_points,
            last_clock: now,
        })
    }
    pub fn command(
        &mut self,
        operation: &str,
        args: &serde_json::Value,
        now: f64,
    ) -> Result<serde_json::Value, String> {
        if !now.is_finite() {
            return Err("Tween requires a finite clock".into());
        }
        let values = args
            .as_array()
            .ok_or("Tween command arguments must be an array")?;
        let number = |i: usize| {
            values
                .get(i)
                .and_then(serde_json::Value::as_f64)
                .filter(|n| n.is_finite())
                .ok_or_else(|| "Tween argument must be finite".to_owned())
        };
        let boolean = |i: usize| {
            values
                .get(i)
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| "Tween flag must be Boolean".to_owned())
        };
        let mut next = self.clone();
        let mut events = Vec::new();
        match operation {
            "tick" if !next.is_playing => {}
            "tick" => {
                if now < next.last_clock {
                    return Err("Tween clock must be monotonic".into());
                }
                let step = if next.use_seconds {
                    (now - next.last_clock) / 1000.0
                } else {
                    1.0
                };
                next.last_clock = now;
                next.seek(next.time + step, &mut events)?;
            }
            "start" => {
                next.is_playing = true;
                next.last_clock = now;
                next.seek(0.0, &mut events)?;
                events.push("motionStart");
            }
            "stop" => {
                if next.is_playing {
                    next.is_playing = false;
                    events.push("motionStop");
                }
            }
            "resume" => {
                if !next.is_playing {
                    next.is_playing = true;
                    next.last_clock = now;
                    events.push("motionResume");
                }
            }
            "rewind" => {
                next.last_clock = now;
                next.seek(
                    if values.is_empty() { 0.0 } else { number(0)? },
                    &mut events,
                )?;
            }
            "fforward" => {
                next.seek(next.duration, &mut events)?;
            }
            "nextFrame" | "prevFrame" => {
                if next.use_seconds {
                    return Err("Tween nextFrame/prevFrame support frame-based tweens only".into());
                }
                if next.is_playing {
                    return Err("Stop a Tween before stepping individual frames".into());
                }
                next.seek(
                    next.time + if operation == "nextFrame" { 1.0 } else { -1.0 },
                    &mut events,
                )?;
            }
            "continueTo" => {
                next.begin = next.position;
                next.finish = number(0)?;
                if values.len() > 1 {
                    next.duration = number(1)?;
                }
                if next.duration <= 0.0 {
                    return Err("Tween duration must be positive".into());
                }
                next.is_playing = true;
                next.last_clock = now;
                next.seek(0.0, &mut events)?;
                events.push("motionStart");
            }
            "yoyo" => {
                let finish = next.begin;
                next.begin = next.position;
                next.finish = finish;
                if next.time > 0.0 {
                    next.duration = next.time;
                }
                next.is_playing = true;
                next.last_clock = now;
                next.seek(0.0, &mut events)?;
                events.push("motionStart");
            }
            "set:time" => {
                next.last_clock = now;
                next.seek(number(0)?, &mut events)?;
            }
            "set:position" => {
                next.position = number(0)?;
                events.push("motionChange");
            }
            "set:begin" => {
                next.begin = number(0)?;
            }
            "set:finish" => {
                next.finish = number(0)?;
            }
            "set:duration" => {
                next.duration = number(0)?;
                if next.duration <= 0.0 {
                    return Err("Tween duration must be positive".into());
                }
            }
            "set:fps" => {
                next.fps = number(0)?;
                if next.fps <= 0.0 || next.fps > 240.0 {
                    return Err("Tween FPS must be greater than 0 and at most 240".into());
                }
            }
            "set:looping" => next.looping = boolean(0)?,
            "set:useSeconds" => {
                next.use_seconds = boolean(0)?;
                next.last_clock = now;
            }
            "set:curve" => {
                next.family = values
                    .first()
                    .and_then(serde_json::Value::as_str)
                    .ok_or("Tween curve family is missing")?
                    .to_owned();
                next.mode = values
                    .get(1)
                    .and_then(serde_json::Value::as_str)
                    .ok_or("Tween curve direction is missing")?
                    .to_owned();
                next.custom_points = if values.get(2).is_none_or(serde_json::Value::is_null) {
                    Vec::new()
                } else {
                    serde_json::from_value(values[2].clone())
                        .map_err(|_| "Invalid CustomEase point data")?
                };
                if next.family == "Custom" {
                    custom_ease(
                        &next.custom_points,
                        0.0,
                        next.begin,
                        next.finish - next.begin,
                        next.duration,
                    )?;
                } else {
                    ease(
                        &next.family,
                        &next.mode,
                        0.0,
                        next.begin,
                        next.finish - next.begin,
                        next.duration,
                        f64::NAN,
                        f64::NAN,
                    )?;
                }
            }
            _ => return Err(format!("Unsupported Tween operation '{operation}'")),
        }
        *self = next;
        Ok(serde_json::json!({"state":self,"events":events}))
    }
    fn seek(&mut self, time: f64, events: &mut Vec<&'static str>) -> Result<(), String> {
        self.time = time.max(0.0);
        let mut finished = false;
        let mut looped = false;
        if self.time >= self.duration {
            if self.looping {
                self.time %= self.duration;
                looped = true;
            } else {
                self.time = self.duration;
                self.is_playing = false;
                finished = true;
            }
        }
        self.position = if self.family == "Custom" {
            custom_ease(
                &self.custom_points,
                self.time,
                self.begin,
                self.finish - self.begin,
                self.duration,
            )?
        } else {
            ease(
                &self.family,
                &self.mode,
                self.time,
                self.begin,
                self.finish - self.begin,
                self.duration,
                f64::NAN,
                f64::NAN,
            )?
        };
        events.push("motionChange");
        if looped {
            events.push("motionLoop");
        }
        if finished {
            events.push("motionFinish");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripted_easing_family() {
        let simple = [0.25, 0.1, 0.25, 1.0];
        assert_eq!(custom_ease(&simple, 0.0, 10.0, 90.0, 1.0).unwrap(), 10.0);
        assert_eq!(custom_ease(&simple, 1.0, 10.0, 90.0, 1.0).unwrap(), 100.0);
        assert!((custom_ease(&simple, 0.5, 0.0, 1.0, 1.0).unwrap() - 0.8024033876).abs() < 1e-8);
        let multi = [0.1, 0.1, 0.2, 0.2, 0.5, 0.5, 0.6, 0.6, 0.9, 0.9];
        assert!((custom_ease(&multi, 0.7, 10.0, -20.0, 1.0).unwrap() + 4.0).abs() < 1e-8);
        assert!(
            custom_ease(
                &[0.9, 0.1, 0.8, 0.2, 0.5, 0.5, 0.6, 0.6, 0.9, 0.9],
                0.5,
                0.0,
                1.0,
                1.0
            )
            .is_err()
        );
        for family in [
            "Linear",
            "Quadratic",
            "Cubic",
            "Quartic",
            "Quintic",
            "Sine",
            "Exponential",
            "Circular",
            "Back",
            "Bounce",
            "Elastic",
        ] {
            for mode in ["easeIn", "easeOut", "easeInOut"] {
                let value =
                    |time| ease(family, mode, time, 10.0, 90.0, 2.0, f64::NAN, f64::NAN).unwrap();
                assert!((value(0.0) - 10.0).abs() < 1e-9, "{family}.{mode} start");
                assert!((value(2.0) - 100.0).abs() < 1e-9, "{family}.{mode} finish");
                assert!(value(0.73).is_finite(), "{family}.{mode} interior");
            }
        }
        assert_eq!(
            ease(
                "Quadratic",
                "easeIn",
                0.5,
                0.0,
                100.0,
                1.0,
                f64::NAN,
                f64::NAN
            )
            .unwrap(),
            25.0
        );
        assert_eq!(
            ease(
                "Quintic",
                "easeOut",
                0.5,
                0.0,
                100.0,
                1.0,
                f64::NAN,
                f64::NAN
            )
            .unwrap(),
            96.875
        );
        assert!(ease("Back", "easeOut", 0.7, 0.0, 100.0, 1.0, 2.0, f64::NAN).unwrap() > 100.0);
        assert!(ease("Circular", "easeIn", 2.0, 0.0, 1.0, 1.0, f64::NAN, f64::NAN).is_err());
        assert!(ease("Linear", "easeNone", 0.0, 0.0, 1.0, 0.0, f64::NAN, f64::NAN).is_err());
        let mut tween=ScriptTweenState::new(&serde_json::json!({"begin":10.0,"finish":110.0,"duration":4.0,"fps":24.0,"useSeconds":false,"family":"Quadratic","mode":"easeIn"}),0.0).unwrap();
        tween.command("stop", &serde_json::json!([]), 0.0).unwrap();
        tween
            .command("nextFrame", &serde_json::json!([]), 0.0)
            .unwrap();
        assert_eq!(tween.position, 16.25);
        tween
            .command("resume", &serde_json::json!([]), 0.0)
            .unwrap();
        tween.command("tick", &serde_json::json!([]), 40.0).unwrap();
        assert_eq!(tween.position, 35.0);
        tween
            .command("fforward", &serde_json::json!([]), 40.0)
            .unwrap();
        assert_eq!(tween.position, 110.0);
        assert!(!tween.is_playing);
        tween.command("yoyo", &serde_json::json!([]), 40.0).unwrap();
        tween
            .command("fforward", &serde_json::json!([]), 40.0)
            .unwrap();
        assert_eq!(tween.position, 10.0);
    }
}
