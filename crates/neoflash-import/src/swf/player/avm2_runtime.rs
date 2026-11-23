//! AVM2 runtime support submodules, multiname keys, and value/number conversion helpers.

use super::*;

mod calls;
mod construct;
mod events;
mod properties;
mod timeline;
mod types;

#[derive(Clone, Debug)]
pub(super) enum Avm2MultinameKey {
    Static { name: String, is_attr: bool },
    Runtime { key: Avm1Value, is_attr: bool },
}

impl Avm2MultinameKey {
    pub(super) fn to_prop_string(&self, objects: &[Avm1Object]) -> String {
        match self {
            Self::Static { name, is_attr } => {
                if *is_attr && !name.starts_with('@') {
                    format!("@{name}")
                } else {
                    name.clone()
                }
            }
            Self::Runtime { key, is_attr } => {
                let mut s = if let Avm1Value::Object(qoid) = key
                    && let Some(qo) = objects.get(*qoid)
                    && let Some(Avm1Value::String(ln)) = qo.props.get("localName")
                {
                    ln.clone()
                } else {
                    avm2_convert_string(key, objects)
                };
                if *is_attr && !s.starts_with('@') {
                    s = format!("@{s}");
                }
                s
            }
        }
    }
}

pub(super) fn is_avm2_fn_token(s: &str) -> bool {
    s.starts_with("__avm2_fn:")
        || s.starts_with("__avm2_bound:")
        || s.starts_with("__avm2_bound_vec:")
        || s.starts_with("__avm2_closure:")
}

pub(super) fn parse_avm2_fn_token(s: &str) -> Option<(usize, usize)> {
    if let Some((_, a, m)) = parse_avm2_closure_token(s) {
        return Some((a, m));
    }
    if let Some((_, a, m)) = parse_avm2_bound_token(s) {
        return Some((a, m));
    }
    let rest = s.strip_prefix("__avm2_fn:")?;
    let (a, m) = rest.split_once(':')?;
    Some((a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_closure_token(s: &str) -> Option<(usize, usize, usize)> {
    let rest = s.strip_prefix("__avm2_closure:")?;
    let (cid, rem) = rest.split_once(':')?;
    let (a, m) = rem.split_once(':')?;
    Some((cid.parse().ok()?, a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_bound_token(s: &str) -> Option<(usize, usize, usize)> {
    let rest = s.strip_prefix("__avm2_bound:")?;
    let (oid, rem) = rest.split_once(':')?;
    let (a, m) = rem.split_once(':')?;
    Some((oid.parse().ok()?, a.parse().ok()?, m.parse().ok()?))
}

pub(super) fn parse_avm2_class_token(s: &str) -> Option<(usize, usize)> {
    let rest = s.strip_prefix("__avm2_class:")?;
    let (a, c) = rest.split_once(':')?;
    Some((a.parse().ok()?, c.parse().ok()?))
}

pub(super) fn abc_const_to_avm_value(abc: &AbcFile, vindex: usize, vkind: u8) -> Avm1Value {
    match vkind {
        0x03 => Avm1Value::Number(f64::from(abc.ints.get(vindex).copied().unwrap_or(0))),
        0x04 => Avm1Value::Number(f64::from(abc.uints.get(vindex).copied().unwrap_or(0))),
        0x06 => Avm1Value::Number(abc.doubles.get(vindex).copied().unwrap_or(f64::NAN)),
        0x01 => Avm1Value::String(abc.string(vindex).to_owned()),
        0x0B => Avm1Value::Bool(true),
        0x0A => Avm1Value::Bool(false),
        0x0C => Avm1Value::Null,
        0x00 => Avm1Value::Undefined,
        _ => Avm1Value::Null,
    }
}

pub(super) fn parse_avm2_float(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return f64::NAN;
    }
    let bytes = t.as_bytes();
    let mut i = 0usize;
    let neg = if bytes[0] == b'-' {
        i = 1;
        true
    } else if bytes[0] == b'+' {
        i = 1;
        false
    } else {
        false
    };
    if t[i..].starts_with("Infinity") {
        return if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let start_digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let has_int_digits = i > start_digits;
    let mut has_frac_digits = false;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        has_frac_digits = i > frac_start;
    }
    if !has_int_digits && !has_frac_digits {
        return f64::NAN;
    }
    let pre_exp = i;
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        } else {
            i = pre_exp;
        }
    }
    t[..i].parse::<f64>().unwrap_or(f64::NAN)
}

pub(super) fn avm2_string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return parse_hex_digits_to_f64(hex);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return -parse_hex_digits_to_f64(hex);
    }
    if let Some(hex) = t.strip_prefix("+0x").or_else(|| t.strip_prefix("+0X")) {
        return parse_hex_digits_to_f64(hex);
    }
    parse_ecma_decimal_strict(t)
}

pub(super) fn avm2_to_number(val: &Avm1Value) -> f64 {
    match val {
        Avm1Value::Undefined => f64::NAN,
        Avm1Value::Null => 0.0,
        Avm1Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Avm1Value::Number(n) => *n,
        Avm1Value::String(s) => {
            if is_avm2_fn_token(s) {
                f64::NAN
            } else {
                avm2_string_to_number(s)
            }
        }
        Avm1Value::Object(_)
        | Avm1Value::MovieClip(_)
        | Avm1Value::MovieClipIdentity(_)
        | Avm1Value::Super { .. } => f64::NAN,
    }
}

pub(super) fn avm2_convert_string(val: &Avm1Value, objects: &[Avm1Object]) -> String {
    match val {
        Avm1Value::Undefined => "undefined".to_owned(),
        Avm1Value::Null => "null".to_owned(),
        Avm1Value::String(s) if is_avm2_fn_token(s) => "function Function() {}".to_owned(),
        other => other.to_trace_string(9, objects),
    }
}

pub(super) fn avm2_less_than(a: &Avm1Value, b: &Avm1Value) -> Option<bool> {
    if let (Avm1Value::String(sa), Avm1Value::String(sb)) = (a, b)
        && !is_avm2_fn_token(sa)
        && !is_avm2_fn_token(sb)
    {
        return Some(sa.encode_utf16().lt(sb.encode_utf16()));
    }
    let na = avm2_to_number(a);
    let nb = avm2_to_number(b);
    if na.is_nan() || nb.is_nan() {
        None
    } else {
        Some(na < nb)
    }
}
