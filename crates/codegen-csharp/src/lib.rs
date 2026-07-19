//! Deterministic standalone .NET library emission for lowered ferrule mappings.

#![forbid(unsafe_code)]

mod error;
mod literal;
mod mapping;
mod runtime;

pub use error::EmitError;

use codegen::{ArtifactPath, ArtifactSet, GeneratedFile, Program};

/// Emits a complete package-free .NET 10 class library.
///
/// The artifact embeds the small Ferrule C# runtime and exposes
/// `Ferrule.Generated.GeneratedMapping.Execute(FerruleInstance)`.
pub fn emit(program: &Program) -> Result<ArtifactSet, EmitError> {
    let generated_mapping = mapping::render(program)?;
    let mut files = Vec::with_capacity(runtime::SOURCES.len() + 3);
    files.push(file("Ferrule.Generated.csproj", runtime::PROJECT)?);
    files.push(file("GeneratedMapping.cs", generated_mapping)?);
    files.push(file("GeneratedTargetBuilder.cs", runtime::TARGET_BUILDER)?);
    for (path, source) in runtime::SOURCES {
        files.push(file(path, source)?);
    }
    Ok(ArtifactSet::new(files)?)
}

fn file(path: &str, contents: impl Into<Vec<u8>>) -> Result<GeneratedFile, EmitError> {
    Ok(GeneratedFile::new(ArtifactPath::new(path)?, contents))
}

#[cfg(test)]
mod tests {
    use codegen::{Binding, Expression, ExpressionNode, Program, TargetScope};
    use ir::{ScalarType, SchemaNode, Value};

    use super::*;

    fn program() -> Program {
        Program {
            source: SchemaNode::group("schema source", Vec::new()),
            target: SchemaNode::group("schema target", Vec::new()),
            expressions: vec![
                ExpressionNode {
                    id: 9,
                    expression: Expression::Const {
                        value: Value::String("fixed".into()),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::SourceField {
                        path: vec!["input field".into()],
                    },
                },
            ],
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                bindings: vec![Binding {
                    target_field: "root value".into(),
                    expression: 9,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: vec![TargetScope {
                    target_field: "child group".into(),
                    repeating: false,
                    bindings: vec![Binding {
                        target_field: "copied value".into(),
                        expression: 2,
                        target_type: ScalarType::String,
                        repeating: false,
                    }],
                    children: Vec::new(),
                }],
            },
        }
    }

    #[test]
    fn artifacts_are_path_sorted_ascii_and_deterministic() {
        let first = emit(&program()).expect("supported program emits");
        let second = emit(&program()).expect("supported program emits deterministically");

        assert_eq!(first, second);
        assert!(
            first
                .files()
                .windows(2)
                .all(|files| files[0].path < files[1].path)
        );
        assert!(first.files().iter().all(|file| file.contents.is_ascii()));
    }

    #[test]
    fn generated_identifiers_use_only_node_and_scope_numbers() {
        let artifacts = emit(&program()).expect("supported program emits");
        let source = generated_source(&artifacts);

        assert!(source.contains("Node_2"));
        assert!(source.contains("Node_9"));
        assert!(source.contains("Scope_0"));
        assert!(source.contains("Scope_1"));
        assert!(!source.contains("schema_source"));
        assert!(!source.contains("schema_target"));
    }

    #[test]
    fn repeated_bindings_coalesce_at_the_first_field_position() {
        let mut program = program();
        program.root.bindings = vec![
            Binding {
                target_field: "line".into(),
                expression: 9,
                target_type: ScalarType::String,
                repeating: true,
            },
            Binding {
                target_field: "other".into(),
                expression: 2,
                target_type: ScalarType::String,
                repeating: false,
            },
            Binding {
                target_field: "line".into(),
                expression: 2,
                target_type: ScalarType::String,
                repeating: true,
            },
        ];

        let artifacts = emit(&program).expect("repeating bindings emit");
        let source = generated_source(&artifacts);
        assert_eq!(source.matches("FerruleField(\"line\"").count(), 1);
        let line = source.find("FerruleField(\"line\"").expect("line field");
        let other = source.find("FerruleField(\"other\"").expect("other field");
        assert!(line < other);
        let first = source
            .find("var value_0_0 = Node_9(source)")
            .expect("first binding evaluation");
        let middle = source
            .find("var value_0_1 = Node_2(source)")
            .expect("middle binding evaluation");
        let last = source
            .find("var value_0_2 = Node_2(source)")
            .expect("last binding evaluation");
        assert!(first < middle && middle < last);
        assert!(source.contains("{ value_0_0, value_0_2 }"));
    }

    #[test]
    fn repeating_scopes_wrap_the_constructed_group_once() {
        let mut program = program();
        program.root.repeating = true;
        program.root.children[0].repeating = true;

        let artifacts = emit(&program).expect("repeating scopes emit");
        let source = generated_source(&artifacts);
        assert!(source.contains("FerruleInstance[] { group_0 }"));
        assert!(source.contains("FerruleInstance[] { group_1 }"));
        assert_eq!(source.matches("FerruleInstance[] { group_").count(), 2);
    }

    #[test]
    fn malformed_program_references_are_typed_errors() {
        let mut missing = program();
        missing.root.bindings[0].expression = 404;
        assert_eq!(
            emit(&missing),
            Err(EmitError::MissingExpression { node: 404 })
        );

        let mut duplicate = program();
        duplicate.expressions.push(ExpressionNode {
            id: 9,
            expression: Expression::Const { value: Value::Null },
        });
        assert_eq!(emit(&duplicate), Err(EmitError::DuplicateNode { node: 9 }));
    }

    #[test]
    fn nonfinite_constants_abort_before_artifacts_are_created() {
        let mut program = program();
        program.expressions[0].expression = Expression::Const {
            value: Value::Float(f64::NAN),
        };

        assert_eq!(emit(&program), Err(EmitError::NonFiniteFloat { node: 9 }));
    }

    fn generated_source(artifacts: &ArtifactSet) -> &str {
        let file = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .expect("generated mapping artifact");
        std::str::from_utf8(&file.contents).expect("generated source is UTF-8")
    }
}
