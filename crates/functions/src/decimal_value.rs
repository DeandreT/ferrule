use ir::{Decimal96, Value};

use crate::FunctionError;

pub(crate) fn from_nullable_text(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "decimal_from_nullable_text";
    match args {
        [Value::Null | Value::JsonNull(_)] => Ok(Value::decimal(Decimal96::ZERO)),
        [Value::String(text)] => {
            Decimal96::parse_number(text)
                .map(Value::decimal)
                .map_err(|source| FunctionError::DecimalConversion {
                    function: FUNCTION,
                    source,
                })
        }
        [value] => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: value.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 1,
            got: args.len(),
        }),
    }
}

pub(crate) fn truncate(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "decimal_truncate";
    match args {
        [Value::Decimal(value)] => Decimal96::from_parts(
            value.is_negative(),
            value.coefficient() / 10_u128.pow(u32::from(value.scale())),
            0,
        )
        .map(Value::decimal)
        .map_err(|source| FunctionError::DecimalConversion {
            function: FUNCTION,
            source,
        }),
        [value] => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: value.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 1,
            got: args.len(),
        }),
    }
}

/// The native integer-source contract rounds the bounded Decimal quotient
/// before applying floor. Exact rational floor and binary division differ.
pub(crate) fn mapforce_integer_quotient_floor(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "mapforce_integer_quotient_floor";
    if args.len() != 2 {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 2,
            got: args.len(),
        });
    }
    let integer = |value: &Value| match value {
        Value::Decimal(value) => Ok((
            value.is_negative(),
            value.coefficient() / 10_u128.pow(u32::from(value.scale())),
        )),
        value => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: value.type_name(),
        }),
    };
    let (left_negative, left) = integer(&args[0])?;
    let (right_negative, right) = integer(&args[1])?;
    if right == 0 {
        return Err(FunctionError::DivideByZero);
    }
    let negative = left_negative != right_negative;
    let mut coefficient = left / right;
    let mut remainder = left % right;
    let mut scale = 0_u32;
    while remainder != 0 && scale < u32::from(ir::MAX_DECIMAL_SCALE) {
        // All intermediates fit in 100 bits: each operand is at most 96 bits.
        let next_remainder = remainder * 10;
        let next = coefficient * 10 + next_remainder / right;
        if next > ir::MAX_DECIMAL_COEFFICIENT {
            break;
        }
        coefficient = next;
        remainder = next_remainder % right;
        scale += 1;
    }
    let round_up =
        remainder * 2 > right || remainder * 2 == right && !coefficient.is_multiple_of(2);
    let mut rounded = coefficient + u128::from(round_up);
    if rounded > ir::MAX_DECIMAL_COEFFICIENT {
        if scale == 0 {
            return Err(FunctionError::DecimalConversion {
                function: FUNCTION,
                source: ir::DecimalError::Overflow,
            });
        }
        let digit = coefficient % 10;
        coefficient /= 10;
        // Re-round the original quotient, not the already rounded coefficient.
        rounded = coefficient
            + u128::from(
                digit > 5 || digit == 5 && (remainder != 0 || !coefficient.is_multiple_of(2)),
            );
        scale -= 1;
    }
    let unit = 10_u128.pow(scale);
    let whole = rounded / unit + u128::from(negative && !rounded.is_multiple_of(unit));
    Decimal96::from_parts(negative, whole, 0)
        .map(Value::decimal)
        .map_err(|source| FunctionError::DecimalConversion {
            function: FUNCTION,
            source,
        })
}
