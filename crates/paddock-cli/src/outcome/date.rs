//! A calendar date, parsed once (`YYYY-MM-DD`), so an expiry or "today" is never a string that can be empty or impossible.

/// A valid calendar date. Ordered by day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Reads `YYYY-MM-DD`; the error says what was wrong (shape, month 1 to 12, day of that month, leap years).
    pub fn parse(text: &str) -> Result<Date, String> {
        let shape = format!("\"{text}\" is not a date: write YYYY-MM-DD");
        let b = text.as_bytes();
        let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
        if b.len() != 10
            || b[4] != b'-'
            || b[7] != b'-'
            || !digits(0..4)
            || !digits(5..7)
            || !digits(8..10)
        {
            return Err(shape);
        }
        let year: u16 = text[0..4].parse().map_err(|_| shape.clone())?;
        let month: u8 = text[5..7].parse().map_err(|_| shape.clone())?;
        let day: u8 = text[8..10].parse().map_err(|_| shape.clone())?;
        if !(1..=12).contains(&month) {
            return Err(format!(
                "\"{text}\" is not a date: month {month} does not exist"
            ));
        }
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let last = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if day == 0 || day > last {
            return Err(format!(
                "\"{text}\" is not a date: that month has {last} days"
            ));
        }
        Ok(Date { year, month, day })
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_dates_and_orders_them() {
        let a = Date::parse("2026-11-10").unwrap();
        assert_eq!(a.to_string(), "2026-11-10");
        assert!(a < Date::parse("2026-11-11").unwrap() && a < Date::parse("2027-01-01").unwrap());
        assert_eq!(a, Date::parse("2026-11-10").unwrap());
        assert!(Date::parse("2028-02-29").is_ok() && Date::parse("2000-02-29").is_ok());
    }

    #[test]
    fn refuses_everything_else() {
        for bad in [
            "",
            " 2026-11-10",
            "2026-11-10 ",
            "2026-1-10",
            "26-11-10",
            "2026/11/10",
            "2027-13-99",
            "2027-00-10",
            "2027-04-31",
            "2027-02-29",
            "1900-02-29",
            "2027-01-00",
            "abcd-ef-gh",
            "２０２６-11-10",
        ] {
            let e = Date::parse(bad).unwrap_err();
            assert!(e.contains("not a date"), "{bad:?}: {e}");
        }
    }
}
