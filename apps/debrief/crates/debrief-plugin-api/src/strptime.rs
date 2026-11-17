//! A strftime-format reader with the exact reading rules Debrief's timestamp formats have always had.
//!
//! The formats users write (`%d/%b/%Y:%H:%M:%S %z`, `%b %e %H:%M:%S`, `%Y%m%d-%H%M%S`, ...) were read by chrono's `parse`;
//! this module keeps those rules (whitespace in a format matches any run of whitespace, numbers skip leading
//! whitespace and take at most their width, `%b` reads three letters and `%B` a short or long name, `%.f` is optional,
//! `%3f` takes exactly three digits, a weekday must agree with the date, `%p` is case blind, ...) on top of `jiff`'s
//! calendar. It also holds the strict RFC 3339 and the RFC 2822 readers the parser used. Differences from chrono are
//! limited to years outside -9999..=9999, which `jiff` cannot represent: they read as "not a time".
//!
//! Pure: no clock, no I/O, never panics on any input.

use jiff::civil::{Date, ISOWeekDate, Weekday};
use jiff::tz::{AmbiguousOffset, TimeZone};

/// Why a read failed (the kinds matter in a few places where a missing field is filled from a timestamp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bad {
    OutOfRange,
    Impossible,
    NotEnough,
    Invalid,
    TooShort,
    TooLong,
    UnknownSpecifier,
}

type R<T> = Result<T, Bad>;

/// A time of day; a leap second is second 59 with 1_000_000_000 added to `nano`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// 0..=23.
    pub hour: u32,
    /// 0..=59.
    pub minute: u32,
    /// 0..=59 (a leap second is 59 with 1e9 more in `nano`).
    pub second: u32,
    /// 0..=1_999_999_999.
    pub nano: u32,
}

impl Clock {
    /// A valid clock: `second` 0..=59, `nano` below 2e9 (a leap second carries 1e9 in it).
    pub fn new(hour: u32, minute: u32, second: u32, nano: u32) -> Option<Clock> {
        (hour < 24 && minute < 60 && second < 60 && nano < 2_000_000_000).then_some(Clock {
            hour,
            minute,
            second,
            nano,
        })
    }

    fn seconds(self) -> i64 {
        i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second)
    }
}

/// Epoch seconds of 00:00:00 UTC of `date`.
fn midnight_utc_seconds(date: Date) -> Option<i64> {
    // Counted in days, not through a `Timestamp`, whose range stops a day short of the last date.
    let days = date.since(Date::constant(1970, 1, 1)).ok()?.get_days();
    Some(i64::from(days) * 86_400)
}

/// Milliseconds since the Unix epoch of `date` + `clock` at `offset_seconds` east of UTC (a leap second adds its 1000 ms).
pub fn epoch_ms(date: Date, clock: Clock, offset_seconds: i32) -> Option<i64> {
    let secs = midnight_utc_seconds(date)?
        .checked_add(clock.seconds())?
        .checked_sub(i64::from(offset_seconds))?;
    secs.checked_mul(1000)?.checked_add(i64::from(clock.nano / 1_000_000))
}

/// Milliseconds since the Unix epoch of a local `date` + `clock` in the machine's time zone: the earlier of two readings
/// in a repeated hour, `None` inside a skipped hour.
pub fn local_epoch_ms(date: Date, clock: Clock) -> Option<i64> {
    let leap = clock.nano / 1_000_000;
    let dt = date.at(clock.hour as i8, clock.minute as i8, clock.second as i8, 0);
    let offset = match TimeZone::system().to_ambiguous_timestamp(dt).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        AmbiguousOffset::Fold { before, .. } => before,
        AmbiguousOffset::Gap { .. } => return None,
    };
    let secs = midnight_utc_seconds(date)?
        .checked_add(clock.seconds())?
        .checked_sub(i64::from(offset.seconds()))?;
    secs.checked_mul(1000)?.checked_add(i64::from(leap))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Numeric {
    Year,
    YearDiv100,
    YearMod100,
    IsoYear,
    IsoYearMod100,
    Quarter,
    Month,
    Day,
    WeekFromSun,
    WeekFromMon,
    IsoWeek,
    NumDaysFromSun,
    WeekdayFromMon,
    Ordinal,
    Hour,
    Hour12,
    Minute,
    Second,
    Nanosecond,
    Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixed {
    ShortMonth,
    LongMonth,
    ShortWeekday,
    LongWeekday,
    AmPm,
    /// `%.f`, `%.3f`, `%.6f`, `%.9f`: an optional dot and digits (`Some(n)`: exactly `n`).
    NanoDot(Option<usize>),
    /// `%3f`, `%6f`, `%9f`: exactly that many digits, no dot.
    NanoDigits(usize),
    ZoneName,
    /// `%z`, `%:z`, `%::z`, `%:::z`.
    Offset,
    /// `%#z`.
    OffsetPermissive,
    /// `%+`.
    Rfc3339Relaxed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Literal(String),
    Space,
    Num(Numeric),
    Fixed(Fixed),
    Error,
}

/// A compiled strftime format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    items: Vec<Item>,
}

impl Format {
    /// Compiles a strftime format; an unknown or malformed specifier makes every read of it fail (as it always did).
    pub fn new(fmt: &str) -> Format {
        Format { items: compile(fmt) }
    }

    /// Reads `s` in full (no trailing text).
    fn read(&self, s: &str) -> R<Parsed> {
        let mut p = Parsed::default();
        let rest = parse_items(&mut p, s, &self.items)?;
        if rest.is_empty() {
            Ok(p)
        } else {
            Err(Bad::TooLong)
        }
    }

    /// A date, a time and an offset: the text must carry the offset (or a `%s` timestamp). Milliseconds since the epoch.
    pub fn read_with_offset(&self, s: &str) -> Option<i64> {
        let p = self.read(s).ok()?;
        let (date, clock, offset) = p.to_datetime().ok()?;
        epoch_ms(date, clock, offset)
    }

    /// A date and a time, any offset in the text ignored: `(date, clock)`.
    pub fn read_local(&self, s: &str) -> Option<(Date, Clock)> {
        self.read(s).ok()?.to_naive_datetime(0).ok()
    }

    /// A date only (any time in the text ignored).
    pub fn read_date(&self, s: &str) -> Option<Date> {
        self.read(s).ok()?.to_date().ok()
    }
}

fn compile(fmt: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut rest = fmt;
    while let Some(c) = rest.chars().next() {
        if c == '%' {
            rest = compile_spec(&mut items, rest);
        } else if c.is_whitespace() {
            let end = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
            items.push(Item::Space);
            rest = &rest[end..];
        } else {
            let end = rest.find(|c: char| c.is_whitespace() || c == '%').unwrap_or(rest.len());
            items.push(Item::Literal(rest[..end].to_string()));
            rest = &rest[end..];
        }
        if matches!(items.last(), Some(Item::Error)) {
            break;
        }
    }
    items
}

/// Reads one `%...` specifier at the start of `rest`, pushes its items, returns the rest after it.
fn compile_spec<'a>(items: &mut Vec<Item>, rest: &'a str) -> &'a str {
    use Numeric::*;
    let mut r = &rest[1..];
    let next = |r: &mut &'a str| -> Option<char> {
        let c = r.chars().next()?;
        *r = &r[c.len_utf8()..];
        Some(c)
    };
    macro_rules! bad {
        () => {{
            items.push(Item::Error);
            return r;
        }};
    }
    let Some(mut spec) = next(&mut r) else { bad!() };
    let padded = matches!(spec, '-' | '0' | '_');
    let alternate = spec == '#';
    if padded || alternate {
        let Some(c) = next(&mut r) else { bad!() };
        spec = c;
    }
    if alternate && spec != 'z' {
        bad!();
    }
    let before = items.len();
    let num = |n: Numeric| Item::Num(n);
    let lit = |s: &str| Item::Literal(s.to_string());
    match spec {
        'A' => items.push(Item::Fixed(Fixed::LongWeekday)),
        'B' => items.push(Item::Fixed(Fixed::LongMonth)),
        'C' => items.push(num(YearDiv100)),
        'D' => items.extend([num(Month), lit("/"), num(Day), lit("/"), num(YearMod100)]),
        'F' => items.extend([num(Year), lit("-"), num(Month), lit("-"), num(Day)]),
        'G' => items.push(num(IsoYear)),
        'H' => items.push(num(Hour)),
        'I' => items.push(num(Hour12)),
        'M' => items.push(num(Minute)),
        'P' | 'p' => items.push(Item::Fixed(Fixed::AmPm)),
        'R' => items.extend([num(Hour), lit(":"), num(Minute)]),
        'S' => items.push(num(Second)),
        'T' | 'X' => items.extend([num(Hour), lit(":"), num(Minute), lit(":"), num(Second)]),
        'U' => items.push(num(WeekFromSun)),
        'V' => items.push(num(IsoWeek)),
        'W' => items.push(num(WeekFromMon)),
        'Y' => items.push(num(Year)),
        'Z' => items.push(Item::Fixed(Fixed::ZoneName)),
        'a' => items.push(Item::Fixed(Fixed::ShortWeekday)),
        'b' | 'h' => items.push(Item::Fixed(Fixed::ShortMonth)),
        'c' => items.extend([
            Item::Fixed(Fixed::ShortWeekday),
            Item::Space,
            Item::Fixed(Fixed::ShortMonth),
            Item::Space,
            num(Day),
            Item::Space,
            num(Hour),
            lit(":"),
            num(Minute),
            lit(":"),
            num(Second),
            Item::Space,
            num(Year),
        ]),
        'd' | 'e' => items.push(num(Day)),
        'f' => items.push(num(Nanosecond)),
        'g' => items.push(num(IsoYearMod100)),
        'j' => items.push(num(Ordinal)),
        'k' => items.push(num(Hour)),
        'l' => items.push(num(Hour12)),
        'm' => items.push(num(Month)),
        'n' | 't' => items.push(Item::Space),
        'q' => items.push(num(Quarter)),
        'r' => items.extend([
            num(Hour12),
            lit(":"),
            num(Minute),
            lit(":"),
            num(Second),
            Item::Space,
            Item::Fixed(Fixed::AmPm),
        ]),
        's' => items.push(num(Timestamp)),
        'u' => items.push(num(WeekdayFromMon)),
        'v' => items.extend([num(Day), lit("-"), Item::Fixed(Fixed::ShortMonth), lit("-"), num(Year)]),
        'w' => items.push(num(NumDaysFromSun)),
        'x' => items.extend([num(Month), lit("/"), num(Day), lit("/"), num(YearMod100)]),
        'y' => items.push(num(YearMod100)),
        'z' => items.push(Item::Fixed(if alternate { Fixed::OffsetPermissive } else { Fixed::Offset })),
        '+' => items.push(Item::Fixed(Fixed::Rfc3339Relaxed)),
        ':' => {
            if let Some(tail) = r.strip_prefix("::z") {
                r = tail;
            } else if let Some(tail) = r.strip_prefix(":z") {
                r = tail;
            } else if let Some(tail) = r.strip_prefix('z') {
                r = tail;
            } else {
                bad!();
            }
            items.push(Item::Fixed(Fixed::Offset));
        }
        '.' => {
            let digits = match next(&mut r) {
                Some('f') => None,
                Some(d @ ('3' | '6' | '9')) => {
                    if next(&mut r) != Some('f') {
                        bad!();
                    }
                    Some((d as u8 - b'0') as usize)
                }
                _ => bad!(),
            };
            items.push(Item::Fixed(Fixed::NanoDot(digits)));
        }
        d @ ('3' | '6' | '9') => {
            if next(&mut r) != Some('f') {
                bad!();
            }
            items.push(Item::Fixed(Fixed::NanoDigits((d as u8 - b'0') as usize)));
        }
        '%' => items.push(lit("%")),
        _ => bad!(),
    }
    // A padding modifier is allowed on a single numeric item only.
    if padded && !(items.len() == before + 1 && matches!(items[before], Item::Num(_))) {
        items.truncate(before);
        items.push(Item::Error);
    }
    r
}

/// The numeric fields a format filled in; chrono's `Parsed`.
#[derive(Debug, Default, Clone)]
struct Parsed {
    year: Option<i32>,
    year_div_100: Option<i32>,
    year_mod_100: Option<i32>,
    isoyear: Option<i32>,
    isoyear_div_100: Option<i32>,
    isoyear_mod_100: Option<i32>,
    quarter: Option<u32>,
    month: Option<u32>,
    week_from_sun: Option<u32>,
    week_from_mon: Option<u32>,
    isoweek: Option<u32>,
    weekday: Option<Weekday>,
    ordinal: Option<u32>,
    day: Option<u32>,
    hour_div_12: Option<u32>,
    hour_mod_12: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    nanosecond: Option<u32>,
    timestamp: Option<i64>,
    offset: Option<i32>,
}

fn set<T: PartialEq>(old: &mut Option<T>, new: T) -> R<()> {
    match old {
        Some(o) if *o != new => Err(Bad::Impossible),
        _ => {
            *old = Some(new);
            Ok(())
        }
    }
}

fn in_range(v: i64, lo: i64, hi: i64) -> R<i64> {
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(Bad::OutOfRange)
    }
}

impl Parsed {
    fn set_numeric(&mut self, n: Numeric, v: i64) -> R<()> {
        use Numeric::*;
        let to_i32 = |v: i64| i32::try_from(v).map_err(|_| Bad::OutOfRange);
        match n {
            Year => set(&mut self.year, to_i32(v)?),
            YearDiv100 => set(&mut self.year_div_100, in_range(v, 0, i64::from(i32::MAX))? as i32),
            YearMod100 => set(&mut self.year_mod_100, in_range(v, 0, 99)? as i32),
            IsoYear => set(&mut self.isoyear, to_i32(v)?),
            IsoYearMod100 => set(&mut self.isoyear_mod_100, in_range(v, 0, 99)? as i32),
            Quarter => set(&mut self.quarter, in_range(v, 1, 4)? as u32),
            Month => set(&mut self.month, in_range(v, 1, 12)? as u32),
            Day => set(&mut self.day, in_range(v, 1, 31)? as u32),
            WeekFromSun => set(&mut self.week_from_sun, in_range(v, 0, 53)? as u32),
            WeekFromMon => set(&mut self.week_from_mon, in_range(v, 0, 53)? as u32),
            IsoWeek => set(&mut self.isoweek, in_range(v, 1, 53)? as u32),
            NumDaysFromSun => {
                let wd = match v {
                    0 => Weekday::Sunday,
                    1 => Weekday::Monday,
                    2 => Weekday::Tuesday,
                    3 => Weekday::Wednesday,
                    4 => Weekday::Thursday,
                    5 => Weekday::Friday,
                    6 => Weekday::Saturday,
                    _ => return Err(Bad::OutOfRange),
                };
                set(&mut self.weekday, wd)
            }
            WeekdayFromMon => {
                let wd = match v {
                    1 => Weekday::Monday,
                    2 => Weekday::Tuesday,
                    3 => Weekday::Wednesday,
                    4 => Weekday::Thursday,
                    5 => Weekday::Friday,
                    6 => Weekday::Saturday,
                    7 => Weekday::Sunday,
                    _ => return Err(Bad::OutOfRange),
                };
                set(&mut self.weekday, wd)
            }
            Ordinal => set(&mut self.ordinal, in_range(v, 1, 366)? as u32),
            Hour => {
                let (div, modulo) = match v {
                    0..=11 => (0, v as u32),
                    12..=23 => (1, v as u32 - 12),
                    _ => return Err(Bad::OutOfRange),
                };
                set(&mut self.hour_div_12, div)?;
                set(&mut self.hour_mod_12, modulo)
            }
            Hour12 => {
                let v = in_range(v, 1, 12)?;
                set(&mut self.hour_mod_12, if v == 12 { 0 } else { v as u32 })
            }
            Minute => set(&mut self.minute, in_range(v, 0, 59)? as u32),
            Second => set(&mut self.second, in_range(v, 0, 60)? as u32),
            Nanosecond => self.set_nano(v),
            Timestamp => set(&mut self.timestamp, v),
        }
    }

    fn set_nano(&mut self, v: i64) -> R<()> {
        set(&mut self.nanosecond, in_range(v, 0, 999_999_999)? as u32)
    }

    fn set_offset(&mut self, v: i32) -> R<()> {
        set(&mut self.offset, v)
    }

    fn to_date(&self) -> R<Date> {
        fn resolve_year(y: Option<i32>, q: Option<i32>, r: Option<i32>) -> R<Option<i32>> {
            match (y, q, r) {
                (y, None, None) => Ok(y),
                (Some(y), q, r @ Some(0..=99)) | (Some(y), q, r @ None) => {
                    if y < 0 {
                        return Err(Bad::Impossible);
                    }
                    let (q_, r_) = (y / 100, y % 100);
                    if q.unwrap_or(q_) == q_ && r.unwrap_or(r_) == r_ {
                        Ok(Some(y))
                    } else {
                        Err(Bad::Impossible)
                    }
                }
                (None, Some(q), Some(r @ 0..=99)) => {
                    if q < 0 {
                        return Err(Bad::Impossible);
                    }
                    let y = q.checked_mul(100).and_then(|v| v.checked_add(r));
                    Ok(Some(y.ok_or(Bad::OutOfRange)?))
                }
                (None, None, Some(r @ 0..=99)) => Ok(Some(r + if r < 70 { 2000 } else { 1900 })),
                (None, Some(_), None) => Err(Bad::NotEnough),
                (_, _, Some(_)) => Err(Bad::OutOfRange),
            }
        }

        let given_year = resolve_year(self.year, self.year_div_100, self.year_mod_100)?;
        let given_isoyear = resolve_year(self.isoyear, self.isoyear_div_100, self.isoyear_mod_100)?;

        let split = |y: i32| if y >= 0 { (Some(y / 100), Some(y % 100)) } else { (None, None) };
        let verify_ymd = |d: Date| {
            let year = i32::from(d.year());
            let (div, modulo) = split(year);
            self.year.unwrap_or(year) == year
                && self.year_div_100.or(div) == div
                && self.year_mod_100.or(modulo) == modulo
                && self.month.unwrap_or(d.month() as u32) == d.month() as u32
                && self.day.unwrap_or(d.day() as u32) == d.day() as u32
        };
        let verify_iso = |d: Date| {
            let w = d.iso_week_date();
            let isoyear = i32::from(w.year());
            let (div, modulo) = split(isoyear);
            self.isoyear.unwrap_or(isoyear) == isoyear
                && self.isoyear_div_100.or(div) == div
                && self.isoyear_mod_100.or(modulo) == modulo
                && self.isoweek.unwrap_or(w.week() as u32) == w.week() as u32
                && self.weekday.unwrap_or(d.weekday()) == d.weekday()
        };
        let verify_ordinal = |d: Date| {
            let ordinal = d.day_of_year() as u32;
            let weeks_from = |start: Weekday| (i32::from(d.day_of_year()) - 1 + 7 - i32::from(d.weekday().since(start))) / 7;
            self.ordinal.unwrap_or(ordinal) == ordinal
                && self.week_from_sun.map_or(weeks_from(Weekday::Sunday), |v| v as i32) == weeks_from(Weekday::Sunday)
                && self.week_from_mon.map_or(weeks_from(Weekday::Monday), |v| v as i32) == weeks_from(Weekday::Monday)
        };

        let year_i16 = |y: i32| i16::try_from(y).ok().filter(|y| (-9999..=9999).contains(y)).ok_or(Bad::OutOfRange);
        let (verified, date) = if let (Some(year), Some(month), Some(day)) = (given_year, self.month, self.day) {
            let date = Date::new(year_i16(year)?, month as i8, day as i8).map_err(|_| Bad::OutOfRange)?;
            (verify_iso(date) && verify_ordinal(date), date)
        } else if let (Some(year), Some(ordinal)) = (given_year, self.ordinal) {
            let date = from_yo(year_i16(year)?, ordinal).ok_or(Bad::OutOfRange)?;
            (verify_ymd(date) && verify_iso(date) && verify_ordinal(date), date)
        } else if let (Some(year), Some(week), Some(weekday)) = (given_year, self.week_from_sun, self.weekday) {
            let date = resolve_week_date(year_i16(year)?, week, weekday, Weekday::Sunday)?;
            (verify_ymd(date) && verify_iso(date) && verify_ordinal(date), date)
        } else if let (Some(year), Some(week), Some(weekday)) = (given_year, self.week_from_mon, self.weekday) {
            let date = resolve_week_date(year_i16(year)?, week, weekday, Weekday::Monday)?;
            (verify_ymd(date) && verify_iso(date) && verify_ordinal(date), date)
        } else if let (Some(isoyear), Some(isoweek), Some(weekday)) = (given_isoyear, self.isoweek, self.weekday) {
            let date = ISOWeekDate::new(year_i16(isoyear)?, isoweek as i8, weekday)
                .map_err(|_| Bad::OutOfRange)?
                .date();
            (verify_ymd(date) && verify_ordinal(date), date)
        } else {
            return Err(Bad::NotEnough);
        };

        if !verified {
            return Err(Bad::Impossible);
        }
        if let Some(q) = self.quarter {
            if q != (date.month() as u32 - 1) / 3 + 1 {
                return Err(Bad::Impossible);
            }
        }
        Ok(date)
    }

    fn to_clock(&self) -> R<Clock> {
        let div = match self.hour_div_12 {
            Some(v @ 0..=1) => v,
            Some(_) => return Err(Bad::OutOfRange),
            None => return Err(Bad::NotEnough),
        };
        let modulo = match self.hour_mod_12 {
            Some(v @ 0..=11) => v,
            Some(_) => return Err(Bad::OutOfRange),
            None => return Err(Bad::NotEnough),
        };
        let minute = match self.minute {
            Some(v @ 0..=59) => v,
            Some(_) => return Err(Bad::OutOfRange),
            None => return Err(Bad::NotEnough),
        };
        let (second, mut nano) = match self.second.unwrap_or(0) {
            v @ 0..=59 => (v, 0),
            60 => (59, 1_000_000_000),
            _ => return Err(Bad::OutOfRange),
        };
        nano += match self.nanosecond {
            Some(v @ 0..=999_999_999) if self.second.is_some() => v,
            Some(0..=999_999_999) => return Err(Bad::NotEnough),
            Some(_) => return Err(Bad::OutOfRange),
            None => 0,
        };
        Clock::new(div * 12 + modulo, minute, second, nano).ok_or(Bad::OutOfRange)
    }

    fn to_naive_datetime(&self, offset: i32) -> R<(Date, Clock)> {
        let date = self.to_date();
        let clock = self.to_clock();
        if let (Ok(date), Ok(clock)) = (date, clock) {
            if let Some(given) = self.timestamp {
                let timestamp = midnight_utc_seconds(date).ok_or(Bad::OutOfRange)? + clock.seconds() - i64::from(offset);
                if given != timestamp && !(clock.nano >= 1_000_000_000 && given == timestamp + 1) {
                    return Err(Bad::Impossible);
                }
            }
            return Ok((date, clock));
        }
        let Some(timestamp) = self.timestamp else {
            date?;
            clock?;
            return Err(Bad::NotEnough);
        };
        match (date, clock) {
            (Err(Bad::OutOfRange), _) | (_, Err(Bad::OutOfRange)) => return Err(Bad::OutOfRange),
            (Err(Bad::Impossible), _) | (_, Err(Bad::Impossible)) => return Err(Bad::Impossible),
            _ => {}
        }
        let ts = timestamp.checked_add(i64::from(offset)).ok_or(Bad::OutOfRange)?;
        let (mut date, mut secs_of_day) = civil_from_seconds(ts).ok_or(Bad::OutOfRange)?;
        let mut parsed = self.clone();
        if parsed.second == Some(60) {
            match secs_of_day % 60 {
                59 => {}
                0 => {
                    // Off by one second: the reading is the leap second's successor.
                    (date, secs_of_day) = civil_from_seconds(ts - 1).ok_or(Bad::OutOfRange)?;
                }
                _ => return Err(Bad::Impossible),
            }
        } else {
            parsed.set_numeric(Numeric::Second, secs_of_day % 60)?;
        }
        parsed.set_numeric(Numeric::Year, i64::from(date.year()))?;
        parsed.set_numeric(Numeric::Ordinal, i64::from(date.day_of_year()))?;
        parsed.set_numeric(Numeric::Hour, secs_of_day / 3600)?;
        parsed.set_numeric(Numeric::Minute, secs_of_day / 60 % 60)?;
        Ok((parsed.to_date()?, parsed.to_clock()?))
    }

    /// Date, time and the offset in seconds the text carried (a `%s` timestamp stands for offset 0).
    fn to_datetime(&self) -> R<(Date, Clock, i32)> {
        let offset = match (self.offset, self.timestamp) {
            (Some(o), _) => o,
            (None, Some(_)) => 0,
            (None, None) => return Err(Bad::NotEnough),
        };
        let (date, clock) = self.to_naive_datetime(offset)?;
        // A fixed offset is under 24 hours.
        if offset.abs() >= 86_400 {
            return Err(Bad::OutOfRange);
        }
        Ok((date, clock, offset))
    }
}

/// The UTC date and the second of that day of epoch second `ts`.
fn civil_from_seconds(ts: i64) -> Option<(Date, i64)> {
    let days = i32::try_from(ts.div_euclid(86_400)).ok()?;
    let date = Date::constant(1970, 1, 1)
        .checked_add(jiff::Span::new().try_days(days).ok()?)
        .ok()?;
    Some((date, ts.rem_euclid(86_400)))
}

fn from_yo(year: i16, ordinal: u32) -> Option<Date> {
    Date::new(year, 1, 1)
        .ok()?
        .with()
        .day_of_year(i16::try_from(ordinal).ok()?)
        .build()
        .ok()
}

fn resolve_week_date(year: i16, week: u32, weekday: Weekday, week_start: Weekday) -> R<Date> {
    if week > 53 {
        return Err(Bad::OutOfRange);
    }
    let first = from_yo(year, 1).ok_or(Bad::OutOfRange)?;
    let first_week_start = 1 + i32::from(week_start.since(first.weekday()));
    let wd = i32::from(weekday.since(week_start));
    let ordinal = first_week_start + (week as i32 - 1) * 7 + wd;
    if ordinal <= 0 {
        return Err(Bad::Impossible);
    }
    from_yo(year, ordinal as u32).ok_or(Bad::Impossible)
}

// ---- scanning ----

fn number(s: &str, min: usize, max: usize) -> R<(&str, i64)> {
    let bytes = s.as_bytes();
    if bytes.len() < min {
        return Err(Bad::TooShort);
    }
    let mut n = 0i64;
    for (i, c) in bytes.iter().copied().take(max).enumerate() {
        if !c.is_ascii_digit() {
            if i < min {
                return Err(Bad::Invalid);
            }
            return Ok((&s[i..], n));
        }
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(c - b'0')))
            .ok_or(Bad::OutOfRange)?;
    }
    Ok((&s[max.min(bytes.len())..], n))
}

const NANO_SCALE: [i64; 10] = [0, 100_000_000, 10_000_000, 1_000_000, 100_000, 10_000, 1_000, 100, 10, 1];

fn nanosecond(s: &str) -> R<(&str, u32)> {
    let orig = s.len();
    let (s, v) = number(s, 1, 9)?;
    let consumed = orig - s.len();
    let v = (v * NANO_SCALE[consumed]) as u32;
    Ok((s.trim_start_matches(|c: char| c.is_ascii_digit()), v))
}

fn nanosecond_fixed(s: &str, digits: usize) -> R<(&str, i64)> {
    let (s, v) = number(s, digits, digits)?;
    Ok((s, v * NANO_SCALE[digits]))
}

fn short_month0(s: &str) -> R<(&str, u8)> {
    if s.len() < 3 {
        return Err(Bad::TooShort);
    }
    let b = s.as_bytes();
    let month0 = match (b[0] | 32, b[1] | 32, b[2] | 32) {
        (b'j', b'a', b'n') => 0,
        (b'f', b'e', b'b') => 1,
        (b'm', b'a', b'r') => 2,
        (b'a', b'p', b'r') => 3,
        (b'm', b'a', b'y') => 4,
        (b'j', b'u', b'n') => 5,
        (b'j', b'u', b'l') => 6,
        (b'a', b'u', b'g') => 7,
        (b's', b'e', b'p') => 8,
        (b'o', b'c', b't') => 9,
        (b'n', b'o', b'v') => 10,
        (b'd', b'e', b'c') => 11,
        _ => return Err(Bad::Invalid),
    };
    Ok((&s[3..], month0))
}

fn short_weekday(s: &str) -> R<(&str, Weekday)> {
    if s.len() < 3 {
        return Err(Bad::TooShort);
    }
    let b = s.as_bytes();
    let wd = match (b[0] | 32, b[1] | 32, b[2] | 32) {
        (b'm', b'o', b'n') => Weekday::Monday,
        (b't', b'u', b'e') => Weekday::Tuesday,
        (b'w', b'e', b'd') => Weekday::Wednesday,
        (b't', b'h', b'u') => Weekday::Thursday,
        (b'f', b'r', b'i') => Weekday::Friday,
        (b's', b'a', b't') => Weekday::Saturday,
        (b's', b'u', b'n') => Weekday::Sunday,
        _ => return Err(Bad::Invalid),
    };
    Ok((&s[3..], wd))
}

fn short_or_long_month0(s: &str) -> R<(&str, u8)> {
    const SUFFIX: [&[u8]; 12] = [
        b"uary", b"ruary", b"ch", b"il", b"", b"e", b"y", b"ust", b"tember", b"ober", b"ember", b"ember",
    ];
    let (mut s, m) = short_month0(s)?;
    let suffix = SUFFIX[m as usize];
    if s.len() >= suffix.len() && s.as_bytes()[..suffix.len()].eq_ignore_ascii_case(suffix) {
        s = &s[suffix.len()..];
    }
    Ok((s, m))
}

fn short_or_long_weekday(s: &str) -> R<(&str, Weekday)> {
    let (mut s, wd) = short_weekday(s)?;
    let suffix: &[u8] = match wd {
        Weekday::Monday | Weekday::Friday | Weekday::Sunday => b"day",
        Weekday::Tuesday => b"sday",
        Weekday::Wednesday => b"nesday",
        Weekday::Thursday => b"rsday",
        Weekday::Saturday => b"urday",
    };
    if s.len() >= suffix.len() && s.as_bytes()[..suffix.len()].eq_ignore_ascii_case(suffix) {
        s = &s[suffix.len()..];
    }
    Ok((s, wd))
}

fn char_(s: &str, c: u8) -> R<&str> {
    match s.as_bytes().first() {
        Some(&x) if x == c => Ok(&s[1..]),
        Some(_) => Err(Bad::Invalid),
        None => Err(Bad::TooShort),
    }
}

fn space(s: &str) -> R<&str> {
    let t = s.trim_start();
    if t.len() < s.len() {
        Ok(t)
    } else if s.is_empty() {
        Err(Bad::TooShort)
    } else {
        Err(Bad::Invalid)
    }
}

fn colon_or_space(s: &str) -> R<&str> {
    Ok(s.trim_start_matches(|c: char| c == ':' || c.is_whitespace()))
}

fn timezone_offset<'a>(
    mut s: &'a str,
    colon: impl Fn(&'a str) -> R<&'a str>,
    allow_zulu: bool,
    allow_missing_minutes: bool,
    allow_unicode_minus: bool,
) -> R<(&'a str, i32)> {
    if allow_zulu {
        if let Some(&(b'Z' | b'z')) = s.as_bytes().first() {
            return Ok((&s[1..], 0));
        }
    }
    let digits = |s: &str| -> R<(u8, u8)> {
        let b = s.as_bytes();
        if b.len() < 2 {
            Err(Bad::TooShort)
        } else {
            Ok((b[0], b[1]))
        }
    };
    let negative = match s.chars().next() {
        Some('+') => {
            s = &s[1..];
            false
        }
        Some('-') => {
            s = &s[1..];
            true
        }
        Some('\u{2212}') => {
            if !allow_unicode_minus {
                return Err(Bad::Invalid);
            }
            s = &s['\u{2212}'.len_utf8()..];
            true
        }
        Some(_) => return Err(Bad::Invalid),
        None => return Err(Bad::TooShort),
    };
    let hours = match digits(s)? {
        (h1 @ b'0'..=b'9', h2 @ b'0'..=b'9') => i32::from((h1 - b'0') * 10 + (h2 - b'0')),
        _ => return Err(Bad::Invalid),
    };
    s = &s[2..];
    s = colon(s)?;
    let minutes = if let Ok(ds) = digits(s) {
        match ds {
            (m1 @ b'0'..=b'5', m2 @ b'0'..=b'9') => i32::from((m1 - b'0') * 10 + (m2 - b'0')),
            (b'6'..=b'9', b'0'..=b'9') => return Err(Bad::OutOfRange),
            _ => return Err(Bad::Invalid),
        }
    } else if allow_missing_minutes {
        0
    } else {
        return Err(Bad::TooShort);
    };
    s = match s.len() {
        len if len >= 2 => &s[2..],
        0 => s,
        _ => return Err(Bad::TooShort),
    };
    let seconds = hours * 3600 + minutes * 60;
    Ok((s, if negative { -seconds } else { seconds }))
}

fn timezone_offset_2822(s: &str) -> R<(&str, i32)> {
    let upto = s.as_bytes().iter().position(|&c| !c.is_ascii_alphabetic()).unwrap_or(s.len());
    if upto > 0 {
        let name = &s.as_bytes()[..upto];
        let rest = &s[upto..];
        let hours = |o: i32| Ok((rest, o * 3600));
        if name.eq_ignore_ascii_case(b"gmt") || name.eq_ignore_ascii_case(b"ut") || name.eq_ignore_ascii_case(b"z") {
            return hours(0);
        } else if name.eq_ignore_ascii_case(b"edt") {
            return hours(-4);
        } else if name.eq_ignore_ascii_case(b"est") || name.eq_ignore_ascii_case(b"cdt") {
            return hours(-5);
        } else if name.eq_ignore_ascii_case(b"cst") || name.eq_ignore_ascii_case(b"mdt") {
            return hours(-6);
        } else if name.eq_ignore_ascii_case(b"mst") || name.eq_ignore_ascii_case(b"pdt") {
            return hours(-7);
        } else if name.eq_ignore_ascii_case(b"pst") {
            return hours(-8);
        } else if name.len() == 1 {
            if let b'a'..=b'i' | b'k'..=b'y' | b'A'..=b'I' | b'K'..=b'Y' = name[0] {
                return Ok((rest, 0));
            }
        }
        Err(Bad::Invalid)
    } else {
        timezone_offset(s, Ok, false, false, false)
    }
}

fn comment_2822(s: &str) -> R<&str> {
    #[derive(Clone, Copy)]
    enum St {
        Start,
        Next(usize),
        Escape(usize),
    }
    let s = s.trim_start();
    let mut state = St::Start;
    for (i, c) in s.bytes().enumerate() {
        state = match (state, c) {
            (St::Start, b'(') => St::Next(1),
            (St::Next(1), b')') => return Ok(&s[i + 1..]),
            (St::Next(d), b'\\') => St::Escape(d),
            (St::Next(d), b'(') => St::Next(d + 1),
            (St::Next(d), b')') => St::Next(d - 1),
            (St::Next(d), _) | (St::Escape(d), _) => St::Next(d),
            _ => return Err(Bad::Invalid),
        };
    }
    Err(Bad::TooShort)
}

// ---- the item reader ----

fn parse_items<'a>(p: &mut Parsed, mut s: &'a str, items: &[Item]) -> R<&'a str> {
    for item in items {
        match item {
            Item::Literal(lit) => {
                if s.len() < lit.len() {
                    return Err(Bad::TooShort);
                }
                s = s.strip_prefix(lit.as_str()).ok_or(Bad::Invalid)?;
            }
            Item::Space => s = s.trim_start(),
            Item::Num(n) => {
                use Numeric::*;
                let (width, signed) = match n {
                    Year | IsoYear => (4, true),
                    YearDiv100 | YearMod100 | IsoYearMod100 => (2, false),
                    Quarter | NumDaysFromSun | WeekdayFromMon => (1, false),
                    Month | Day | WeekFromSun | WeekFromMon | IsoWeek | Hour | Hour12 | Minute | Second => (2, false),
                    Ordinal => (3, false),
                    Nanosecond => (9, false),
                    Timestamp => (usize::MAX, false),
                };
                s = s.trim_start();
                let v = if signed && s.starts_with('-') {
                    let (rest, v) = number(&s[1..], 1, usize::MAX)?;
                    s = rest;
                    0i64.checked_sub(v).ok_or(Bad::OutOfRange)?
                } else if signed && s.starts_with('+') {
                    let (rest, v) = number(&s[1..], 1, usize::MAX)?;
                    s = rest;
                    v
                } else {
                    let (rest, v) = number(s, 1, width)?;
                    s = rest;
                    v
                };
                p.set_numeric(*n, v)?;
            }
            Item::Fixed(f) => s = parse_fixed(p, s, *f)?,
            Item::Error => return Err(Bad::UnknownSpecifier),
        }
    }
    Ok(s)
}

fn parse_fixed<'a>(p: &mut Parsed, mut s: &'a str, f: Fixed) -> R<&'a str> {
    match f {
        Fixed::ShortMonth => {
            let (rest, m) = short_month0(s)?;
            p.set_numeric(Numeric::Month, i64::from(m) + 1)?;
            s = rest;
        }
        Fixed::LongMonth => {
            let (rest, m) = short_or_long_month0(s)?;
            p.set_numeric(Numeric::Month, i64::from(m) + 1)?;
            s = rest;
        }
        Fixed::ShortWeekday => {
            let (rest, w) = short_weekday(s)?;
            set(&mut p.weekday, w)?;
            s = rest;
        }
        Fixed::LongWeekday => {
            let (rest, w) = short_or_long_weekday(s)?;
            set(&mut p.weekday, w)?;
            s = rest;
        }
        Fixed::AmPm => {
            if s.len() < 2 {
                return Err(Bad::TooShort);
            }
            let pm = match (s.as_bytes()[0] | 32, s.as_bytes()[1] | 32) {
                (b'a', b'm') => false,
                (b'p', b'm') => true,
                _ => return Err(Bad::Invalid),
            };
            set(&mut p.hour_div_12, u32::from(pm))?;
            s = &s[2..];
        }
        Fixed::NanoDot(None) => {
            if let Some(rest) = s.strip_prefix('.') {
                let (rest, nano) = nanosecond(rest)?;
                p.set_nano(i64::from(nano))?;
                s = rest;
            }
        }
        Fixed::NanoDot(Some(digits)) => {
            if let Some(rest) = s.strip_prefix('.') {
                let (rest, nano) = nanosecond_fixed(rest, digits)?;
                p.set_nano(nano)?;
                s = rest;
            }
        }
        Fixed::NanoDigits(digits) => {
            if s.len() < digits {
                return Err(Bad::TooShort);
            }
            let (rest, nano) = nanosecond_fixed(s, digits)?;
            p.set_nano(nano)?;
            s = rest;
        }
        Fixed::ZoneName => s = s.trim_start_matches(|c: char| !c.is_whitespace()),
        Fixed::Offset => {
            let (rest, off) = timezone_offset(s.trim_start(), colon_or_space, false, false, true)?;
            p.set_offset(off)?;
            s = rest;
        }
        Fixed::OffsetPermissive => {
            let (rest, off) = timezone_offset(s.trim_start(), colon_or_space, true, true, true)?;
            p.set_offset(off)?;
            s = rest;
        }
        Fixed::Rfc3339Relaxed => s = parse_rfc3339_relaxed(p, s)?,
    }
    Ok(s)
}

fn parse_rfc3339_relaxed<'a>(p: &mut Parsed, mut s: &'a str) -> R<&'a str> {
    use Numeric::*;
    let date = [
        Item::Num(Year),
        Item::Space,
        Item::Literal("-".into()),
        Item::Num(Month),
        Item::Space,
        Item::Literal("-".into()),
        Item::Num(Day),
    ];
    let time = [
        Item::Num(Hour),
        Item::Space,
        Item::Literal(":".into()),
        Item::Num(Minute),
        Item::Space,
        Item::Literal(":".into()),
        Item::Num(Second),
        Item::Fixed(Fixed::NanoDot(None)),
        Item::Space,
    ];
    s = parse_items(p, s, &date)?;
    s = match s.as_bytes().first() {
        Some(b't' | b'T' | b' ') => &s[1..],
        Some(_) => return Err(Bad::Invalid),
        None => return Err(Bad::TooShort),
    };
    s = parse_items(p, s, &time)?;
    s = s.trim_start();
    let (rest, offset) = if s.len() >= 3 && s.as_bytes()[..3].eq_ignore_ascii_case(b"UTC") {
        (&s[3..], 0)
    } else {
        timezone_offset(s, colon_or_space, true, false, true)?
    };
    p.set_offset(offset)?;
    Ok(rest)
}

// ---- RFC 3339 and RFC 2822 ----

/// Milliseconds since the epoch of a strict RFC 3339 timestamp (`T`, `t` or a space between date and time, a leap second
/// counted as the next second, any number of fraction digits, `Z` or `+hh:mm`); `None` for anything else.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    rfc3339(s).ok()
}

fn rfc3339(mut s: &str) -> R<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return Err(Bad::TooShort);
    }
    let digit = |i: usize| -> R<u32> {
        if bytes[i].is_ascii_digit() {
            Ok(u32::from(bytes[i] - b'0'))
        } else {
            Err(Bad::Invalid)
        }
    };
    let year = digit(0)? * 1000 + digit(1)? * 100 + digit(2)? * 10 + digit(3)?;
    if bytes[4] != b'-' {
        return Err(Bad::Invalid);
    }
    let month = digit(5)? * 10 + digit(6)?;
    if bytes[7] != b'-' {
        return Err(Bad::Invalid);
    }
    let day = digit(8)? * 10 + digit(9)?;
    let date = Date::new(year as i16, month as i8, day as i8).map_err(|_| Bad::OutOfRange)?;
    if !matches!(bytes[10], b't' | b'T' | b' ') {
        return Err(Bad::Invalid);
    }
    let hour = digit(11)? * 10 + digit(12)?;
    if bytes[13] != b':' {
        return Err(Bad::Invalid);
    }
    let min = digit(14)? * 10 + digit(15)?;
    if bytes[16] != b':' {
        return Err(Bad::Invalid);
    }
    let sec = digit(17)? * 10 + digit(18)?;
    let (sec, extra) = if sec == 60 { (59, 1_000_000_000) } else { (sec, 0) };
    let nano = if bytes.get(19) == Some(&b'.') {
        let (rest, n) = nanosecond(&s[20..])?;
        s = rest;
        extra + n
    } else {
        s = &s[19..];
        extra
    };
    let clock = Clock::new(hour, min, sec, nano).ok_or(Bad::OutOfRange)?;
    let (rest, offset) = timezone_offset(s, |s| char_(s, b':'), true, false, true)?;
    if !rest.is_empty() {
        return Err(Bad::TooLong);
    }
    if offset.abs() >= 86_400 {
        return Err(Bad::OutOfRange);
    }
    epoch_ms(date, clock, offset).ok_or(Bad::OutOfRange)
}

/// Milliseconds since the epoch of an RFC 2822 timestamp (`Wed, 14 Oct 2026 10:41:22 +0000`, with the legacy zone names,
/// two- and three-digit years and trailing comments); `None` for anything else.
pub fn parse_rfc2822_ms(s: &str) -> Option<i64> {
    rfc2822(s).ok()
}

fn rfc2822(s: &str) -> R<i64> {
    let mut p = Parsed::default();
    let mut s = s.trim_start();
    if let Ok((rest, weekday)) = short_weekday(s) {
        s = rest.strip_prefix(',').ok_or(Bad::Invalid)?;
        set(&mut p.weekday, weekday)?;
    }
    s = s.trim_start();
    let (rest, day) = number(s, 1, 2)?;
    s = rest;
    p.set_numeric(Numeric::Day, day)?;
    s = space(s)?;
    let (rest, m0) = short_month0(s)?;
    s = rest;
    p.set_numeric(Numeric::Month, i64::from(m0) + 1)?;
    s = space(s)?;
    let prev = s.len();
    let (rest, mut year) = number(s, 2, usize::MAX)?;
    s = rest;
    match (prev - s.len(), year) {
        (2, 0..=49) => year += 2000,
        (2, 50..=99) => year += 1900,
        (3, _) => year += 1900,
        _ => {}
    }
    p.set_numeric(Numeric::Year, year)?;
    s = space(s)?;
    let (rest, hour) = number(s, 2, 2)?;
    s = rest;
    p.set_numeric(Numeric::Hour, hour)?;
    s = char_(s.trim_start(), b':')?.trim_start();
    let (rest, minute) = number(s, 2, 2)?;
    s = rest;
    p.set_numeric(Numeric::Minute, minute)?;
    if let Ok(after) = char_(s.trim_start(), b':') {
        let (rest, second) = number(after, 2, 2)?;
        s = rest;
        p.set_numeric(Numeric::Second, second)?;
    }
    s = space(s)?;
    let (rest, offset) = timezone_offset_2822(s)?;
    s = rest;
    p.set_offset(offset)?;
    while let Ok(rest) = comment_2822(s) {
        s = rest;
    }
    if !s.is_empty() {
        return Err(Bad::TooLong);
    }
    let (date, clock, offset) = p.to_datetime()?;
    epoch_ms(date, clock, offset).ok_or(Bad::OutOfRange)
}

/// A calendar date read with a strftime `format` (any time in the text is ignored); `None` when the text does not fit.
pub fn parse_date(text: &str, format: &str) -> Option<Date> {
    Format::new(format).read_date(text)
}

/// A date and time of day read with a strftime `format` and taken as UTC (any offset in the text ignored), as milliseconds
/// since the Unix epoch; `None` when the text does not fit.
pub fn parse_utc_ms(text: &str, format: &str) -> Option<i64> {
    let (date, clock) = parse_datetime(text, format)?;
    epoch_ms(date, clock, 0)
}

/// A date and time of day read with a strftime `format` (any offset in the text is ignored); `None` when the text does not fit.
pub fn parse_datetime(text: &str, format: &str) -> Option<(Date, Clock)> {
    Format::new(format).read_local(text)
}
