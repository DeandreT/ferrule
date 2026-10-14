use anyhow::{Context, bail};

const EXPECTED_CLOCK: &str = "payload current dateTime must use YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM), with 1–9 fractional digits and an offset no greater than ±14:00";

pub(super) fn validate_current_datetime(value: &str) -> anyhow::Result<()> {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len()) || !bytes.is_ascii() {
        bail!(EXPECTED_CLOCK);
    }
    if !bytes[..19]
        .iter()
        .enumerate()
        .all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        })
    {
        bail!(EXPECTED_CLOCK);
    }

    let offset_start = if bytes.last() == Some(&b'Z') {
        bytes.len() - 1
    } else {
        let start = bytes.len() - 6;
        let offset = &bytes[start..];
        if !matches!(offset[0], b'+' | b'-')
            || offset[3] != b':'
            || !offset[1..3].iter().all(u8::is_ascii_digit)
            || !offset[4..6].iter().all(u8::is_ascii_digit)
        {
            bail!(EXPECTED_CLOCK);
        }
        let hours = decimal(&offset[1..3]);
        let minutes = decimal(&offset[4..6]);
        if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
            bail!(EXPECTED_CLOCK);
        }
        start
    };
    if offset_start < 19 {
        bail!(EXPECTED_CLOCK);
    }
    let fraction = &bytes[19..offset_start];
    let nanoseconds = if fraction.is_empty() {
        0
    } else {
        if !(2..=10).contains(&fraction.len())
            || fraction[0] != b'.'
            || !fraction[1..].iter().all(u8::is_ascii_digit)
        {
            bail!(EXPECTED_CLOCK);
        }
        decimal(&fraction[1..]) * 10_i32.pow(10 - fraction.len() as u32)
    };

    let year = decimal(&bytes[..4]);
    if year == 0 {
        bail!("payload current dateTime year must be 0001–9999");
    }
    // Validate civil components without imposing an instant-range restriction
    // when a boundary year is combined with an otherwise valid offset.
    jiff::civil::DateTime::new(
        year as i16,
        decimal(&bytes[5..7]) as i8,
        decimal(&bytes[8..10]) as i8,
        decimal(&bytes[11..13]) as i8,
        decimal(&bytes[14..16]) as i8,
        decimal(&bytes[17..19]) as i8,
        nanoseconds,
    )
    .context("payload current dateTime has invalid Gregorian calendar or time components")?;
    Ok(())
}

fn decimal(digits: &[u8]) -> i32 {
    digits
        .iter()
        .fold(0, |value, digit| value * 10 + i32::from(digit - b'0'))
}
