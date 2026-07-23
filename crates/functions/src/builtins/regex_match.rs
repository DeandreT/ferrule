use ir::Value;
use regex::RegexBuilder;

use crate::{FunctionError, scalar::text as scalar_text};

const MAX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_COMPILED_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn matches(args: &[Value]) -> Result<Value, FunctionError> {
    let (input, pattern, flags) = match args {
        [input, pattern] => (input, pattern, String::new()),
        [input, pattern, flags] => (input, pattern, scalar_text(flags)),
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "matches",
                expected: 2,
                got: args.len(),
            });
        }
    };
    let input = scalar_text(input);
    let pattern = scalar_text(pattern);
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(FunctionError::InvalidArgument {
            function: "matches",
            message: "pattern exceeds 64 KiB",
        });
    }
    let mut builder = RegexBuilder::new(&pattern);
    builder.size_limit(MAX_COMPILED_BYTES);
    for flag in flags.chars() {
        match flag {
            'i' => builder.case_insensitive(true),
            'm' => builder.multi_line(true),
            's' => builder.dot_matches_new_line(true),
            'x' => builder.ignore_whitespace(true),
            _ => {
                return Err(FunctionError::InvalidArgument {
                    function: "matches",
                    message: "flags contain an unsupported value",
                });
            }
        };
    }
    let regex = builder
        .build()
        .map_err(|_| FunctionError::InvalidArgument {
            function: "matches",
            message: "pattern is invalid or exceeds the compiled-size limit",
        })?;
    Ok(Value::Bool(regex.is_match(&input)))
}
