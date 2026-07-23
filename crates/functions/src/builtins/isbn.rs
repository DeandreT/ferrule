use ir::Value;

use crate::FunctionError;

/// Converts a validated ISBN-10 into its equivalent Bookland ISBN-13/EAN-13.
/// ASCII spaces and hyphens are accepted as presentation separators.
pub(super) fn isbn10_to_isbn13(args: &[Value]) -> Result<Value, FunctionError> {
    let [Value::String(input)] = args else {
        return match args {
            [_] => Err(FunctionError::TypeMismatch {
                function: "isbn10_to_isbn13",
                got: args[0].type_name(),
            }),
            _ => Err(FunctionError::ArityMismatch {
                function: "isbn10_to_isbn13",
                expected: 1,
                got: args.len(),
            }),
        };
    };
    let normalized = input
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'-'))
        .collect::<Vec<_>>();
    if normalized.len() != 10
        || !normalized[..9].iter().all(u8::is_ascii_digit)
        || !(normalized[9].is_ascii_digit() || normalized[9].eq_ignore_ascii_case(&b'X'))
    {
        return Err(FunctionError::InvalidArgument {
            function: "isbn10_to_isbn13",
            message: "expected a 10-character ISBN with an optional final X check digit",
        });
    }
    let isbn10_sum = normalized[..9]
        .iter()
        .enumerate()
        .map(|(index, byte)| u32::from(byte - b'0') * (10 - index as u32))
        .sum::<u32>()
        + if normalized[9].eq_ignore_ascii_case(&b'X') {
            10
        } else {
            u32::from(normalized[9] - b'0')
        };
    if isbn10_sum % 11 != 0 {
        return Err(FunctionError::InvalidArgument {
            function: "isbn10_to_isbn13",
            message: "ISBN-10 check digit is invalid",
        });
    }

    let mut output = Vec::with_capacity(13);
    output.extend_from_slice(b"978");
    output.extend_from_slice(&normalized[..9]);
    let weighted = output
        .iter()
        .enumerate()
        .map(|(index, byte)| u32::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 })
        .sum::<u32>();
    output.push(b'0' + ((10 - weighted % 10) % 10) as u8);
    let converted = String::from_utf8(output).map_err(|_| FunctionError::InvalidArgument {
        function: "isbn10_to_isbn13",
        message: "converted ISBN was not valid UTF-8",
    })?;
    Ok(Value::String(converted))
}
