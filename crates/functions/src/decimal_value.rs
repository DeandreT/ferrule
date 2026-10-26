use ir::{Decimal96, Value};

use crate::FunctionError;

pub(crate) fn from_nullable_text(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "decimal_from_nullable_text";
    match args {
        [Value::Null | Value::JsonNull(_)] => Ok(Value::decimal(Decimal96::ZERO)),
        [Value::String(text)] => Decimal96::parse_decimal_text(text)
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
