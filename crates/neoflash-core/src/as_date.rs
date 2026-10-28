//! Bounded shared ActionScript Date calendar arithmetic.
//!
//! Civil day conversion adapts Howard Hinnant's MIT date v3.0.4 algorithms;
//! optional-component adjustment follows the inspected MIT Ruffle Date design.
//! Browser timezone databases and locale/parser services remain host facilities.
//! See docs/RUNTIME-DATE-PROVENANCE.md for pins and deliberate partial limits.

const DAY: f64 = 86_400_000.0;
pub const MAX_TIME: f64 = 8_640_000_000_000_000.0;
const COMPONENT_LIMIT: f64 = 9_007_199_254_740_991.0;

pub fn clip(value: f64) -> f64 {
    if value.is_finite() && value.abs() <= MAX_TIME {
        value.trunc() + 0.0
    } else {
        f64::NAN
    }
}

fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
    let year = year - i128::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let march_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * march_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = march_month + if march_month < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

/// Normalized wall-clock fields: year, zero-based month, day, hour, minute,
/// second, millisecond, weekday. The host may use them solely to resolve a
/// normalized local civil time against its timezone database.
pub fn parts(time: f64) -> [f64; 8] {
    // Offsetting an endpoint into local time may exceed TimeClip by one day.
    if !time.is_finite() || time.abs() > MAX_TIME + 2.0 * DAY {
        return [f64::NAN; 8];
    }
    let millis = time.trunc() as i64;
    let days = millis.div_euclid(DAY as i64);
    let within = millis.rem_euclid(DAY as i64);
    let (year, month, day) = civil_from_days(days);
    [
        year as f64,
        (month - 1) as f64,
        day as f64,
        (within / 3_600_000) as f64,
        (within / 60_000 % 60) as f64,
        (within / 1000 % 60) as f64,
        (within % 1000) as f64,
        (days + 4).rem_euclid(7) as f64,
    ]
}

fn compose(fields: &[f64; 7]) -> f64 {
    if fields
        .iter()
        .any(|v| !v.is_finite() || v.abs() > COMPONENT_LIMIT)
    {
        return f64::NAN;
    }
    let [year, month, day, hour, minute, second, millisecond] = fields.map(|v| v.trunc() as i128);
    let year = year + month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    let days = days_from_civil(year, month, 1) + day - 1;
    let millis =
        days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + millisecond;
    millis as f64
}

pub fn construct(args: &[f64]) -> f64 {
    if args.len() < 2 {
        return f64::NAN;
    }
    let mut fields = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (field, value) in fields.iter_mut().zip(args) {
        *field = *value;
    }
    if (0.0..=99.0).contains(&fields[0].trunc()) {
        fields[0] = fields[0].trunc() + 1900.0;
    }
    compose(&fields)
}

/// Getter calculation after the host supplies its offset in minutes west of UTC.
pub fn get(operation: &str, time: f64, timezone_offset: f64) -> Result<f64, String> {
    if matches!(operation, "getTime" | "valueOf") {
        return Ok(clip(time));
    }
    if operation == "getTimezoneOffset" {
        return Ok(if time.is_finite() {
            timezone_offset
        } else {
            f64::NAN
        });
    }
    let utc = operation.contains("UTC");
    let name = operation
        .strip_prefix("getUTC")
        .or_else(|| operation.strip_prefix("get"))
        .ok_or_else(|| format!("Unsupported Date getter: {operation}"))?;
    let fields = parts(time - if utc { 0.0 } else { timezone_offset * 60_000.0 });
    Ok(match name {
        "FullYear" => fields[0],
        "Year" => fields[0] - 1900.0,
        "Month" => fields[1],
        "Date" => fields[2],
        "Hours" => fields[3],
        "Minutes" => fields[4],
        "Seconds" => fields[5],
        "Milliseconds" => fields[6],
        "Day" => fields[7],
        _ => return Err(format!("Unsupported Date getter: {operation}")),
    })
}

/// A UTC setter returns a clipped timestamp; a local setter returns normalized
/// wall-clock milliseconds for the host's timezone resolution, then clipping.
pub fn set(operation: &str, time: f64, args: &[f64], timezone_offset: f64) -> Result<f64, String> {
    if operation == "setTime" {
        return Ok(clip(args.first().copied().unwrap_or(f64::NAN)));
    }
    let utc = operation.contains("UTC");
    let name = operation
        .strip_prefix("setUTC")
        .or_else(|| operation.strip_prefix("set"))
        .ok_or_else(|| format!("Unsupported Date setter: {operation}"))?;
    let (start, count) = match name {
        "FullYear" => (0, 3),
        "Year" => (0, 1),
        "Month" => (1, 2),
        "Date" => (2, 1),
        "Hours" => (3, 4),
        "Minutes" => (4, 3),
        "Seconds" => (5, 2),
        "Milliseconds" => (6, 1),
        _ => return Err(format!("Unsupported Date setter: {operation}")),
    };
    let wall = if !time.is_finite() && matches!(name, "FullYear" | "Year") {
        0.0
    } else {
        time - if utc { 0.0 } else { timezone_offset * 60_000.0 }
    };
    let mut fields: [f64; 7] = parts(wall)[..7].try_into().unwrap();
    if args.is_empty() {
        fields[start] = f64::NAN;
    } else {
        for (field, arg) in fields[start..start + count].iter_mut().zip(args) {
            *field = *arg;
        }
    }
    if name == "Year" && (0.0..=99.0).contains(&fields[0].trunc()) {
        fields[0] = fields[0].trunc() + 1900.0;
    }
    let result = compose(&fields);
    Ok(if utc { clip(result) } else { result })
}

pub fn utc_string(time: f64) -> String {
    let p = parts(clip(time));
    if !p[0].is_finite() {
        return "Invalid Date".into();
    }
    const WEEK: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTH: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let year = p[0] as i64;
    let year = if year < 0 {
        format!("-{:04}", -year)
    } else {
        format!("{year:04}")
    };
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        WEEK[p[7] as usize],
        p[2] as i64,
        MONTH[p[1] as usize],
        year,
        p[3] as i64,
        p[4] as i64,
        p[5] as i64
    )
}

#[derive(Clone, serde::Deserialize)]
pub struct LocalePart {
    #[serde(rename = "type")]
    kind: String,
    value: String,
}

/// Locale names and style templates are supplied by the host's OS/Intl service.
/// Neither pattern interpretation nor final date construction lives in JS.
#[derive(Clone, serde::Deserialize)]
pub struct LocaleProfile {
    requested: String,
    actual: String,
    fallback: bool,
    first_weekday: u8,
    hour_cycle: String,
    patterns: std::collections::BTreeMap<String, Vec<LocalePart>>,
    month_standalone: Vec<Vec<String>>,
    month_format: Vec<Vec<String>>,
    weekdays: Vec<Vec<String>>,
    day_periods: Vec<String>,
}

#[derive(Clone)]
enum PatternPiece {
    Literal(String),
    Field(char, usize),
}

pub struct DateFormatter {
    profile: LocaleProfile,
    date_style: String,
    time_style: String,
    pattern: String,
    pieces: Vec<PatternPiece>,
    status: &'static str,
}

fn compile_pattern(pattern: &str) -> Result<Vec<PatternPiece>, String> {
    if pattern.chars().count() > 4096 {
        return Err("DateTimeFormatter pattern exceeds 4096 characters".into());
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    let mut quoted = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\'' {
            if chars.get(index + 1) == Some(&'\'') {
                literal.push('\'');
                index += 2;
                continue;
            }
            quoted = !quoted;
            index += 1;
            continue;
        }
        if quoted || !ch.is_ascii_alphabetic() {
            literal.push(ch);
            index += 1;
            continue;
        }
        if !literal.is_empty() {
            result.push(PatternPiece::Literal(std::mem::take(&mut literal)));
        }
        let start = index;
        while chars.get(index) == Some(&ch) {
            index += 1;
        }
        let count = index - start;
        let max = match ch {
            'y' => 8,
            'M' | 'L' | 'E' | 'z' => 4,
            'd' | 'H' | 'h' | 'K' | 'k' | 'm' | 's' => 2,
            'S' => 3,
            'a' | 'Z' => 1,
            _ => {
                return Err(format!(
                    "Unsupported DateTimeFormatter pattern field '{ch}'; supported: y M L d E H h K k m s S a z Z and quoted literals"
                ));
            }
        };
        if count > max {
            return Err(format!(
                "Unsupported DateTimeFormatter field width: {ch} repeated {count}"
            ));
        }
        result.push(PatternPiece::Field(ch, count));
    }
    if quoted {
        return Err("DateTimeFormatter pattern has an unterminated quoted literal".into());
    }
    if !literal.is_empty() {
        result.push(PatternPiece::Literal(literal));
    }
    Ok(result)
}

fn quoted_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

impl DateFormatter {
    pub fn new(profile_json: &str, date_style: &str, time_style: &str) -> Result<Self, String> {
        if profile_json.len() > 131_072 {
            return Err("DateTimeFormatter locale profile exceeds 128 KiB".into());
        }
        let profile: LocaleProfile = serde_json::from_str(profile_json)
            .map_err(|e| format!("Invalid DateTimeFormatter locale profile: {e}"))?;
        let valid_names = |names: &[Vec<String>], count| {
            names.len() == 3
                && names
                    .iter()
                    .all(|row| row.len() == count && row.iter().all(|name| name.len() <= 512))
        };
        if profile.first_weekday > 6
            || !valid_names(&profile.month_standalone, 12)
            || !valid_names(&profile.month_format, 12)
            || !valid_names(&profile.weekdays, 7)
            || profile.day_periods.len() != 2
            || profile.day_periods.iter().any(|v| v.len() > 512)
        {
            return Err("Invalid DateTimeFormatter locale names or first weekday".into());
        }
        let mut formatter = Self {
            profile,
            date_style: String::new(),
            time_style: String::new(),
            pattern: String::new(),
            pieces: vec![],
            status: "noError",
        };
        formatter.set_styles(date_style, time_style)?;
        if formatter.profile.fallback {
            formatter.status = "usingFallbackWarning";
        }
        Ok(formatter)
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({"requestedLocaleIDName":self.profile.requested,"actualLocaleIDName":self.profile.actual,"lastOperationStatus":self.status,"dateStyle":self.date_style,"timeStyle":self.time_style,"pattern":self.pattern})
    }

    pub fn set_styles(&mut self, date_style: &str, time_style: &str) -> Result<(), String> {
        self.status = "illegalArgumentError";
        if ![date_style, time_style]
            .iter()
            .all(|style| matches!(*style, "none" | "short" | "medium" | "long"))
        {
            return Err("DateTimeFormatter styles must be none, short, medium or long; custom is only set by setDateTimePattern".into());
        }
        let parts = self
            .profile
            .patterns
            .get(&format!("{date_style}/{time_style}"))
            .ok_or("DateTimeFormatter locale style template is unavailable")?;
        let mut pattern = String::new();
        for part in parts {
            let token = match part.kind.as_str() {
                "literal" => quoted_literal(&part.value),
                "year" => if part.value.len() == 2 { "yy" } else { "yyyy" }.into(),
                "month" => if part.value.chars().all(|c| c.is_ascii_digit()) {
                    if part.value.len() == 2 { "MM" } else { "M" }
                } else if date_style == "long" {
                    "MMMM"
                } else {
                    "MMM"
                }
                .into(),
                "day" => if part.value.len() == 2 { "dd" } else { "d" }.into(),
                "weekday" => "EEEE".into(),
                "hour" => {
                    let field = match self.profile.hour_cycle.as_str() {
                        "h11" => 'K',
                        "h12" => 'h',
                        "h24" => 'k',
                        _ => 'H',
                    };
                    std::iter::repeat_n(field, if part.value.len() == 2 { 2 } else { 1 }).collect()
                }
                "minute" => "mm".into(),
                "second" => "ss".into(),
                "dayPeriod" => "a".into(),
                "timeZoneName" => "z".into(),
                _ => {
                    return Err(format!(
                        "Unsupported host calendar style field: {}",
                        part.kind
                    ));
                }
            };
            pattern.push_str(&token);
        }
        let pieces = compile_pattern(&pattern)?;
        self.date_style = date_style.into();
        self.time_style = time_style.into();
        self.pattern = pattern;
        self.pieces = pieces;
        self.status = "noError";
        Ok(())
    }

    pub fn set_pattern(&mut self, pattern: &str) -> Result<(), String> {
        self.status = "illegalArgumentError";
        let pieces = compile_pattern(pattern)?;
        self.pattern = pattern.into();
        self.pieces = pieces;
        self.date_style = "custom".into();
        self.time_style = "custom".into();
        self.status = "noError";
        Ok(())
    }

    pub fn names(
        &mut self,
        months: bool,
        style: &str,
        context: &str,
    ) -> Result<Vec<String>, String> {
        self.status = "illegalArgumentError";
        let index = match style {
            "full" => 0,
            "longAbbreviation" => 1,
            "shortAbbreviation" => 2,
            _ => return Err(
                "DateTimeFormatter name style must be full, longAbbreviation or shortAbbreviation"
                    .into(),
            ),
        };
        if !matches!(context, "format" | "standalone") {
            return Err("DateTimeFormatter name context must be format or standalone".into());
        }
        self.status = "noError";
        Ok(if months {
            if context == "format" {
                &self.profile.month_format[index]
            } else {
                &self.profile.month_standalone[index]
            }
        } else {
            &self.profile.weekdays[index]
        }
        .clone())
    }

    pub fn first_weekday(&mut self) -> u8 {
        self.status = "noError";
        self.profile.first_weekday
    }
    pub fn note_read(&mut self) {
        self.status = "noError";
    }

    pub fn format(
        &mut self,
        time: f64,
        offset: f64,
        zone_short: &str,
        zone_long: &str,
    ) -> Result<String, String> {
        self.status = "illegalArgumentError";
        if !time.is_finite()
            || time.abs() > MAX_TIME
            || !offset.is_finite()
            || offset.abs() > 1440.0
        {
            return Err("DateTimeFormatter requires a valid Date and timezone offset".into());
        }
        if zone_short.len() > 512 || zone_long.len() > 512 {
            return Err("DateTimeFormatter timezone name exceeds 512 bytes".into());
        }
        let fields = parts(time - offset * 60_000.0);
        let mut result = String::new();
        for piece in &self.pieces {
            match piece {
                PatternPiece::Literal(text) => result.push_str(text),
                PatternPiece::Field(ch, width) => {
                    let width = *width;
                    let number = match *ch {
                        'y' => Some(if width == 2 {
                            (fields[0] as i64).rem_euclid(100)
                        } else {
                            fields[0] as i64
                        }),
                        'M' | 'L' if width <= 2 => Some(fields[1] as i64 + 1),
                        'd' => Some(fields[2] as i64),
                        'H' => Some(fields[3] as i64),
                        'h' => Some((fields[3] as i64 + 11) % 12 + 1),
                        'K' => Some(fields[3] as i64 % 12),
                        'k' => Some((fields[3] as i64 + 23) % 24 + 1),
                        'm' => Some(fields[4] as i64),
                        's' => Some(fields[5] as i64),
                        'S' => Some(fields[6] as i64 / 10_i64.pow((3 - width) as u32)),
                        _ => None,
                    };
                    if let Some(number) = number {
                        result.push_str(&format!("{number:0width$}"));
                        continue;
                    }
                    match *ch {
                        'M' | 'L' => {
                            let rows = if *ch == 'L' {
                                &self.profile.month_standalone
                            } else {
                                &self.profile.month_format
                            };
                            result.push_str(&rows[usize::from(width == 3)][fields[1] as usize]);
                        }
                        'E' => result.push_str(
                            &self.profile.weekdays[usize::from(width <= 3)][fields[7] as usize],
                        ),
                        'a' => result
                            .push_str(&self.profile.day_periods[usize::from(fields[3] >= 12.0)]),
                        'z' => result.push_str(if width == 4 { zone_long } else { zone_short }),
                        'Z' => {
                            let minutes = -(offset as i64);
                            result.push_str(&format!(
                                "{}{:02}{:02}",
                                if minutes < 0 { '-' } else { '+' },
                                minutes.abs() / 60,
                                minutes.abs() % 60
                            ));
                        }
                        _ => unreachable!(),
                    }
                }
            }
            if result.len() > 65536 {
                return Err("DateTimeFormatter output exceeds 65536 bytes".into());
            }
        }
        self.status = "noError";
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_calendar_family() {
        let leap = construct(&[2000.0, 1.0, 29.0, 23.0, 59.0, 59.0, 999.0]);
        assert_eq!(
            parts(leap),
            [2000.0, 1.0, 29.0, 23.0, 59.0, 59.0, 999.0, 2.0]
        );
        assert_eq!(get("getUTCFullYear", leap, -120.0).unwrap(), 2000.0);
        assert_eq!(get("getDate", leap, -120.0).unwrap(), 1.0);
        assert_eq!(get("getHours", leap, -120.0).unwrap(), 1.0);
        let next = set("setUTCSeconds", leap, &[60.0, 0.0], 0.0).unwrap();
        assert_eq!(parts(next), [2000.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 3.0]);
        assert_eq!(
            set("setUTCFullYear", f64::NAN, &[2024.0], 0.0).unwrap(),
            construct(&[2024.0, 0.0])
        );
        assert!(set("setUTCMonth", f64::NAN, &[0.0], 0.0).unwrap().is_nan());
        assert!(set("setTime", 0.0, &[], 0.0).unwrap().is_nan());
        assert_eq!(utc_string(0.0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(
            parts(-1.0),
            [1969.0, 11.0, 31.0, 23.0, 59.0, 59.0, 999.0, 3.0]
        );
        assert_eq!(construct(&[99.0, 0.0]), construct(&[1999.0, 0.0]));
        assert_eq!(
            construct(&[2000.0, -1.0, 0.0]),
            construct(&[1999.0, 10.0, 30.0])
        );
        assert_eq!(parts(construct(&[-1.0, 0.0]))[0], -1.0);
        assert_eq!(parts(MAX_TIME)[..3], [275760.0, 8.0, 13.0]);
        assert!(clip(MAX_TIME + 1.0).is_nan());
        assert_eq!(clip(-0.5).to_bits(), 0.0_f64.to_bits());
        for &(year, month, day) in &[
            (1900.0, 1.0, 29.0),
            (2000.0, 1.0, 29.0),
            (2024.0, 11.0, 31.0),
            (-100.0, 5.0, 1.0),
        ] {
            let time = construct(&[year, month, day]);
            let p = parts(time);
            assert_eq!(compose(&p[..7].try_into().unwrap()), time);
        }
        let months: Vec<String> = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ]
        .map(str::to_owned)
        .into();
        let weekdays: Vec<String> = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ]
        .map(str::to_owned)
        .into();
        let profile = serde_json::json!({"requested":"en-US","actual":"en-US","fallback":false,"first_weekday":0,"hour_cycle":"h12",
            "patterns":{"long/none":[{"type":"month","value":"November"},{"type":"literal","value":" "},{"type":"day","value":"22"},{"type":"literal","value":", "},{"type":"year","value":"2006"}],"none/none":[]},
            "month_standalone":[months,months,months],"month_format":[months,months,months],"weekdays":[weekdays,weekdays,weekdays],"day_periods":["AM","PM"]});
        let mut formatter = DateFormatter::new(&profile.to_string(), "long", "none").unwrap();
        assert_eq!(
            formatter.format(leap, 0.0, "UTC", "UTC").unwrap(),
            "February 29, 2000"
        );
        formatter
            .set_pattern("yyyy-MM-dd'T'HH:mm:ss.SSS Z")
            .unwrap();
        assert_eq!(
            formatter.format(leap, 0.0, "UTC", "UTC").unwrap(),
            "2000-02-29T23:59:59.999 +0000"
        );
        assert_eq!(
            formatter
                .format(leap, -120.0, "EET", "Eastern European Time")
                .unwrap(),
            "2000-03-01T01:59:59.999 +0200"
        );
        assert_eq!(
            formatter.names(true, "full", "standalone").unwrap()[1],
            "February"
        );
        assert_eq!(
            formatter.names(false, "full", "format").unwrap()[2],
            "Tuesday"
        );
        assert_eq!(formatter.first_weekday(), 0);
        assert!(formatter.set_pattern("yyyy Q").is_err());
        assert_eq!(
            formatter.snapshot()["lastOperationStatus"],
            "illegalArgumentError"
        );
        assert_eq!(
            formatter.snapshot()["pattern"],
            "yyyy-MM-dd'T'HH:mm:ss.SSS Z"
        );
        formatter.set_pattern("'o''clock' h:mm a").unwrap();
        assert_eq!(
            formatter.format(leap, 0.0, "UTC", "UTC").unwrap(),
            "o'clock 11:59 PM"
        );
        formatter.set_styles("none", "none").unwrap();
        assert_eq!(formatter.format(0.0, 0.0, "UTC", "UTC").unwrap(), "");
    }
}
