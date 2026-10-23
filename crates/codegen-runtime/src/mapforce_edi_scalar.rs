//! MapForce's X12 field formatting, not the strict native X12 value policy.
//! Formatting does not escape syntax, encode bytes, or perform any file I/O.

use ir::{Decimal96, Value};
use mapping::{MapForceEdiScalar, MapForceEdiScalarKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapForceEdiScalarError {
    Descriptor,
    Value,
    Decimal(ir::DecimalError),
    Limit,
}

impl std::fmt::Display for MapForceEdiScalarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapForce EDI scalar: {self:?}")
    }
}
impl std::error::Error for MapForceEdiScalarError {}

/// Missing and empty values do not receive padding. Text is padded by UTF-16
/// code-unit length but is not truncated to the declared maximum. Decimal
/// precision and time fractions use the original writer's distinct policies.
pub fn format_mapforce_edi_scalar(
    field: MapForceEdiScalar,
    value: &Value,
) -> Result<String, MapForceEdiScalarError> {
    if field.min_chars > field.max_chars || field.max_chars > 65536 {
        return Err(MapForceEdiScalarError::Descriptor);
    }
    let text = match value {
        Value::Null | Value::JsonNull(_) => return Ok(String::new()),
        Value::String(text) => text.clone(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) if value.is_finite() => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        _ => return Err(MapForceEdiScalarError::Value),
    };
    if text.len() > 64 * 1024 * 1024 {
        return Err(MapForceEdiScalarError::Limit);
    }
    if text.is_empty() {
        return Ok(text);
    }
    let min = field.min_chars as usize;
    let max = field.max_chars as usize;
    match field.kind {
        MapForceEdiScalarKind::Text => {
            let length = text.encode_utf16().count();
            Ok(text + &" ".repeat(min.saturating_sub(length)))
        }
        MapForceEdiScalarKind::Decimal => decimal(&text, min, max),
        MapForceEdiScalarKind::Date => {
            let text = text.trim();
            if !text.is_ascii() {
                return Err(MapForceEdiScalarError::Value);
            }
            let mut text = if text.len() >= 10 {
                format!("{}{}{}", &text[..4], &text[5..7], &text[8..10])
            } else {
                text.to_string()
            };
            if text.len() != 6 && text.len() != 8 {
                return Err(MapForceEdiScalarError::Value);
            }
            if max == 6 && text.len() == 8 {
                text = text[2..].to_string();
            }
            let padding = min.saturating_sub(text.len());
            Ok(text + &" ".repeat(padding))
        }
        MapForceEdiScalarKind::Time => time(&text, min, max),
    }
}

fn decimal(text: &str, min: usize, max: usize) -> Result<String, MapForceEdiScalarError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let mut number = Decimal96::parse_rounded(unsigned).map_err(MapForceEdiScalarError::Decimal)?;
    let initial = number.to_string();
    if let Some(point) = initial.find('.')
        && initial.len() - 1 > max
    {
        let places = max.saturating_add(1).saturating_sub(point).min(28);
        number = round_scale(number, places as u8)?;
    }
    let rendered = number.to_string();
    let (whole, fraction) = rendered.split_once('.').unwrap_or((&rendered, ""));
    let whole = whole.trim_start_matches('0');
    // X12 omits a zero integer part only for nonzero fractional values.
    let whole = if whole.is_empty() && fraction.is_empty() {
        "0"
    } else {
        whole
    };
    let fraction = if fraction.is_empty() {
        String::new()
    } else {
        format!(".{fraction}")
    };
    let padding = min.saturating_sub(whole.len() + fraction.len());
    Ok(format!(
        "{}{}{whole}{fraction}",
        if negative { "-" } else { "" },
        "0".repeat(padding)
    ))
}

fn round_scale(value: Decimal96, scale: u8) -> Result<Decimal96, MapForceEdiScalarError> {
    if value.scale() <= scale {
        return Ok(value);
    }
    let divisor = 10_u128.pow(u32::from(value.scale() - scale));
    let coefficient = value.coefficient();
    let quotient = coefficient / divisor;
    let remainder = coefficient % divisor;
    let increment = remainder * 2 > divisor || remainder * 2 == divisor && quotient % 2 == 1;
    Decimal96::from_parts(false, quotient + u128::from(increment), scale)
        .map_err(MapForceEdiScalarError::Decimal)
}

fn time(text: &str, min: usize, max: usize) -> Result<String, MapForceEdiScalarError> {
    if !(4..=8).contains(&max) {
        return Err(MapForceEdiScalarError::Descriptor);
    }
    let text = text.split_once('T').map_or(text, |(_, time)| time);
    let mut compact = text.replace([':', '.'], "");
    if let Some(position) = compact.find(['Z', '-', '+']) {
        compact.truncate(position);
    }
    if !compact.is_ascii() {
        return Err(MapForceEdiScalarError::Value);
    }
    if compact.len() > max {
        if max > 6 {
            let fraction = format!("0.{}", &compact[6..])
                .parse::<f64>()
                .map_err(|_| MapForceEdiScalarError::Value)?;
            compact.truncate(6);
            if fraction > 0.0 {
                let places = max - 6;
                let power = 10_u64.pow(places as u32);
                let digits = (fraction * power as f64).round_ties_even() as u64 % power;
                compact.push_str(&format!("{digits:0places$}"));
            }
        } else {
            compact.truncate(max);
        }
    }
    while compact.len() > min && compact.len() > 6 && compact.ends_with('0') {
        compact.pop();
    }
    Ok(compact)
}
