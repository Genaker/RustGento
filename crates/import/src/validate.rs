use chrono::{NaiveDate, NaiveDateTime};

/// Parses an EAV `int`-backend attribute value. Matches Go's `strconv.Atoi`
/// (base-10, no leading `+`, optional leading `-`) closely enough for CSV
/// import purposes.
pub fn parse_int_value(s: &str) -> Result<i32, String> {
    s.parse::<i32>().map_err(|_| format!("invalid int value {s:?}"))
}

/// Parses an EAV `decimal`-backend attribute value.
pub fn parse_decimal_value(s: &str) -> Result<f64, String> {
    s.parse::<f64>().map_err(|_| format!("invalid decimal value {s:?}"))
}

/// Parses an EAV `datetime`-backend attribute value, accepting either a full
/// `YYYY-MM-DD HH:MM:SS` timestamp or a bare `YYYY-MM-DD` date (midnight
/// implied) -- the two formats Magento exports commonly use.
pub fn parse_datetime_value(s: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap()))
        .map_err(|_| format!("invalid datetime value {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_int() {
        assert_eq!(parse_int_value("42"), Ok(42));
        assert_eq!(parse_int_value("-7"), Ok(-7));
    }

    #[test]
    fn rejects_invalid_int() {
        assert!(parse_int_value("abc").is_err());
        assert!(parse_int_value("4.2").is_err());
        assert!(parse_int_value("").is_err());
    }

    #[test]
    fn parses_valid_decimal() {
        assert_eq!(parse_decimal_value("9.99"), Ok(9.99));
        assert_eq!(parse_decimal_value("42"), Ok(42.0));
        assert_eq!(parse_decimal_value("-1.5"), Ok(-1.5));
    }

    #[test]
    fn rejects_invalid_decimal() {
        assert!(parse_decimal_value("not-a-number").is_err());
        assert!(parse_decimal_value("").is_err());
    }

    #[test]
    fn parses_full_datetime() {
        let dt = parse_datetime_value("2026-03-05 10:30:00").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-03-05 10:30:00");
    }

    #[test]
    fn parses_bare_date_as_midnight() {
        let dt = parse_datetime_value("2026-03-05").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-03-05 00:00:00");
    }

    #[test]
    fn rejects_invalid_datetime() {
        assert!(parse_datetime_value("not-a-date").is_err());
        assert!(parse_datetime_value("2026-13-99").is_err());
        assert!(parse_datetime_value("").is_err());
    }
}
