//! Go `time` layout parsing and formatting.
//!
//! CUE's `time` package exposes Go's layout language. This module implements
//! the layout engine (layout tokenization, parsing, formatting, RFC3339Nano
//! values) without depending on a Go runtime.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Timelike, Utc};

/// A parsed time value with the zone offset it was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedTime {
    pub(crate) local: chrono::NaiveDateTime,
    pub(crate) offset_seconds: i32,
}

impl ParsedTime {
    #[must_use]
    pub(crate) fn utc(&self) -> DateTime<Utc> {
        (self.local - chrono::Duration::seconds(i64::from(self.offset_seconds))).and_utc()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Literal(String),
    Year4,
    Year2,
    MonthLong,
    MonthShort,
    MonthNum,
    MonthNum0,
    Day,
    DaySpace,
    Day0,
    YDay0,
    YDaySpace,
    WeekdayLong,
    WeekdayShort,
    Hour24,
    Hour12,
    Hour12Zero,
    Minute,
    MinuteZero,
    Second,
    SecondZero,
    PmUpper,
    PmLower,
    ZoneName,
    ZoneNumeric(ZoneStyle),
    Fraction {
        digits: usize,
        trim: bool,
        separator: char,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoneStyle {
    Short,
    Basic,
    Colon,
    Seconds,
    SecondsColon,
    IsoShort,
    IsoBasic,
    IsoColon,
    IsoSeconds,
    IsoSecondsColon,
}

impl ZoneStyle {
    fn iso(self) -> bool {
        matches!(
            self,
            Self::IsoShort
                | Self::IsoBasic
                | Self::IsoColon
                | Self::IsoSeconds
                | Self::IsoSecondsColon
        )
    }
}

const MONTHS_LONG: [&str; 12] = [
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
];

const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const WEEKDAYS_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn token_at(text: &str) -> Option<(Item, usize)> {
    const SIMPLE: &[(&str, Item)] = &[
        ("January", Item::MonthLong),
        ("Monday", Item::WeekdayLong),
        ("Z07:00:00", Item::ZoneNumeric(ZoneStyle::IsoSecondsColon)),
        ("Z070000", Item::ZoneNumeric(ZoneStyle::IsoSeconds)),
        ("-07:00:00", Item::ZoneNumeric(ZoneStyle::SecondsColon)),
        ("-070000", Item::ZoneNumeric(ZoneStyle::Seconds)),
        ("2006", Item::Year4),
        ("__2", Item::YDaySpace),
        ("002", Item::YDay0),
        ("Jan", Item::MonthShort),
        ("Mon", Item::WeekdayShort),
        ("Z07:00", Item::ZoneNumeric(ZoneStyle::IsoColon)),
        ("Z0700", Item::ZoneNumeric(ZoneStyle::IsoBasic)),
        ("-07:00", Item::ZoneNumeric(ZoneStyle::Colon)),
        ("-0700", Item::ZoneNumeric(ZoneStyle::Basic)),
        ("Z07", Item::ZoneNumeric(ZoneStyle::IsoShort)),
        ("-07", Item::ZoneNumeric(ZoneStyle::Short)),
        ("01", Item::MonthNum0),
        ("02", Item::Day0),
        ("03", Item::Hour12Zero),
        ("04", Item::MinuteZero),
        ("05", Item::SecondZero),
        ("06", Item::Year2),
        ("15", Item::Hour24),
        ("_2", Item::DaySpace),
        ("1", Item::MonthNum),
        ("2", Item::Day),
        ("3", Item::Hour12),
        ("4", Item::Minute),
        ("5", Item::Second),
        ("PM", Item::PmUpper),
        ("pm", Item::PmLower),
        ("MST", Item::ZoneName),
    ];
    for (pattern, item) in SIMPLE {
        if !text.starts_with(pattern) {
            continue;
        }
        if matches!(item, Item::MonthShort | Item::WeekdayShort)
            && text[pattern.len()..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_lowercase())
        {
            continue;
        }
        return Some((item.clone(), pattern.len()));
    }
    let mut chars = text.chars();
    let first = chars.next()?;
    if first == '.' || first == ',' {
        let rest = chars.as_str();
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            let run = &rest[..digits];
            let all_zero = run.chars().all(|value| value == '0');
            let all_nine = run.chars().all(|value| value == '9');
            if all_zero || all_nine {
                return Some((
                    Item::Fraction {
                        digits,
                        trim: all_nine,
                        separator: first,
                    },
                    first.len_utf8() + digits,
                ));
            }
        }
    }
    None
}

fn tokenize(layout: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    while index < layout.len() {
        let rest = &layout[index..];
        if rest.starts_with("_2006") {
            literal.push('_');
            index += 1;
            continue;
        }
        if let Some((item, consumed)) = token_at(rest) {
            if !literal.is_empty() {
                items.push(Item::Literal(std::mem::take(&mut literal)));
            }
            items.push(item);
            index += consumed;
        } else {
            let character = rest.chars().next().unwrap_or(' ');
            literal.push(character);
            index += character.len_utf8();
        }
    }
    if !literal.is_empty() {
        items.push(Item::Literal(literal));
    }
    items
}

fn digit_value(character: char) -> Option<u32> {
    character.to_digit(10)
}

fn take_digits(input: &str, count: usize) -> Option<(u32, &str)> {
    let mut value = 0u32;
    let mut taken = 0;
    for character in input.chars() {
        if taken == count {
            break;
        }
        value = value
            .checked_mul(10)?
            .checked_add(digit_value(character)?)?;
        taken += 1;
    }
    if taken != count {
        return None;
    }
    Some((value, &input[taken..]))
}

fn take_number(input: &str, min: usize, max: usize) -> Option<(u32, &str)> {
    let mut value = 0u32;
    let mut taken = 0;
    for character in input.chars() {
        if taken == max {
            break;
        }
        let Some(digit) = digit_value(character) else {
            break;
        };
        value = value.checked_mul(10)?.checked_add(digit)?;
        taken += 1;
    }
    if taken < min {
        return None;
    }
    Some((value, &input[taken..]))
}

fn fraction_nanos(digits: &str, used: usize) -> Option<u32> {
    let mut nanos = 0u32;
    let mut taken = 0;
    for character in digits.chars().take(used) {
        nanos = nanos * 10 + digit_value(character)?;
        taken += 1;
    }
    for _ in taken..9 {
        nanos *= 10;
    }
    Some(nanos)
}

fn parse_fraction(input: &str, digits: usize, trim: bool) -> Option<(u32, &str)> {
    let rest = input.strip_prefix(['.', ','])?;
    let count = rest.chars().take_while(char::is_ascii_digit).count();
    if trim {
        if count == 0 {
            return None;
        }
        let nanos = fraction_nanos(rest, count.min(9))?;
        Some((nanos, &rest[count..]))
    } else {
        if count != digits {
            return None;
        }
        let nanos = fraction_nanos(rest, digits.min(9))?;
        Some((nanos, &rest[digits..]))
    }
}

fn parse_nanoseconds(input: &str) -> Option<(u32, &str)> {
    let rest = input.strip_prefix(['.', ','])?;
    let count = rest.chars().take_while(char::is_ascii_digit).count();
    if count == 0 {
        return None;
    }
    let nanos = fraction_nanos(rest, count.min(9))?;
    Some((nanos, &rest[count..]))
}

fn parse_name<'a>(input: &'a str, names: &[&str]) -> Option<(usize, &'a str)> {
    let bytes = input.as_bytes();
    for (index, name) in names.iter().enumerate() {
        if bytes.len() >= name.len() && bytes[..name.len()].eq_ignore_ascii_case(name.as_bytes()) {
            return Some((index, &input[name.len()..]));
        }
    }
    None
}

fn skip_literal<'a>(mut value: &'a str, mut prefix: &str) -> Option<&'a str> {
    while let Some(character) = prefix.chars().next() {
        if character == ' ' {
            if !value.is_empty() && !value.starts_with(' ') {
                return None;
            }
            value = value.trim_start_matches(' ');
            prefix = prefix.trim_start_matches(' ');
        } else {
            value = value.strip_prefix(character)?;
            prefix = &prefix[character.len_utf8()..];
        }
    }
    Some(value)
}

fn parse_signed_offset(input: &str) -> usize {
    let bytes = input.as_bytes();
    if bytes.is_empty() || (bytes[0] != b'+' && bytes[0] != b'-') {
        return 0;
    }
    let mut index = 1;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 1 {
        return 0;
    }
    match input[1..index].parse::<u32>() {
        Ok(value) if value <= 23 => index,
        _ => 0,
    }
}

fn parse_zone_name(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    if bytes.starts_with(b"UTC") {
        return Some(3);
    }
    if bytes.starts_with(b"ChST") || bytes.starts_with(b"MeST") {
        return Some(4);
    }
    if bytes.starts_with(b"GMT") {
        let extra = parse_signed_offset(&input[3..]);
        return Some(if extra == 0 { 3 } else { 3 + extra });
    }
    if bytes[0] == b'+' || bytes[0] == b'-' {
        let consumed = parse_signed_offset(input);
        return (consumed > 0).then_some(consumed);
    }
    let mut length = 0;
    while length < 6 && length < bytes.len() && bytes[length].is_ascii_uppercase() {
        length += 1;
    }
    match length {
        3 => Some(3),
        4 if bytes[3] == b'T' || &bytes[..4] == b"WITA" => Some(4),
        5 if bytes[4] == b'T' => Some(5),
        _ => None,
    }
}

fn parse_zone(input: &str, style: ZoneStyle) -> Option<(i32, &str)> {
    if style.iso() && input.starts_with('Z') {
        return Some((0, &input[1..]));
    }
    let (sign, rest) = match input.chars().next()? {
        '+' => (1i64, &input[1..]),
        '-' => (-1i64, &input[1..]),
        _ => return None,
    };
    let (hours, rest) = take_digits(rest, 2)?;
    let (minutes, rest) = match style {
        ZoneStyle::Short | ZoneStyle::IsoShort => (0, rest),
        ZoneStyle::Basic | ZoneStyle::IsoBasic => take_digits(rest, 2)?,
        ZoneStyle::Colon | ZoneStyle::IsoColon => {
            let rest = rest.strip_prefix(':')?;
            take_digits(rest, 2)?
        }
        ZoneStyle::Seconds | ZoneStyle::IsoSeconds => {
            let (minutes, rest) = take_digits(rest, 2)?;
            let (seconds, rest) = take_digits(rest, 2)?;
            if hours > 24 || minutes > 60 || seconds > 60 {
                return None;
            }
            let offset =
                sign * (i64::from(hours) * 3600 + i64::from(minutes) * 60 + i64::from(seconds));
            return i32::try_from(offset).ok().map(|offset| (offset, rest));
        }
        ZoneStyle::SecondsColon | ZoneStyle::IsoSecondsColon => {
            let rest = rest.strip_prefix(':')?;
            let (minutes, rest) = take_digits(rest, 2)?;
            let rest = rest.strip_prefix(':')?;
            let (seconds, rest) = take_digits(rest, 2)?;
            if hours > 24 || minutes > 60 || seconds > 60 {
                return None;
            }
            let offset =
                sign * (i64::from(hours) * 3600 + i64::from(minutes) * 60 + i64::from(seconds));
            return i32::try_from(offset).ok().map(|offset| (offset, rest));
        }
    };
    if hours > 24 || minutes > 60 {
        return None;
    }
    let offset = sign * (i64::from(hours) * 3600 + i64::from(minutes) * 60);
    i32::try_from(offset).ok().map(|offset| (offset, rest))
}

/// Parse `value` according to a Go layout. Zone abbreviations are treated as
/// UTC, matching CUE's `time.Parse`.
#[allow(clippy::question_mark)]
pub(crate) fn parse(layout: &str, value: &str) -> Option<ParsedTime> {
    let items = tokenize(layout);
    let mut rest = value;
    let mut year = 0i32;
    let mut month = 1u32;
    let mut day = 1u32;
    let mut month_seen = false;
    let mut day_seen = false;
    let mut yday: Option<u32> = None;
    let mut hour = 0u32;
    let mut minute = 0u32;
    let mut second = 0u32;
    let mut nanos = 0u32;
    let mut pm: Option<bool> = None;
    let mut offset_seconds = 0i32;
    let mut saw_hour12 = false;
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Literal(text) => {
                rest = skip_literal(rest, text)?;
            }
            Item::Year4 => {
                let (value, next) = take_digits(rest, 4)?;
                year = i32::try_from(value).ok()?;
                rest = next;
            }
            Item::Year2 => {
                let (value, next) = take_digits(rest, 2)?;
                year = i32::try_from(value).ok()?;
                year += if value >= 69 { 1900 } else { 2000 };
                rest = next;
            }
            Item::MonthLong => {
                let (value, next) = parse_name(rest, &MONTHS_LONG)?;
                month = u32::try_from(value + 1).ok()?;
                month_seen = true;
                rest = next;
            }
            Item::MonthShort => {
                let (value, next) = parse_name(rest, &MONTHS_SHORT)?;
                month = u32::try_from(value + 1).ok()?;
                month_seen = true;
                rest = next;
            }
            Item::MonthNum | Item::MonthNum0 => {
                let fixed = matches!(item, Item::MonthNum0);
                let (value, next) = take_number(rest, if fixed { 2 } else { 1 }, 2)?;
                month = value;
                month_seen = true;
                rest = next;
            }
            Item::Day | Item::Day0 | Item::DaySpace => {
                if matches!(item, Item::DaySpace) && rest.starts_with(' ') {
                    rest = &rest[1..];
                }
                let fixed = matches!(item, Item::Day0);
                let (value, next) = take_number(rest, if fixed { 2 } else { 1 }, 2)?;
                day = value;
                day_seen = true;
                rest = next;
            }
            Item::YDay0 => {
                let (value, next) = take_digits(rest, 3)?;
                yday = Some(value);
                rest = next;
            }
            Item::YDaySpace => {
                let mut trimmed = rest;
                for _ in 0..2 {
                    if let Some(stripped) = trimmed.strip_prefix(' ') {
                        trimmed = stripped;
                    }
                }
                let (value, next) = take_number(trimmed, 1, 3)?;
                yday = Some(value);
                rest = next;
            }
            Item::WeekdayLong | Item::WeekdayShort => {
                let names: &[&str] = if matches!(item, Item::WeekdayLong) {
                    &WEEKDAYS_LONG
                } else {
                    &WEEKDAYS_SHORT
                };
                let (_, next) = parse_name(rest, names)?;
                rest = next;
            }
            Item::Hour24 => {
                let (value, next) = take_number(rest, 1, 2)?;
                hour = value;
                rest = next;
            }
            Item::Hour12 | Item::Hour12Zero => {
                let fixed = matches!(item, Item::Hour12Zero);
                let (value, next) = take_number(rest, if fixed { 2 } else { 1 }, 2)?;
                hour = value;
                saw_hour12 = true;
                rest = next;
            }
            Item::Minute | Item::MinuteZero => {
                let fixed = matches!(item, Item::MinuteZero);
                let (value, next) = take_number(rest, if fixed { 2 } else { 1 }, 2)?;
                minute = value;
                rest = next;
            }
            Item::Second | Item::SecondZero => {
                let fixed = matches!(item, Item::SecondZero);
                let (value, next) = take_number(rest, if fixed { 2 } else { 1 }, 2)?;
                second = value;
                rest = next;
                let fraction_follows = rest.starts_with(['.', ',']);
                let next_is_fraction = matches!(items.get(index + 1), Some(Item::Fraction { .. }));
                if fraction_follows && !next_is_fraction {
                    let (value, next) = parse_nanoseconds(rest)?;
                    nanos = value;
                    rest = next;
                }
            }
            Item::Fraction { digits, trim, .. } => {
                if rest.starts_with(['.', ',']) {
                    let (value, next) = parse_fraction(rest, *digits, *trim)?;
                    nanos = value;
                    rest = next;
                } else if !trim {
                    return None;
                }
            }
            Item::PmUpper | Item::PmLower => {
                let upper = matches!(item, Item::PmUpper);
                let (is_pm, next) = if upper {
                    if let Some(next) = rest.strip_prefix("PM") {
                        (true, next)
                    } else if let Some(next) = rest.strip_prefix("AM") {
                        (false, next)
                    } else {
                        return None;
                    }
                } else if let Some(next) = rest.strip_prefix("pm") {
                    (true, next)
                } else if let Some(next) = rest.strip_prefix("am") {
                    (false, next)
                } else {
                    return None;
                };
                pm = Some(is_pm);
                rest = next;
            }
            Item::ZoneName => {
                let consumed = parse_zone_name(rest)?;
                rest = &rest[consumed..];
                offset_seconds = 0;
            }
            Item::ZoneNumeric(style) => {
                let (offset, next) = parse_zone(rest, *style)?;
                offset_seconds = offset;
                rest = next;
            }
        }
    }
    if !rest.is_empty() {
        return None;
    }
    if saw_hour12 && hour > 12 {
        return None;
    }
    let _ = saw_hour12;
    if let Some(pm) = pm {
        if pm {
            if hour < 12 {
                hour += 12;
            }
        } else if hour == 12 {
            hour = 0;
        }
    }
    let date = if let Some(yday) = yday {
        if yday == 0 {
            return None;
        }
        let date = NaiveDate::from_yo_opt(year, yday)?;
        if (month_seen && date.month() != month) || (day_seen && date.day() != day) {
            return None;
        }
        date
    } else {
        NaiveDate::from_ymd_opt(year, month, day)?
    };
    let time = NaiveTime::from_hms_nano_opt(hour, minute, second, nanos)?;
    Some(ParsedTime {
        local: date.and_time(time),
        offset_seconds,
    })
}

fn format_fraction(value: u64, precision: usize) -> (String, u64) {
    let mut digits = Vec::with_capacity(precision);
    let mut remaining = value;
    let mut printed = false;
    for _ in 0..precision {
        let digit = remaining % 10;
        printed |= digit != 0;
        if printed {
            digits.push(char::from(b'0' + u8::try_from(digit).unwrap_or(0)));
        }
        remaining /= 10;
    }
    let mut fraction = String::new();
    if printed {
        fraction.push('.');
        for digit in digits.into_iter().rev() {
            fraction.push(digit);
        }
    }
    (fraction, remaining)
}

fn format_zone(offset_seconds: i32, style: ZoneStyle) -> String {
    let iso = style.iso();
    if iso && offset_seconds == 0 {
        return "Z".to_owned();
    }
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let total = offset_seconds.unsigned_abs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    match style {
        ZoneStyle::Short | ZoneStyle::IsoShort => format!("{sign}{hours:02}"),
        ZoneStyle::Basic | ZoneStyle::IsoBasic => format!("{sign}{hours:02}{minutes:02}"),
        ZoneStyle::Colon | ZoneStyle::IsoColon => {
            format!("{sign}{hours:02}:{minutes:02}")
        }
        ZoneStyle::Seconds | ZoneStyle::IsoSeconds => {
            format!("{sign}{hours:02}{minutes:02}{seconds:02}")
        }
        ZoneStyle::SecondsColon | ZoneStyle::IsoSecondsColon => {
            format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
        }
    }
}

/// Format a time value according to a Go layout.
#[must_use]
pub(crate) fn format(layout: &str, value: &ParsedTime) -> String {
    let items = tokenize(layout);
    let mut out = String::new();
    let offset_seconds = value.offset_seconds;
    let value = &value.local;
    for item in items {
        match item {
            Item::Literal(text) => out.push_str(&text),
            Item::Year4 => {
                let year = value.year();
                if year < 0 {
                    out.push('-');
                    out.push_str(&format!("{:04}", -year));
                } else {
                    out.push_str(&format!("{year:04}"));
                }
            }
            Item::Year2 => out.push_str(&format!("{:02}", value.year().rem_euclid(100))),
            Item::MonthLong => out.push_str(MONTHS_LONG[(value.month0() as usize).min(11)]),
            Item::MonthShort => out.push_str(MONTHS_SHORT[(value.month0() as usize).min(11)]),
            Item::MonthNum => out.push_str(&value.month().to_string()),
            Item::MonthNum0 => out.push_str(&format!("{:02}", value.month())),
            Item::Day => out.push_str(&value.day().to_string()),
            Item::Day0 => out.push_str(&format!("{:02}", value.day())),
            Item::DaySpace => out.push_str(&format!("{:2}", value.day())),
            Item::YDay0 => out.push_str(&format!("{:03}", value.ordinal())),
            Item::YDaySpace => out.push_str(&format!("{:3}", value.ordinal())),
            Item::WeekdayLong => {
                out.push_str(WEEKDAYS_LONG[value.weekday().num_days_from_sunday() as usize]);
            }
            Item::WeekdayShort => {
                out.push_str(WEEKDAYS_SHORT[value.weekday().num_days_from_sunday() as usize]);
            }
            Item::Hour24 => out.push_str(&format!("{:02}", value.hour())),
            Item::Hour12 => {
                let hour = value.hour() % 12;
                out.push_str(&(if hour == 0 { 12 } else { hour }).to_string());
            }
            Item::Hour12Zero => {
                let hour = value.hour() % 12;
                out.push_str(&format!("{:02}", if hour == 0 { 12 } else { hour }));
            }
            Item::Minute => out.push_str(&value.minute().to_string()),
            Item::MinuteZero => out.push_str(&format!("{:02}", value.minute())),
            Item::Second => out.push_str(&value.second().to_string()),
            Item::SecondZero => out.push_str(&format!("{:02}", value.second())),
            Item::Fraction {
                digits,
                trim,
                separator,
            } => {
                let text = format!("{:09}", value.nanosecond());
                let take = digits.min(9);
                if trim {
                    let trimmed = text[..take].trim_end_matches('0');
                    if !trimmed.is_empty() {
                        out.push(separator);
                        out.push_str(trimmed);
                    }
                } else {
                    out.push(separator);
                    out.push_str(&text[..take]);
                }
            }
            Item::PmUpper => out.push_str(if value.hour() < 12 { "AM" } else { "PM" }),
            Item::PmLower => out.push_str(if value.hour() < 12 { "am" } else { "pm" }),
            Item::ZoneName => {
                if offset_seconds == 0 {
                    out.push_str("UTC");
                } else {
                    out.push_str(&format_zone(offset_seconds, ZoneStyle::Basic));
                }
            }
            Item::ZoneNumeric(style) => out.push_str(&format_zone(offset_seconds, style)),
        }
    }
    out
}

/// Format an instant as RFC3339Nano (UTC) as CUE does for time values.
#[must_use]
pub(crate) fn to_rfc3339_nano(value: &DateTime<Utc>) -> String {
    let year = value.year();
    let mut out = String::new();
    if year < 0 {
        out.push('-');
        out.push_str(&format!("{:04}", -year));
    } else {
        out.push_str(&format!("{year:04}"));
    }
    out.push_str(&format!(
        "-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    ));
    let (fraction, _) = format_fraction(u64::from(value.nanosecond()), 9);
    out.push_str(&fraction);
    out.push('Z');
    out
}

/// Parse an RFC3339Nano CUE time value.
pub(crate) fn parse_rfc3339_nano(value: &str) -> Option<ParsedTime> {
    parse("2006-01-02T15:04:05.999999999Z07:00", value)
}

/// Parse an RFC3339Nano value and return its UTC instant.
/// Format nanoseconds like Go's `time.Duration.String`.
#[must_use]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub(crate) fn format_duration(nanos: i64) -> String {
    if nanos == 0 {
        return "0s".to_owned();
    }
    let negative = nanos < 0;
    let value = nanos.unsigned_abs();
    let mut out = if value < 1_000_000_000 {
        let (precision, unit) = if value < 1_000 {
            (0usize, "ns")
        } else if value < 1_000_000 {
            (3, "µs")
        } else {
            (6, "ms")
        };
        let (fraction, integer) = format_fraction(value, precision);
        format!("{integer}{fraction}{unit}")
    } else {
        let fraction = value % 1_000_000_000;
        let seconds = value / 1_000_000_000;
        let (fraction, _) = format_fraction(fraction, 9);
        let second = seconds % 60;
        let mut minutes = seconds / 60;
        let mut result = format!("{second}{fraction}s");
        if minutes > 0 {
            let minute = minutes % 60;
            minutes /= 60;
            result = format!("{minute}m{result}");
        }
        if minutes > 0 {
            result = format!("{minutes}h{result}");
        }
        result
    };
    if negative {
        out.insert(0, '-');
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_go_layouts() {
        let value = parse("Mon Jan _2 15:04:05 2006", "Tue Mar  5 06:07:08 2024").expect("ANSIC");
        assert_eq!(to_rfc3339_nano(&value.utc()), "2024-03-05T06:07:08Z");

        let value = parse(
            "2006-01-02T15:04:05.999999999Z07:00",
            "2024-03-05T06:07:08.12+02:00",
        )
        .expect("RFC3339Nano");
        assert_eq!(to_rfc3339_nano(&value.utc()), "2024-03-05T04:07:08.12Z");

        let value = parse("2006-002", "2024-065").expect("year day");
        assert_eq!(to_rfc3339_nano(&value.utc()), "2024-03-05T00:00:00Z");
    }

    #[test]
    fn formats_go_layouts() {
        let value = parse_rfc3339_nano("2024-03-05T06:07:08Z").expect("time");
        assert_eq!(format("Monday", &value), "Tuesday");
        assert_eq!(
            format("Mon Jan _2 15:04:05 2006", &value),
            "Tue Mar  5 06:07:08 2024"
        );
        assert_eq!(
            format("2006-01-02T15:04:05.999999999Z07:00", &value),
            "2024-03-05T06:07:08Z"
        );
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(1_200_000), "1.2ms");
        assert_eq!(format_duration(3_723_000_500_000), "1h2m3.0005s");
    }
}
