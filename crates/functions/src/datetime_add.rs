use ir::Value;

use crate::{FunctionError, datetime};

const FUNCTION: &str = "datetime_add";
const INVALID: &str =
    "requires an xs:date or xs:dateTime followed by one or more xs:duration values";
// Fixed-width arithmetic stays exact through this precision. Longer
// significant fractions are rejected instead of rounded.
const MAX_FRACTION_DIGITS: usize = 18;

#[derive(Clone, Copy)]
enum TemporalKind {
    Date,
    DateTime,
}

struct DateTime {
    kind: TemporalKind,
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    fraction: i128,
    fraction_digits: usize,
    timezone: Option<String>,
}

#[derive(Default)]
struct Duration {
    months: i128,
    seconds: i128,
    fraction: i128,
    fraction_digits: usize,
    negative: bool,
}

pub(super) fn datetime_add(args: &[Value]) -> Result<Value, FunctionError> {
    if args.len() < 2 {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 2,
            got: args.len(),
        });
    }
    let mut values = args.iter();
    let datetime = values.next().ok_or_else(invalid)?;
    let Value::String(datetime) = datetime else {
        return type_mismatch(datetime);
    };
    let mut result = DateTime::parse(datetime)?;
    let mut saw_duration = false;
    for duration in values {
        if matches!(duration, Value::Null) {
            continue;
        }
        let Value::String(duration) = duration else {
            return type_mismatch(duration);
        };
        saw_duration = true;
        result.add(Duration::parse(duration)?)?;
    }
    if !saw_duration {
        return Err(invalid());
    }
    Ok(Value::String(result.render()))
}

impl DateTime {
    fn parse(value: &str) -> Result<Self, FunctionError> {
        let (kind, date, time, timezone) = if let Some((date, time)) = value.split_once('T') {
            let (time, timezone) = datetime::split_iso_timezone(time, FUNCTION)?;
            (TemporalKind::DateTime, date, time, timezone)
        } else {
            let (date, timezone) = datetime::split_iso_timezone(value, FUNCTION)?;
            (TemporalKind::Date, date, "00:00:00", timezone)
        };
        datetime::validate_iso_date(date, FUNCTION)?;
        let end_of_day = matches!(kind, TemporalKind::DateTime) && time.starts_with("24:");
        if end_of_day {
            let normalized = format!("00{}", &time[2..]);
            datetime::validate_iso_time(&normalized, FUNCTION)?;
            let (_, minute_second) = time.split_once(':').ok_or_else(invalid)?;
            let (minute, second) = minute_second.split_once(':').ok_or_else(invalid)?;
            let (second, fraction) = second
                .split_once('.')
                .map_or((second, ""), |(second, fraction)| (second, fraction));
            if minute != "00" || second != "00" || fraction.bytes().any(|digit| digit != b'0') {
                return Err(invalid());
            }
        } else {
            datetime::validate_iso_time(time, FUNCTION)?;
        }

        let year_end = date.len().checked_sub(6).ok_or_else(invalid)?;
        let year = date[..year_end].parse().map_err(|_| invalid())?;
        let month = date[year_end + 1..date.len() - 3]
            .parse()
            .map_err(|_| invalid())?;
        let day = date[date.len() - 2..].parse().map_err(|_| invalid())?;
        let (whole, fraction) = time
            .split_once('.')
            .map_or((time, ""), |(whole, fraction)| (whole, fraction));
        let hour = whole[..2].parse().map_err(|_| invalid())?;
        let minute = whole[3..5].parse().map_err(|_| invalid())?;
        let second = whole[6..].parse().map_err(|_| invalid())?;
        let fraction = fraction.trim_end_matches('0');
        if fraction.len() > MAX_FRACTION_DIGITS {
            return Err(invalid());
        }
        Ok(Self {
            kind,
            year,
            month,
            day,
            hour,
            minute,
            second,
            fraction: parse_digits(fraction)?,
            fraction_digits: fraction.len(),
            timezone: timezone.map(str::to_string),
        })
    }

    fn add(&mut self, duration: Duration) -> Result<(), FunctionError> {
        let signed_months = if duration.negative {
            duration.months.checked_neg()
        } else {
            Some(duration.months)
        }
        .ok_or_else(invalid)?;
        self.add_months(signed_months)?;

        let digits = self.fraction_digits.max(duration.fraction_digits);
        let scale = power_of_ten(digits)?;
        let current_scale = power_of_ten(self.fraction_digits)?;
        let duration_scale = power_of_ten(duration.fraction_digits)?;
        let ordinal = date_ordinal(self.year, self.month, self.day)?;
        let seconds = ordinal
            .checked_mul(86_400)
            .and_then(|value| value.checked_add(i128::from(self.hour) * 3_600))
            .and_then(|value| value.checked_add(i128::from(self.minute) * 60))
            .and_then(|value| value.checked_add(i128::from(self.second)))
            .ok_or_else(invalid)?;
        let base = seconds
            .checked_mul(scale)
            .and_then(|value| value.checked_add(self.fraction.checked_mul(scale / current_scale)?))
            .ok_or_else(invalid)?;
        let delta = duration
            .seconds
            .checked_mul(scale)
            .and_then(|value| {
                value.checked_add(duration.fraction.checked_mul(scale / duration_scale)?)
            })
            .and_then(|value| {
                if duration.negative {
                    value.checked_neg()
                } else {
                    Some(value)
                }
            })
            .ok_or_else(invalid)?;
        let total = base.checked_add(delta).ok_or_else(invalid)?;
        let day_units = scale.checked_mul(86_400).ok_or_else(invalid)?;
        let ordinal = total.div_euclid(day_units);
        let within_day = total.rem_euclid(day_units);
        let whole_seconds = within_day / scale;
        let (year, month, day) = date_from_ordinal(ordinal)?;
        self.year = year;
        self.month = month;
        self.day = day;
        self.hour = u32::try_from(whole_seconds / 3_600).map_err(|_| invalid())?;
        self.minute = u32::try_from(whole_seconds % 3_600 / 60).map_err(|_| invalid())?;
        self.second = u32::try_from(whole_seconds % 60).map_err(|_| invalid())?;
        self.fraction = within_day % scale;
        self.fraction_digits = digits;
        Ok(())
    }

    fn add_months(&mut self, months: i128) -> Result<(), FunctionError> {
        let astronomical_year = if self.year < 0 {
            i128::from(self.year) + 1
        } else {
            i128::from(self.year)
        };
        let month_index = astronomical_year
            .checked_mul(12)
            .and_then(|value| value.checked_add(i128::from(self.month) - 1))
            .and_then(|value| value.checked_add(months))
            .ok_or_else(invalid)?;
        let astronomical_year = month_index.div_euclid(12);
        let year = if astronomical_year <= 0 {
            astronomical_year - 1
        } else {
            astronomical_year
        };
        self.year = i64::try_from(year).map_err(|_| invalid())?;
        self.month = u32::try_from(month_index.rem_euclid(12) + 1).map_err(|_| invalid())?;
        self.day = self.day.min(days_in_month(self.year, self.month));
        Ok(())
    }

    fn render(&self) -> String {
        let year = if self.year < 0 {
            format!("-{:04}", self.year.unsigned_abs())
        } else {
            format!("{:04}", self.year)
        };
        let mut output = format!("{year}-{:02}-{:02}", self.month, self.day);
        if matches!(self.kind, TemporalKind::DateTime) {
            output.push_str(&format!(
                "T{:02}:{:02}:{:02}",
                self.hour, self.minute, self.second
            ));
            if self.fraction != 0 {
                let fraction = format!("{:0width$}", self.fraction, width = self.fraction_digits);
                output.push('.');
                output.push_str(fraction.trim_end_matches('0'));
            }
        }
        if let Some(timezone) = &self.timezone {
            output.push_str(timezone);
        }
        output
    }
}

impl Duration {
    fn parse(value: &str) -> Result<Self, FunctionError> {
        let (negative, value) = value
            .strip_prefix('-')
            .map_or((false, value), |value| (true, value));
        let bytes = value.strip_prefix('P').ok_or_else(invalid)?.as_bytes();
        if bytes.is_empty() {
            return Err(invalid());
        }

        let mut result = Self {
            negative,
            ..Self::default()
        };
        let mut index = 0;
        let mut in_time = false;
        let mut last_rank = 0;
        let mut saw_value = false;
        let mut time_values = 0;
        while index < bytes.len() {
            if bytes[index] == b'T' {
                if in_time || !saw_value && index != 0 {
                    return Err(invalid());
                }
                in_time = true;
                index += 1;
                last_rank = 3;
                continue;
            }
            let start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if start == index {
                return Err(invalid());
            }
            let whole =
                parse_digits(std::str::from_utf8(&bytes[start..index]).map_err(|_| invalid())?)?;
            let mut fraction = "";
            if bytes.get(index) == Some(&b'.') {
                index += 1;
                let fraction_start = index;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                }
                if fraction_start == index {
                    return Err(invalid());
                }
                fraction =
                    std::str::from_utf8(&bytes[fraction_start..index]).map_err(|_| invalid())?;
            }
            let designator = *bytes.get(index).ok_or_else(invalid)?;
            index += 1;
            let rank = match (in_time, designator) {
                (false, b'Y') => 1,
                (false, b'M') => 2,
                (false, b'D') => 3,
                (true, b'H') => 4,
                (true, b'M') => 5,
                (true, b'S') => 6,
                _ => return Err(invalid()),
            };
            if rank <= last_rank || !fraction.is_empty() && designator != b'S' {
                return Err(invalid());
            }
            last_rank = rank;
            saw_value = true;
            if in_time {
                time_values += 1;
            }
            result.set(rank, whole, fraction)?;
        }
        if !saw_value || in_time && time_values == 0 {
            return Err(invalid());
        }
        Ok(result)
    }

    fn set(&mut self, rank: u8, whole: i128, fraction: &str) -> Result<(), FunctionError> {
        match rank {
            1 => self.months = whole.checked_mul(12).ok_or_else(invalid)?,
            2 => self.months = self.months.checked_add(whole).ok_or_else(invalid)?,
            3 => self.seconds = whole.checked_mul(86_400).ok_or_else(invalid)?,
            4 => {
                self.seconds = self
                    .seconds
                    .checked_add(whole.checked_mul(3_600).ok_or_else(invalid)?)
                    .ok_or_else(invalid)?;
            }
            5 => {
                self.seconds = self
                    .seconds
                    .checked_add(whole.checked_mul(60).ok_or_else(invalid)?)
                    .ok_or_else(invalid)?;
            }
            6 => {
                self.seconds = self.seconds.checked_add(whole).ok_or_else(invalid)?;
                let fraction = fraction.trim_end_matches('0');
                if fraction.len() > MAX_FRACTION_DIGITS {
                    return Err(invalid());
                }
                self.fraction = parse_digits(fraction)?;
                self.fraction_digits = fraction.len();
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
}

fn date_ordinal(year: i64, month: u32, day: u32) -> Result<i128, FunctionError> {
    let before_year = if year > 0 {
        let years = i128::from(year - 1);
        365 * years + years / 4 - years / 100 + years / 400
    } else {
        let years = i128::from(year).checked_neg().ok_or_else(invalid)?;
        -(365 * years + years / 4 - years / 100 + years / 400)
    };
    let before_month: i128 = (1..month)
        .map(|month| i128::from(days_in_month(year, month)))
        .sum();
    before_year
        .checked_add(before_month)
        .and_then(|value| value.checked_add(i128::from(day) - 1))
        .ok_or_else(invalid)
}

fn date_from_ordinal(ordinal: i128) -> Result<(i64, u32, u32), FunctionError> {
    let mut low = i64::MIN;
    let mut high = i64::MAX;
    while low < high {
        let middle = i64::try_from((i128::from(low) + i128::from(high) + 1).div_euclid(2))
            .map_err(|_| invalid())?;
        if date_ordinal(middle, 1, 1)? <= ordinal {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    if low == 0 {
        return Err(invalid());
    }
    let mut remaining = ordinal - date_ordinal(low, 1, 1)?;
    for month in 1..=12 {
        let days = i128::from(days_in_month(low, month));
        if remaining < days {
            return Ok((
                low,
                month,
                u32::try_from(remaining + 1).map_err(|_| invalid())?,
            ));
        }
        remaining -= days;
    }
    Err(invalid())
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || year % 4 == 0 && year % 100 != 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn parse_digits(value: &str) -> Result<i128, FunctionError> {
    if value.is_empty() {
        return Ok(0);
    }
    value.parse().map_err(|_| invalid())
}

fn power_of_ten(digits: usize) -> Result<i128, FunctionError> {
    10_i128
        .checked_pow(u32::try_from(digits).map_err(|_| invalid())?)
        .ok_or_else(invalid)
}

fn type_mismatch<T>(value: &Value) -> Result<T, FunctionError> {
    Err(FunctionError::TypeMismatch {
        function: FUNCTION,
        got: value.type_name(),
    })
}

fn invalid() -> FunctionError {
    FunctionError::InvalidArgument {
        function: FUNCTION,
        message: INVALID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Value {
        Value::String(value.to_string())
    }

    #[test]
    fn adds_calendar_months_before_day_time() {
        assert_eq!(
            datetime_add(&[text("2024-01-31T23:30:00.25Z"), text("P1M1DT1H30M0.75S")]).unwrap(),
            text("2024-03-02T01:00:01Z")
        );
        assert_eq!(
            datetime_add(&[text("2023-01-31T00:00:00"), text("P1M")]).unwrap(),
            text("2023-02-28T00:00:00")
        );
    }

    #[test]
    fn adds_durations_to_dates_without_changing_the_lexical_kind() {
        assert_eq!(
            datetime_add(&[text("2019-02-01"), text("P1M"), text("-P1D")]).unwrap(),
            text("2019-02-28")
        );
        assert_eq!(
            datetime_add(&[text("2024-02-01+05:30"), text("P1M"), text("-P1D")]).unwrap(),
            text("2024-02-29+05:30")
        );
        assert_eq!(
            datetime_add(&[text("2024-01-31Z"), text("PT25H")]).unwrap(),
            text("2024-02-01Z")
        );
    }

    #[test]
    fn adds_negative_and_growable_durations_exactly() {
        assert_eq!(
            datetime_add(&[
                text("2024-03-01T00:00:00.000000000000000001+05:30"),
                text("-PT0.000000000000000002S"),
                text("PT1S"),
            ])
            .unwrap(),
            text("2024-03-01T00:00:00.999999999999999999+05:30")
        );
        assert_eq!(
            datetime_add(&[text("0001-01-01T00:00:00"), text("-P1D")]).unwrap(),
            text("-0001-12-31T00:00:00")
        );
    }

    #[test]
    fn rejects_invalid_duration_and_overflow() {
        for duration in ["P", "PT", "P1S", "P1M1Y", "PT1.2M", "+P1D"] {
            assert!(datetime_add(&[text("2024-01-01T00:00:00"), text(duration)]).is_err());
        }
        assert!(
            datetime_add(&[
                text("2024-01-01T00:00:00"),
                text("P999999999999999999999999999999999999999Y"),
            ])
            .is_err()
        );
    }

    #[test]
    fn normalizes_end_of_day_and_rejects_nonzero_24_hour_parts() {
        assert_eq!(
            datetime_add(&[text("2024-01-31T24:00:00.000Z"), text("P0D")]).unwrap(),
            text("2024-02-01T00:00:00Z")
        );
        for value in [
            "2024-01-31T24:01:00Z",
            "2024-01-31T24:00:01Z",
            "2024-01-31T24:00:00.1Z",
        ] {
            assert!(datetime_add(&[text(value), text("P0D")]).is_err());
        }
    }

    #[test]
    fn skips_null_growable_holes_but_requires_a_duration() {
        assert_eq!(
            datetime_add(&[
                text("2024-01-01T00:00:00"),
                text("P1D"),
                Value::Null,
                text("P1D"),
            ])
            .unwrap(),
            text("2024-01-03T00:00:00")
        );
        assert!(datetime_add(&[text("2024-01-01T00:00:00"), Value::Null]).is_err());
    }

    #[test]
    fn preserves_timezone_boundaries_and_xsd_1_0_bce_calendar() {
        for zone in ["+14:00", "-14:00", "+00:00", "-00:00"] {
            assert_eq!(
                datetime_add(&[text(&format!("2024-02-28T12:00:00{zone}")), text("P1D")]).unwrap(),
                text(&format!("2024-02-29T12:00:00{zone}"))
            );
        }
        assert_eq!(
            datetime_add(&[text("0001-01-31T00:00:00"), text("-P1M")]).unwrap(),
            text("-0001-12-31T00:00:00")
        );
        assert_eq!(
            datetime_add(&[text("-0004-02-28T00:00:00"), text("P1D")]).unwrap(),
            text("-0004-02-29T00:00:00")
        );
        assert!(datetime_add(&[text("0000-01-01T00:00:00"), text("P0D")]).is_err());
    }

    #[test]
    fn growable_month_durations_are_applied_sequentially() {
        assert_eq!(
            datetime_add(&[text("2023-01-31T00:00:00"), text("P1M"), text("-P1M"),]).unwrap(),
            text("2023-01-28T00:00:00")
        );
    }

    #[test]
    fn rejects_fractions_beyond_the_exact_fixed_width_bound() {
        assert!(
            datetime_add(&[text("2024-01-01T00:00:00.0000000000000000001"), text("P0D"),]).is_err()
        );
        assert!(
            datetime_add(&[
                text("2024-01-01T00:00:00"),
                text("PT0.0000000000000000001S"),
            ])
            .is_err()
        );
    }
}
