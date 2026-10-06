use codegen::{CsvOutputError, CsvOutputPolicy, Program};
use ir::SchemaKind;

use super::{EmitError, rust_string};

pub(super) fn render(program: &Program, policy: &CsvOutputPolicy) -> Result<String, EmitError> {
    let SchemaKind::Group { children, .. } = &program.target.kind else {
        return Err(CsvOutputError::TargetSchema {
            field: None,
            reason: "a flat group schema is required",
        }
        .into());
    };
    let mut source = String::from("\nfn csv_output_schema() -> codegen_runtime::SchemaNode {\n");
    source.push_str(&format!(
        "    codegen_runtime::SchemaNode::group({}, vec![\n",
        rust_string(&program.target.name)
    ));
    for child in children {
        let SchemaKind::Scalar { ty } = &child.kind else {
            return Err(CsvOutputError::TargetSchema {
                field: Some(child.name.clone()),
                reason: "each column must have one non-repeating scalar type",
            }
            .into());
        };
        source.push_str(&format!(
            "        codegen_runtime::SchemaNode::scalar({}, ScalarType::{ty:?}),\n",
            rust_string(&child.name)
        ));
    }
    source.push_str("    ])\n}\n\n");
    source.push_str("fn csv_output_options() -> codegen_runtime::CsvWriteOptions {\n");
    source.push_str("    codegen_runtime::CsvWriteOptions {\n");
    source.push_str(&format!(
        "        delimiter: {},\n",
        character_option(policy.delimiter)
    ));
    source.push_str(&format!(
        "        quote: {},\n",
        character_option(policy.quote)
    ));
    source.push_str(&format!(
        "        quote_disabled: {},\n",
        policy.quote_disabled
    ));
    source.push_str(&format!("        has_headers: {},\n", policy.has_headers));
    source.push_str(&format!("        utf8_bom: {},\n", policy.utf8_bom));
    source.push_str("        repair_dependency: None,\n    }\n}\n\n");
    source.push_str(ADAPTERS);
    Ok(source)
}

fn character_option(character: Option<char>) -> String {
    match character {
        Some(character) => format!("Some({character:?})"),
        None => "None".to_string(),
    }
}

const ADAPTERS: &str = r#"/// Map every row, then serialize bounded CSV using the selected static policy.
pub fn execute_csv(source: &Instance) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute(source).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Execute the same CSV mapping with the caller's fixed execution context.
pub fn execute_csv_with_context(
    source: &Instance,
    execution: &ExecutionContext<'_>,
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_context(source, execution).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Return the same complete bounded UTF-8 CSV as an owned byte buffer.
pub fn execute_csv_bytes(source: &Instance) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute(source).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}

/// Return bounded CSV bytes using the caller's fixed execution context.
pub fn execute_csv_bytes_with_context(
    source: &Instance,
    execution: &ExecutionContext<'_>,
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_context(source, execution).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}
"#;

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use codegen::{Binding, Expression, ExpressionNode, IterationPlan, TargetScope};
    use ir::{ScalarType, SchemaNode, Value};

    use super::*;
    use crate::{Options, RuntimeDependency, emit, emit_with_csv_output};

    fn program() -> Program {
        Program {
            xml_boundary: None,
            source: SchemaNode::group("Source", Vec::new()).repeating(),
            extra_sources: Vec::new(),
            target: SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Count", ScalarType::Int),
                ],
            ),
            expressions: vec![
                ExpressionNode {
                    id: 1,
                    expression: Expression::Const {
                        value: Value::String("fixed".into()),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::Const {
                        value: Value::Int(7),
                    },
                },
            ],
            user_functions: Vec::new(),
            failure_rules: Vec::new(),
            extra_targets: Vec::new(),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: Some(IterationPlan::source(Vec::new())),
                construction: Default::default(),
                bindings: vec![
                    Binding {
                        target_field: "Name".into(),
                        expression: 1,
                        target_domain: ScalarType::String.into(),
                        repeating: false,
                    },
                    Binding {
                        target_field: "Count".into(),
                        expression: 2,
                        target_domain: ScalarType::Int.into(),
                        repeating: false,
                    },
                ],
                children: Vec::new(),
            },
        }
    }

    fn options() -> Options {
        Options {
            package_name: "csv_host".into(),
            runtime_dependency: RuntimeDependency::Path("../runtime".into()),
        }
    }

    #[test]
    fn explicit_csv_keeps_both_ordinary_artifact_bodies_as_complete_prefixes() {
        let program = program();
        let options = options();
        let old = emit(&program, &options).unwrap();
        let new = emit_with_csv_output(&program, &options, &CsvOutputPolicy::default()).unwrap();
        assert_eq!(old.len(), 2);
        assert_eq!(new.len(), old.len());
        for (before, after) in old.files().iter().zip(new.files()) {
            assert_eq!(before.path, after.path);
            if before.path.as_str() == "src/lib.rs" {
                assert!(after.contents.starts_with(&before.contents));
                assert!(after.contents.len() > before.contents.len());
            } else {
                assert_eq!(before.contents, after.contents);
            }
        }
        assert_eq!(emit(&program, &options).unwrap(), old);
        assert_eq!(
            emit_with_csv_output(&program, &options, &CsvOutputPolicy::default()).unwrap(),
            new
        );
    }

    #[test]
    fn csv_descriptor_order_is_target_order_with_literal_unicode_and_policy() {
        let mut program = program();
        let SchemaKind::Group { children, .. } = &mut program.target.kind else {
            unreachable!()
        };
        children.swap(0, 1);
        children[1].name = "é\"\\\ncolumn".into();
        program.root.bindings[0].target_field = "é\"\\\ncolumn".into();
        let policy = CsvOutputPolicy {
            delimiter: Some(';'),
            quote: Some('\''),
            quote_disabled: false,
            has_headers: false,
            utf8_bom: true,
        };
        let artifacts = emit_with_csv_output(&program, &options(), &policy).unwrap();
        let source = std::str::from_utf8(
            &artifacts
                .files()
                .iter()
                .find(|file| file.path.as_str() == "src/lib.rs")
                .unwrap()
                .contents,
        )
        .unwrap();
        let appended = source.split("\nfn csv_output_schema()").nth(1).unwrap();
        let first = appended
            .find("SchemaNode::scalar(\"Count\", ScalarType::Int)")
            .unwrap();
        let second = appended
            .find(&format!(
                "SchemaNode::scalar({}, ScalarType::String)",
                rust_string("é\"\\\ncolumn")
            ))
            .unwrap();
        assert!(first < second);
        assert!(appended.contains("delimiter: Some(';'),"));
        assert!(appended.contains("quote: Some('\\''),"));
        assert!(appended.contains("has_headers: false,"));
        assert!(appended.contains("utf8_bom: true,"));
    }

    #[test]
    fn csv_opt_in_retains_typed_invalid_mapping_and_primary_row_refusals() {
        let mut program = program();
        program.root.bindings[0].expression = 999;
        let error = emit_with_csv_output(
            &program,
            &options(),
            &CsvOutputPolicy {
                delimiter: Some('\n'),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            &error,
            EmitError::CsvOutput(CsvOutputError::InvalidProgram(_))
        ));
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<CsvOutputError>()
                .is_some()
        );
        program.root.bindings[0].expression = 1;
        program.root.iteration = None;
        assert!(matches!(
            emit_with_csv_output(&program, &options(), &CsvOutputPolicy::default()),
            Err(EmitError::CsvOutput(CsvOutputError::PrimaryRows))
        ));
        assert!(emit(&program, &options()).is_ok());
    }
}
