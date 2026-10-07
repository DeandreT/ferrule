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
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_none())
    {
        source.push_str(NAMED_ADAPTERS);
    }
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some())
    {
        source.push_str(DYNAMIC_ADAPTERS);
    }
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

const NAMED_ADAPTERS: &str = r#"
/// Validate and map static named inputs, then return bounded CSV text.
pub fn execute_csv_with_sources(
    source: &Instance,
    inputs: &[NamedInput<'_>],
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources(source, inputs).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Use the same named inputs and the caller's fixed execution context.
pub fn execute_csv_with_sources_and_context(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    execution: &ExecutionContext<'_>,
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_and_context(source, inputs, execution).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Return complete bounded CSV bytes after validating and mapping named inputs.
pub fn execute_csv_bytes_with_sources(
    source: &Instance,
    inputs: &[NamedInput<'_>],
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources(source, inputs).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}

/// Return named-input CSV bytes using the caller's fixed execution context.
pub fn execute_csv_bytes_with_sources_and_context(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    execution: &ExecutionContext<'_>,
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_and_context(source, inputs, execution).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}
"#;

const DYNAMIC_ADAPTERS: &str = r#"
/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_with_dynamic_source_loader(
    source: &Instance,
    loader: &dyn DynamicSourceLoader,
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_dynamic_source_loader(source, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_with_sources_and_dynamic_source_loader(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    loader: &dyn DynamicSourceLoader,
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_and_dynamic_source_loader(source, inputs, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_with_sources_context_and_dynamic_source_loader(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    execution: &ExecutionContext<'_>,
    loader: &dyn DynamicSourceLoader,
) -> Result<String, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_context_and_dynamic_source_loader(source, inputs, execution, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv(&csv_output_schema(), &output, &csv_output_options())
}

/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_bytes_with_dynamic_source_loader(
    source: &Instance,
    loader: &dyn DynamicSourceLoader,
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_dynamic_source_loader(source, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}

/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_bytes_with_sources_and_dynamic_source_loader(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    loader: &dyn DynamicSourceLoader,
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_and_dynamic_source_loader(source, inputs, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
    codegen_runtime::serialize_csv_bytes(&csv_output_schema(), &output, &csv_output_options())
}

/// Map with the existing per-driver typed loader, then serialize bounded CSV.
pub fn execute_csv_bytes_with_sources_context_and_dynamic_source_loader(
    source: &Instance,
    inputs: &[NamedInput<'_>],
    execution: &ExecutionContext<'_>,
    loader: &dyn DynamicSourceLoader,
) -> Result<Vec<u8>, codegen_runtime::CsvBoundaryError> {
    let output = execute_with_sources_context_and_dynamic_source_loader(source, inputs, execution, loader).map_err(codegen_runtime::CsvBoundaryError::from)?;
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

    #[test]
    fn named_csv_companions_are_conditional_and_preserve_the_complete_legacy_adapter() {
        let plain = program();
        let before = render(&plain, &CsvOutputPolicy::default()).unwrap();
        assert!(before.ends_with(ADAPTERS));
        assert!(!before.contains("pub fn execute_csv_with_sources("));
        let mut named = plain.clone();
        named.extra_sources.push(codegen::NamedSourceProgram {
            name: "Settings".into(),
            source: SchemaNode::group("Settings", Vec::new()),
            dynamic: None,
        });
        let after = render(&named, &CsvOutputPolicy::default()).unwrap();
        assert_eq!(after, format!("{before}{NAMED_ADAPTERS}"));
        for declaration in [
            "pub fn execute_csv_with_sources(",
            "pub fn execute_csv_with_sources_and_context(",
            "pub fn execute_csv_bytes_with_sources(",
            "pub fn execute_csv_bytes_with_sources_and_context(",
        ] {
            assert_eq!(NAMED_ADAPTERS.matches(declaration).count(), 1);
        }
        for call in [
            "execute_with_sources(source, inputs).map_err",
            "execute_with_sources_and_context(source, inputs, execution).map_err",
        ] {
            assert_eq!(NAMED_ADAPTERS.matches(call).count(), 2);
        }
        assert_eq!(
            NAMED_ADAPTERS
                .matches("codegen_runtime::serialize_csv(")
                .count(),
            2
        );
        assert_eq!(
            NAMED_ADAPTERS
                .matches("codegen_runtime::serialize_csv_bytes(")
                .count(),
            2
        );
        assert!(!NAMED_ADAPTERS.contains("execute_outputs"));
        assert!(!NAMED_ADAPTERS.contains("execute_json"));
        let ordinary = emit(&named, &options()).unwrap();
        let opted = emit_with_csv_output(&named, &options(), &CsvOutputPolicy::default()).unwrap();
        for (old, new) in ordinary.files().iter().zip(opted.files()) {
            assert_eq!(old.path, new.path);
            if old.path.as_str() == "src/lib.rs" {
                assert!(new.contents.starts_with(&old.contents));
                assert!(new.contents.ends_with(after.as_bytes()));
            } else {
                assert_eq!(old.contents, new.contents);
            }
        }
        assert_eq!(emit(&named, &options()).unwrap(), ordinary);
        assert_eq!(render(&plain, &CsvOutputPolicy::default()).unwrap(), before);
    }
    #[test]
    fn dynamic_csv_companions_delegate_once_and_keep_plain_and_static_adapters_exact() {
        let plain = program();
        let before = render(&plain, &CsvOutputPolicy::default()).unwrap();
        let mut dynamic = plain.clone();
        dynamic.extra_sources.push(codegen::NamedSourceProgram {
            name: "Dynamic".into(),
            source: SchemaNode::group("Dynamic", Vec::new()),
            dynamic: Some(codegen::DynamicSourceProgram {
                path: 1,
                driver: codegen::SourceIteration::new(Vec::new()),
            }),
        });
        let after = render(&dynamic, &CsvOutputPolicy::default()).unwrap();
        assert_eq!(after, format!("{before}{DYNAMIC_ADAPTERS}"));
        assert!(!after.contains("pub fn execute_csv_with_sources("));
        for suffix in [
            "with_dynamic_source_loader",
            "with_sources_and_dynamic_source_loader",
            "with_sources_context_and_dynamic_source_loader",
        ] {
            assert_eq!(
                DYNAMIC_ADAPTERS
                    .matches(&format!("pub fn execute_csv_{suffix}("))
                    .count(),
                1
            );
            assert_eq!(
                DYNAMIC_ADAPTERS
                    .matches(&format!("pub fn execute_csv_bytes_{suffix}("))
                    .count(),
                1
            );
        }
        for call in [
            "execute_with_dynamic_source_loader(source, loader)",
            "execute_with_sources_and_dynamic_source_loader(source, inputs, loader)",
            "execute_with_sources_context_and_dynamic_source_loader(source, inputs, execution, loader)",
        ] {
            assert_eq!(DYNAMIC_ADAPTERS.matches(call).count(), 2);
        }
        assert_eq!(
            DYNAMIC_ADAPTERS
                .matches("codegen_runtime::serialize_csv(")
                .count(),
            3
        );
        assert_eq!(
            DYNAMIC_ADAPTERS
                .matches("codegen_runtime::serialize_csv_bytes(")
                .count(),
            3
        );
        assert!(!DYNAMIC_ADAPTERS.contains("execute_outputs"));
        assert!(!DYNAMIC_ADAPTERS.contains("execute_json"));
        assert!(!DYNAMIC_ADAPTERS.contains("parse_json"));
        let ordinary = emit(&dynamic, &options()).unwrap();
        let opted =
            emit_with_csv_output(&dynamic, &options(), &CsvOutputPolicy::default()).unwrap();
        assert_eq!(ordinary.files().len(), opted.files().len());
        for (old, new) in ordinary.files().iter().zip(opted.files()) {
            assert_eq!(old.path, new.path);
            if old.path.as_str() == "src/lib.rs" {
                assert!(new.contents.starts_with(&old.contents));
            } else {
                assert_eq!(old.contents, new.contents);
            }
        }
        assert_eq!(emit(&dynamic, &options()).unwrap(), ordinary);
        dynamic.extra_sources.push(codegen::NamedSourceProgram {
            name: "Static".into(),
            source: SchemaNode::group("Static", Vec::new()),
            dynamic: None,
        });
        let mut static_only = dynamic.clone();
        static_only.extra_sources.remove(0);
        let static_adapter = render(&static_only, &CsvOutputPolicy::default()).unwrap();
        assert_eq!(
            render(&dynamic, &CsvOutputPolicy::default()).unwrap(),
            format!("{static_adapter}{DYNAMIC_ADAPTERS}")
        );
        assert_eq!(render(&plain, &CsvOutputPolicy::default()).unwrap(), before);
        let mixed_ordinary = emit(&dynamic, &options()).unwrap();
        let _mixed_opted =
            emit_with_csv_output(&dynamic, &options(), &CsvOutputPolicy::default()).unwrap();
        assert_eq!(emit(&dynamic, &options()).unwrap(), mixed_ordinary);
    }
}
