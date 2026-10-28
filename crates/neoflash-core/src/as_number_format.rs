//! Bounded AS3 NumberFormatter. Rust owns values, decimal rendering, grouping,
//! digit substitution and parsing; the host supplies locale metadata only.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DIGIT_BASES: &[u32] = &[
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90, 0x1b50,
    0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xaa50, 0xff10,
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberFormatter {
    #[serde(rename = "requestedLocaleIDName")]
    requested_locale_id_name: String,
    #[serde(rename = "actualLocaleIDName")]
    actual_locale_id_name: String,
    decimal_separator: String,
    grouping_separator: String,
    grouping_pattern: String,
    digits_type: u32,
    fractional_digits: usize,
    leading_zero: bool,
    trailing_zeros: bool,
    use_grouping: bool,
    negative_number_format: u32,
    negative_symbol: String,
    last_operation_status: String,
}
impl NumberFormatter {
    pub fn new(metadata: &Value) -> Result<Self, String> {
        let text = |key: &str, default: &str| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(default)
                .to_owned()
        };
        let result = Self {
            requested_locale_id_name: text("requestedLocaleIDName", "i-default"),
            actual_locale_id_name: text("actualLocaleIDName", "en-US"),
            decimal_separator: text("decimalSeparator", "."),
            grouping_separator: text("groupingSeparator", ","),
            grouping_pattern: text("groupingPattern", "3;*"),
            digits_type: metadata
                .get("digitsType")
                .and_then(Value::as_u64)
                .unwrap_or(0x30) as u32,
            fractional_digits: 0,
            leading_zero: true,
            trailing_zeros: false,
            use_grouping: false,
            negative_number_format: 1,
            negative_symbol: text("negativeSymbol", "-"),
            last_operation_status: text("lastOperationStatus", "noError"),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        if self.requested_locale_id_name.len() > 256 || self.actual_locale_id_name.len() > 256 {
            return Err("NumberFormatter locale name exceeds 256 bytes".into());
        }
        if self.fractional_digits > 20
            || self.negative_number_format > 4
            || !DIGIT_BASES.contains(&self.digits_type)
        {
            return Err("NumberFormatter requires fractionalDigits 0..20, negativeNumberFormat 0..4 and a supported decimal digit base".into());
        }
        if self.decimal_separator.is_empty()
            || self.decimal_separator.len() > 32
            || self.grouping_separator.len() > 32
            || self.negative_symbol.is_empty()
            || self.negative_symbol.len() > 32
            || self.decimal_separator == self.grouping_separator
        {
            return Err("NumberFormatter separators/sign must be distinct bounded strings (at most 32 bytes)".into());
        }
        grouping(&self.grouping_pattern)?;
        Ok(())
    }
    pub fn set_property(&mut self, name: &str, value: Value) -> Result<(), String> {
        let mut next = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        if !matches!(
            name,
            "decimalSeparator"
                | "groupingSeparator"
                | "groupingPattern"
                | "digitsType"
                | "fractionalDigits"
                | "leadingZero"
                | "trailingZeros"
                | "useGrouping"
                | "negativeNumberFormat"
                | "negativeSymbol"
        ) {
            return Err(format!(
                "NumberFormatter.{name} is read-only or unsupported"
            ));
        }
        next[name] = value;
        let candidate = serde_json::from_value::<Self>(next)
            .map_err(|e| format!("Invalid NumberFormatter property: {e}"));
        match candidate.and_then(|candidate| {
            candidate.validate()?;
            Ok(candidate)
        }) {
            Ok(mut candidate) => {
                candidate.last_operation_status = "noError".into();
                *self = candidate;
                Ok(())
            }
            Err(e) => {
                self.last_operation_status = "illegalArgumentError".into();
                Err(e)
            }
        }
    }
    pub fn format(&mut self, mut value: f64, kind: &str) -> Result<String, String> {
        if kind == "int" || kind == "uint" {
            let n = if value.is_finite() {
                value.trunc().rem_euclid(4294967296.) as u32
            } else {
                0
            };
            value = if kind == "int" {
                (n as i32) as f64
            } else {
                n as f64
            };
        } else if kind != "number" {
            return Err("Unknown NumberFormatter operation".into());
        }
        self.last_operation_status = "noError".into();
        if value.is_nan() {
            return Ok("NaN".into());
        }
        if value.is_infinite() {
            return Ok(if value.is_sign_negative() {
                format!("{}Infinity", self.negative_symbol)
            } else {
                "Infinity".into()
            });
        }
        let negative = value < 0.;
        let decimal = format!("{:.*}", self.fractional_digits, value.abs());
        let (integer, fraction) = decimal.split_once('.').unwrap_or((&decimal, ""));
        let fraction = if self.trailing_zeros {
            fraction
        } else {
            fraction.trim_end_matches('0')
        };
        let mut result = if !self.leading_zero && integer == "0" && !fraction.is_empty() {
            String::new()
        } else if self.use_grouping {
            self.group(integer)?
        } else {
            integer.to_owned()
        };
        if !fraction.is_empty() {
            result.push_str(&self.decimal_separator);
            result.push_str(fraction);
        }
        result = result
            .chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    char::from_u32(self.digits_type + (c as u32 - '0' as u32)).unwrap()
                } else {
                    c
                }
            })
            .collect();
        if negative {
            result = match self.negative_number_format {
                0 => format!("({result})"),
                1 => format!("{}{result}", self.negative_symbol),
                2 => format!("{} {result}", self.negative_symbol),
                3 => format!("{result}{}", self.negative_symbol),
                _ => format!("{result} {}", self.negative_symbol),
            };
        }
        Ok(result)
    }
    fn group(&self, integer: &str) -> Result<String, String> {
        let (sizes, repeat) = grouping(&self.grouping_pattern)?;
        if sizes.is_empty() || self.grouping_separator.is_empty() {
            return Ok(integer.into());
        }
        let mut end = integer.len();
        let mut parts = Vec::new();
        let mut index = 0;
        while end > 0 {
            let size = if index < sizes.len() {
                sizes[index]
            } else if repeat {
                *sizes.last().unwrap()
            } else {
                end
            };
            let start = end.saturating_sub(size);
            parts.push(&integer[start..end]);
            end = start;
            index += 1;
        }
        parts.reverse();
        Ok(parts.join(&self.grouping_separator))
    }
    pub fn parse(&mut self, input: &str, strict: bool) -> Result<Value, String> {
        if input.len() > 1_048_576 {
            return Err("NumberFormatter input exceeds 1 MiB".into());
        }
        let failed = || json!({"value":null,"startIndex":2147483647,"endIndex":2147483647});
        for (start, _) in input.char_indices() {
            if let Some((value, end)) = self.parse_at(input, start) {
                if strict && (!input[..start].trim().is_empty() || !input[end..].trim().is_empty())
                {
                    self.last_operation_status = "parseError".into();
                    return Ok(failed());
                }
                self.last_operation_status = "noError".into();
                return Ok(
                    json!({"value":if value.is_finite(){Some(value)}else{None},"startIndex":input[..start].encode_utf16().count(),"endIndex":input[..end].encode_utf16().count()}),
                );
            }
        }
        self.last_operation_status = "parseError".into();
        Ok(failed())
    }
    fn parse_at(&self, input: &str, start: usize) -> Option<(f64, usize)> {
        let mut at = start;
        let mut negative = false;
        let mut parenthesis = false;
        if self.negative_number_format == 0 && input[at..].starts_with('(') {
            negative = true;
            parenthesis = true;
            at += 1;
            at = skip_one_space(input, at);
        } else if matches!(self.negative_number_format, 1 | 2)
            && input[at..].starts_with(&self.negative_symbol)
        {
            negative = true;
            at += self.negative_symbol.len();
            at = skip_one_space(input, at);
        }
        let mut numeric = String::new();
        let mut digits = 0;
        let mut decimal = false;
        loop {
            if at >= input.len() {
                break;
            }
            let c = input[at..].chars().next()?;
            if let Some(d) = digit(c) {
                numeric.push(char::from(b'0' + d));
                digits += 1;
                at += c.len_utf8();
                continue;
            }
            if !decimal && input[at..].starts_with(&self.decimal_separator) {
                numeric.push('.');
                decimal = true;
                at += self.decimal_separator.len();
                continue;
            }
            if digits > 0
                && !decimal
                && !self.grouping_separator.is_empty()
                && input[at..].starts_with(&self.grouping_separator)
            {
                let next = at + self.grouping_separator.len();
                if input[next..].chars().next().and_then(digit).is_some() {
                    at = next;
                    continue;
                }
            }
            break;
        }
        if digits == 0 {
            return None;
        }
        if parenthesis {
            at = skip_one_space(input, at);
            if !input[at..].starts_with(')') {
                return None;
            }
            at += 1;
        } else if matches!(self.negative_number_format, 3 | 4) {
            let suffix = skip_one_space(input, at);
            if input[suffix..].starts_with(&self.negative_symbol) {
                negative = true;
                at = suffix + self.negative_symbol.len();
            }
        }
        let value = numeric.parse::<f64>().ok()?;
        Some((if negative { -value } else { value }, at))
    }
}
fn digit(c: char) -> Option<u8> {
    let v = c as u32;
    DIGIT_BASES.iter().find_map(|base| {
        if (*base..*base + 10).contains(&v) {
            Some((v - base) as u8)
        } else {
            None
        }
    })
}
fn skip_one_space(input: &str, at: usize) -> usize {
    input[at..]
        .chars()
        .next()
        .filter(|c| c.is_whitespace())
        .map_or(at, |c| at + c.len_utf8())
}
fn grouping(pattern: &str) -> Result<(Vec<usize>, bool), String> {
    if pattern.is_empty() {
        return Ok((vec![], false));
    }
    let mut parts = pattern.split(';').collect::<Vec<_>>();
    let repeat = parts.last() == Some(&"*");
    if repeat {
        parts.pop();
    }
    if parts.is_empty() || parts.len() > 4 {
        return Err("Grouping pattern requires 1..4 sizes, optionally ending ;*".into());
    }
    let mut sizes = Vec::new();
    for part in parts {
        let n = part.parse::<usize>().map_err(|_| "Invalid grouping size")?;
        if !(1..=9).contains(&n) {
            return Err("Grouping sizes must be 1..9".into());
        }
        sizes.push(n);
    }
    Ok((sizes, repeat))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_formatter_family() {
        let mut f = NumberFormatter::new(&json!({})).unwrap();
        f.set_property("fractionalDigits", json!(2)).unwrap();
        f.set_property("useGrouping", json!(true)).unwrap();
        f.set_property("trailingZeros", json!(true)).unwrap();
        assert_eq!(f.format(1234567.5, "number").unwrap(), "1,234,567.50");
        f.set_property("groupingPattern", json!("3;2;*")).unwrap();
        assert_eq!(f.format(1234567.5, "number").unwrap(), "12,34,567.50");
        f.set_property("negativeNumberFormat", json!(0)).unwrap();
        let text = f.format(-1234.5, "number").unwrap();
        assert_eq!(text, "(1,234.50)");
        let p = f.parse(&format!("💡 {text} USD"), false).unwrap();
        assert_eq!(p["value"], -1234.5);
        assert_eq!(p["startIndex"], 3);
        assert_eq!(p["endIndex"], 13);
        assert!(f.parse("+123", true).unwrap()["value"].is_null());
        f.set_property("digitsType", json!(0x660)).unwrap();
        f.set_property("decimalSeparator", json!("٫")).unwrap();
        f.set_property("groupingSeparator", json!("٬")).unwrap();
        let text = f.format(1234.5, "number").unwrap();
        assert_eq!(text, "١٬٢٣٤٫٥٠");
        assert_eq!(f.parse(&text, true).unwrap()["value"], 1234.5);
        f.set_property("digitsType", json!(0x30)).unwrap();
        f.set_property("useGrouping", json!(false)).unwrap();
        f.set_property("fractionalDigits", json!(0)).unwrap();
        assert_eq!(f.format(4294967295., "int").unwrap(), "(1)");
        assert_eq!(f.format(-1., "uint").unwrap(), "4294967295");
        let before = f.fractional_digits;
        assert!(f.set_property("fractionalDigits", json!(1000)).is_err());
        assert_eq!(f.fractional_digits, before);
        assert_eq!(f.last_operation_status, "illegalArgumentError");
    }
}
