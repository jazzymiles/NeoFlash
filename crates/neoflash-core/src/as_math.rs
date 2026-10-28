//! Shared AS2/AS3 numeric Math profile, adapted from Ruffle's AVM1/AVM2 math.rs.
//! Copyright (c) 2018-2022 Ruffle LLC and Ruffle contributors. MIT selected.
//! Pinned sources and deliberate boundaries: docs/RUNTIME-MATH-PROVENANCE.md.

pub const MAX_MATH_ARGUMENTS: usize = 1_000_000;

pub fn constant(name: &str) -> Option<f64> {
    use std::f64::consts;
    Some(match name {
        "E" => consts::E,
        "LN10" => consts::LN_10,
        "LN2" => consts::LN_2,
        "LOG10E" => consts::LOG10_E,
        "LOG2E" => consts::LOG2_E,
        "PI" => consts::PI,
        "SQRT1_2" => consts::FRAC_1_SQRT_2,
        "SQRT2" => consts::SQRT_2,
        _ => return None,
    })
}

/// Host conversion is observable; this function accepts already-coerced values.
/// AS2 uses the SWF7+ profile, including min/max's first-two-argument contract.
pub fn evaluate(as3: bool, operation: &str, args: &[f64], entropy: f64) -> Result<f64, String> {
    if args.len() > MAX_MATH_ARGUMENTS {
        return Err("Math argument count exceeds 1000000".into());
    }
    let x = args.first().copied().unwrap_or(f64::NAN);
    let y = args.get(1).copied().unwrap_or(f64::NAN);
    Ok(match operation {
        "abs" => x.abs(),
        "acos" => x.acos(),
        "asin" => x.asin(),
        "atan" => x.atan(),
        "atan2" => x.atan2(y),
        "ceil" => x.ceil(),
        "cos" => x.cos(),
        "exp" => x.exp(),
        "floor" => x.floor(),
        "log" => x.ln(),
        "sin" => x.sin(),
        "sqrt" => x.sqrt(),
        "tan" => x.tan(),
        // Flash's inspected implementation rounds half values toward +infinity.
        "round" => (x + 0.5).floor(),
        "min" | "max" => {
            let minimum = operation == "min";
            let mut result = if minimum {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            };
            if !as3 && !args.is_empty() {
                if x.is_nan() || y.is_nan() {
                    f64::NAN
                } else if minimum {
                    x.min(y)
                } else {
                    x.max(y)
                }
            } else {
                for &value in args {
                    if value.is_nan() {
                        return Ok(f64::NAN);
                    }
                    if (minimum && value.total_cmp(&result).is_lt())
                        || (!minimum && value.total_cmp(&result).is_gt())
                    {
                        result = value;
                    }
                }
                result
            }
        }
        "pow" if as3 => {
            if y.is_nan() || (y.is_infinite() && (x == 1.0 || x == -1.0)) {
                f64::NAN
            } else if x == f64::NEG_INFINITY
                && y < 0.0
                && y.is_finite()
                && y.trunc() >= i64::MIN as f64
                && (y.trunc() as i64) % 2 == 0
            {
                -0.0
            } else {
                x.powf(y)
            }
        }
        "pow" => x.powf(y),
        "random" | "randomInt" => {
            if !entropy.is_finite() || !(0.0..1.0).contains(&entropy) {
                return Err("Math random entropy must be in [0,1)".into());
            }
            // Ruffle/AVM2 exposes 31-bit fractions. The host supplies entropy;
            // this does not reproduce Flash's seeded RNG or locale sequence.
            let fraction = (entropy * 2_147_483_648.0).floor() / 2_147_483_648.0;
            if operation == "randomInt" {
                let maximum = if x.is_nan() {
                    0.0
                } else {
                    x.trunc().clamp(0.0, 2_147_483_647.0)
                };
                (fraction * maximum).floor()
            } else {
                fraction
            }
        }
        _ => {
            return Err(format!(
                "Unsupported ActionScript Math operation '{operation}'"
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapted_math_family() {
        let value = |as3, name, args: &[f64]| evaluate(as3, name, args, 0.25).unwrap();
        for (name, args, expected) in [
            ("abs", vec![-3.0], 3.0),
            ("sqrt", vec![81.0], 9.0),
            ("round", vec![-1.5], -1.0),
            ("pow", vec![2.0, 8.0], 256.0),
            ("sin", vec![0.0], 0.0),
            ("cos", vec![0.0], 1.0),
        ] {
            assert_eq!(value(true, name, &args), expected);
        }
        assert_eq!(value(true, "max", &[1.0, 2.0, 7.0]), 7.0);
        assert_eq!(value(false, "max", &[1.0, 2.0, 7.0]), 2.0);
        assert!(value(false, "min", &[1.0]).is_nan());
        assert_eq!(value(true, "min", &[]), f64::INFINITY);
        assert!(value(true, "min", &[0.0, -0.0]).is_sign_negative());
        assert!(value(true, "pow", &[1.0, f64::INFINITY]).is_nan());
        assert_eq!(value(false, "randomInt", &[8.0]), 2.0);
        assert_eq!(value(true, "random", &[]), 0.25);
        assert!(evaluate(true, "random", &[], 1.0).is_err());
    }
}
