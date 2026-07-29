use std::num::NonZeroU32;

use codegen::{DelimitedTextField, Expression, ExpressionNode, ScalarFunction};
use ir::{ScalarType, Value};
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout,
};

use super::*;

#[test]
fn emits_exact_scalar_function_names_through_the_shared_runtime() {
    let mut program = program();
    program.expressions.extend([
        ExpressionNode {
            id: 100,
            expression: Expression::Const {
                value: Value::String(" 42 ".into()),
            },
        },
        ExpressionNode {
            id: 101,
            expression: Expression::Const {
                value: Value::Int(0),
            },
        },
        ExpressionNode {
            id: 102,
            expression: Expression::Call {
                function: ScalarFunction::Trim,
                args: vec![100],
            },
        },
        ExpressionNode {
            id: 103,
            expression: Expression::Call {
                function: ScalarFunction::IsNumeric,
                args: vec![102],
            },
        },
        ExpressionNode {
            id: 104,
            expression: Expression::Call {
                function: ScalarFunction::ToNumber,
                args: vec![102],
            },
        },
        ExpressionNode {
            id: 105,
            expression: Expression::Call {
                function: ScalarFunction::DelayPassthrough,
                args: vec![104, 101],
            },
        },
        ExpressionNode {
            id: 106,
            expression: Expression::Const {
                value: Value::String("\\d+".into()),
            },
        },
        ExpressionNode {
            id: 107,
            expression: Expression::Call {
                function: ScalarFunction::Matches,
                args: vec![100, 106],
            },
        },
        ExpressionNode {
            id: 108,
            expression: Expression::Const {
                value: Value::String("#".into()),
            },
        },
        ExpressionNode {
            id: 109,
            expression: Expression::Call {
                function: ScalarFunction::Replace,
                args: vec![100, 106, 108],
            },
        },
        ExpressionNode {
            id: 110,
            expression: Expression::Const {
                value: Value::String(
                    serde_json::to_string(&ir::SchemaNode::scalar("Value", ir::ScalarType::Int))
                        .expect("JSON parser schema serializes"),
                ),
            },
        },
        ExpressionNode {
            id: 111,
            expression: Expression::Const {
                value: Value::String("[]".into()),
            },
        },
        ExpressionNode {
            id: 112,
            expression: Expression::Call {
                function: ScalarFunction::JsonParseField,
                args: vec![100, 110, 111],
            },
        },
        ExpressionNode {
            id: 113,
            expression: Expression::Const {
                value: Value::String(r#"["Value"]"#.into()),
            },
        },
        ExpressionNode {
            id: 114,
            expression: Expression::Const {
                value: Value::String("string".into()),
            },
        },
        ExpressionNode {
            id: 115,
            expression: Expression::Call {
                function: ScalarFunction::JsonSerializeObject,
                args: vec![113, 114, 100],
            },
        },
        ExpressionNode {
            id: 116,
            expression: Expression::DelimitedTextField {
                input: 100,
                parser: delimited_parser(),
            },
        },
        ExpressionNode {
            id: 117,
            expression: Expression::Call {
                function: ScalarFunction::DurationFromParts,
                args: vec![101, 101, 101],
            },
        },
        ExpressionNode {
            id: 118,
            expression: Expression::Call {
                function: ScalarFunction::SqliteMultiply,
                args: vec![101, 101],
            },
        },
    ]);
    let selected = program
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Selected")
        .expect("test program has a selected binding");
    selected.expression = 105;

    let artifacts = emit(
        &program,
        &Options {
            package_name: "scalar-functions".into(),
            runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .expect("supported scalar calls emit");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated Rust source");

    for name in [
        "trim",
        "is_numeric",
        "to_number",
        "delay_passthrough",
        "matches",
        "replace",
        "json_parse_field",
        "json_serialize_object",
        "flextext_parse_field",
        "duration_from_parts",
        "sqlite_multiply",
    ] {
        assert!(source.contains(&format!("call(\"{name}\", &args)")));
    }
}

fn delimited_parser() -> DelimitedTextField {
    let layout = FlexTextLayout::new(
        "Root",
        FlexCommand::DelimitedRecords {
            name: "Row".into(),
            dialect: DelimitedDialect::new(',', "\n", '"', '"').expect("test dialect is valid"),
            fields: vec![
                DelimitedRecordField::new("Name", ScalarType::String).expect("test field is valid"),
            ],
        },
        FlexLineEnding::Lf,
        false,
    )
    .expect("test layout is valid");
    DelimitedTextField::from_descriptors(
        &serde_json::to_string(&layout).expect("test layout serializes"),
        r#"["Row","Name"]"#,
    )
    .expect("test parser profile is portable")
}

#[test]
fn generated_fixed_width_flextext_projection_executes_the_embedded_layout() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .expect("runtime has a workspace parent");
    let output = TempDir::new("rust_fixed_width_flextext_codegen");
    let program = Program {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Raw", ScalarType::String)],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Code", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Raw".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::DelimitedTextField {
                    input: 1,
                    parser: fixed_width_parser("Code"),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::DelimitedTextField {
                    input: 1,
                    parser: fixed_width_parser("Count"),
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    expression: 2,
                    target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
                Binding {
                    target_field: "Count".into(),
                    expression: 3,
                    target_domain: codegen::ScalarTargetDomain::Single(ScalarType::Int),
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };
    let artifacts = emit(
        &program,
        &Options {
            package_name: "fixed-width-flextext".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("fixed-width program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Value, field, group, scalar};

fn main() {
    let source = group([field(
        "Raw",
        scalar(Value::String(
            "\u{feff}\u{00c5}B_007Zed\r\nCD__42Ada_".into(),
        )),
    )]);
    assert_eq!(
        fixed_width_flextext::execute(&source).unwrap(),
        group([
            field("Code", scalar(Value::String("\u{00c5}B".into()))),
            field("Count", scalar(Value::Int(7))),
        ]),
    );

    let empty = group([field(
        "Raw",
        scalar(Value::String("___007Zed".into())),
    )]);
    assert_eq!(
        fixed_width_flextext::execute(&empty).unwrap(),
        group([
            field("Code", scalar(Value::String(String::new()))),
            field("Count", scalar(Value::Int(7))),
        ]),
    );
}
"#,
    )
    .expect("generated harness is written");

    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()
        .expect("generated Rust harness starts");
    assert!(
        run.status.success(),
        "generated Rust fixed-width project failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

fn fixed_width_parser(field: &str) -> DelimitedTextField {
    let layout = FlexTextLayout::new(
        "Root",
        FlexCommand::FixedWidthRecords {
            name: "Row".into(),
            fields: vec![
                FixedWidthRecordField::new(
                    "Code",
                    ScalarType::String,
                    NonZeroU32::new(3).expect("test width is nonzero"),
                )
                .expect("test field is valid"),
                FixedWidthRecordField::new(
                    "Count",
                    ScalarType::Int,
                    NonZeroU32::new(3).expect("test width is nonzero"),
                )
                .expect("test field is valid"),
                FixedWidthRecordField::new(
                    "Label",
                    ScalarType::String,
                    NonZeroU32::new(4).expect("test width is nonzero"),
                )
                .expect("test field is valid"),
            ],
            fill_char: '_',
            record_delimiters: true,
            treat_empty_as_absent: false,
        },
        FlexLineEnding::Lf,
        false,
    )
    .expect("test layout is valid");
    DelimitedTextField::from_descriptors(
        &serde_json::to_string(&layout).expect("test layout serializes"),
        &serde_json::to_string(&vec!["Row", field]).expect("test path serializes"),
    )
    .expect("test parser profile is portable")
}
