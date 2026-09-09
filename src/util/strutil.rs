//! Small line-parsing helpers shared by the command dispatcher and the
//! persistence loader/saver, which both parse simple whitespace-delimited
//! text lines.

/// Trims leading spaces/tabs and trailing spaces/tabs/\r/\n from `s`.
pub fn trim(s: &str) -> &str {
    let s = s.trim_start_matches([' ', '\t']);
    s.trim_end_matches([' ', '\t', '\r', '\n'])
}

/// Extracts the next single-space-delimited token from `*rest`, advancing
/// `*rest` past it plus any following run of spaces. Returns `None`
/// (leaving `*rest` untouched) if `*rest` is already empty.
pub fn next_token<'a>(rest: &mut &'a str) -> Option<&'a str> {
    if rest.is_empty() {
        return None;
    }
    match rest.find(' ') {
        Some(pos) => {
            let token = &rest[..pos];
            let after = &rest[pos + 1..];
            *rest = after.trim_start_matches(' ');
            Some(token)
        }
        None => {
            let token = *rest;
            *rest = "";
            Some(token)
        }
    }
}

pub fn parse_int(s: &str) -> Option<i32> {
    parse_long(s).map(|v| v as i32)
}

pub fn parse_long(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    s.parse::<i64>().ok()
}

pub fn parse_double(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Formats a float the way C's `printf("%.*g", precision, value)` would:
/// fixed or scientific notation chosen by magnitude, `precision`
/// significant digits, trailing zeros (and a trailing bare decimal point)
/// stripped.
pub fn format_g(value: f64, precision: usize) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }

    let prec = precision.max(1);
    let sci = format!("{:.*e}", prec - 1, value);
    let epos = sci.find('e').expect("scientific format always has 'e'");
    let exp: i32 = sci[epos + 1..].parse().expect("valid exponent digits");
    let mantissa = &sci[..epos];

    if exp >= -4 && exp < prec as i32 {
        let frac_digits = (prec as i32 - 1 - exp).max(0) as usize;
        let mut fixed = format!("{:.*}", frac_digits, value);
        strip_trailing_zeros(&mut fixed);
        fixed
    } else {
        let mut mantissa_owned = mantissa.to_string();
        strip_trailing_zeros(&mut mantissa_owned);
        let sign = if exp < 0 { "-" } else { "+" };
        format!("{}e{}{:02}", mantissa_owned, sign, exp.abs())
    }
}

fn strip_trailing_zeros(s: &mut String) {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_token_splits_on_single_space_and_collapses_runs() {
        let mut rest = "a   b c";
        assert_eq!(next_token(&mut rest), Some("a"));
        assert_eq!(rest, "b c");
        assert_eq!(next_token(&mut rest), Some("b"));
        assert_eq!(rest, "c");
        assert_eq!(next_token(&mut rest), Some("c"));
        assert_eq!(rest, "");
        assert_eq!(next_token(&mut rest), None);
    }

    #[test]
    fn trim_strips_spaces_tabs_and_crlf() {
        assert_eq!(trim("  \thello world\t \r\n"), "hello world");
    }

    #[test]
    fn format_g_matches_printf_g_for_common_values() {
        assert_eq!(format_g(100.0, 6), "100");
        assert_eq!(format_g(50.0, 6), "50");
        assert_eq!(format_g(5.0, 6), "5");
        assert_eq!(format_g(0.0, 6), "0");
        assert_eq!(format_g(2.75, 6), "2.75");
        assert_eq!(format_g(0.1, 6), "0.1");
        assert_eq!(format_g(-5.0, 6), "-5");
    }

    #[test]
    fn format_g_round_trips_at_high_precision() {
        let value = 1.0 / 3.0;
        let formatted = format_g(value, 17);
        let parsed: f64 = formatted.parse().unwrap();
        assert_eq!(parsed, value);
    }
}
