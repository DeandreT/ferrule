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
