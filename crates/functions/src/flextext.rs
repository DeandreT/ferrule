use ir::{Instance, Value};

use crate::FunctionError;

const FUNCTION: &str = "flextext_parse_field";

pub(super) fn parse_field(args: &[Value]) -> Result<Value, FunctionError> {
    let [input, layout, path] = args else {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 3,
            got: args.len(),
        });
    };
    let Value::String(layout) = layout else {
        return Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: layout.type_name(),
        });
    };
    let Value::String(path) = path else {
        return Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: path.type_name(),
        });
    };
    let Value::String(input) = input else {
        if matches!(input, Value::Null) {
            return Ok(Value::Null);
        }
        return Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: input.type_name(),
        });
    };
    let layout: mapping::FlexTextLayout =
        serde_json::from_str(layout).map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "layout descriptor is invalid",
        })?;
    let path: Vec<String> =
        serde_json::from_str(path).map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "field path descriptor is invalid",
        })?;
    let parsed = format_flextext::from_str(input, &layout.schema(), &layout).map_err(|_| {
        FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "input does not match the FlexText layout",
        }
    })?;
    scalar_at(&parsed, &path)
        .cloned()
        .ok_or(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "field path does not resolve to a scalar",
        })
}

fn scalar_at<'a>(instance: &'a Instance, path: &[String]) -> Option<&'a Value> {
    let mut current = instance;
    for segment in path {
        current = first_item(current)?;
        current = current.field(segment)?;
    }
    first_item(current)?.as_scalar()
}

fn first_item(mut instance: &Instance) -> Option<&Instance> {
    loop {
        instance = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items.first()?,
            _ => return Some(instance),
        };
    }
}

#[cfg(test)]
mod tests {
    use ir::ScalarType;
    use mapping::{
        DelimitedDialect, DelimitedRecordField, FlexCommand, FlexLineEnding, FlexTextLayout,
    };

    use super::*;

    #[test]
    fn parses_a_typed_field_through_a_single_record_sequence() {
        let layout = FlexTextLayout::new(
            "Parsed",
            FlexCommand::DelimitedRecords {
                name: "Row".into(),
                dialect: DelimitedDialect::new(',', "\n", '"', '\\').unwrap(),
                fields: vec![
                    DelimitedRecordField::new("Name", ScalarType::String).unwrap(),
                    DelimitedRecordField::new("Count", ScalarType::Int).unwrap(),
                ],
            },
            FlexLineEnding::Lf,
            false,
        )
        .unwrap();
        let args = [
            Value::String("Ada,3".into()),
            Value::String(serde_json::to_string(&layout).unwrap()),
            Value::String(serde_json::to_string(&vec!["Row", "Count"]).unwrap()),
        ];
        assert_eq!(parse_field(&args), Ok(Value::Int(3)));
    }
}
