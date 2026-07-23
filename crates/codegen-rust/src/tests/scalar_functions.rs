use codegen::{Expression, ExpressionNode, ScalarFunction};
use ir::Value;

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
    ] {
        assert!(source.contains(&format!("call(\"{name}\", &args)")));
    }
}
