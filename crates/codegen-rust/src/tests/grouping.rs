use super::*;
use codegen::{AggregateFunction, AggregateValue, GroupingPlan, SequenceWindow};

fn grouping_program() -> Program {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Keep", ScalarType::Bool),
        ],
    )
    .repeating();
    let bucket = SchemaNode::group(
        "Bucket",
        vec![
            SchemaNode::scalar("First", ScalarType::Int),
            SchemaNode::scalar("Sum", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::group("Member", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    )
    .repeating();
    Program {
        source: SchemaNode::group("Source", vec![rows]),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![bucket]),
        expressions: vec![
            source_field(1, &["Key"]),
            source_field(2, &["Value"]),
            ExpressionNode {
                id: 3,
                expression: Expression::Aggregate {
                    function: AggregateFunction::Sum,
                    collection: vec!["Rows".into()],
                    value: AggregateValue::Path(vec!["Value".into()]),
                    arg: None,
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Position {
                    collection: vec!["Rows".into()],
                },
            },
            source_field(5, &["Keep"]),
            constant(6, Value::Int(1)),
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: vec![TargetScope {
                target_field: "Bucket".into(),
                repeating: true,
                iteration: Some(IterationPlan::new_grouped(
                    SourceIteration::new(vec!["Rows".into()]),
                    Some(5),
                    None,
                    Some(GroupingPlan::By { key: 1 }),
                    vec![SequenceWindow::Last { count: 6 }],
                    IterationOutput::Repeated,
                )),
                construction: TargetConstruction::Group,
                bindings: vec![
                    binding("First", 2, ScalarType::Int),
                    binding("Sum", 3, ScalarType::Int),
                    binding("Position", 4, ScalarType::Int),
                ],
                children: vec![TargetScope {
                    target_field: "Member".into(),
                    repeating: true,
                    iteration: Some(IterationPlan::source(Vec::new())),
                    construction: TargetConstruction::Group,
                    bindings: vec![binding("Value", 2, ScalarType::Int)],
                    children: Vec::new(),
                }],
            }],
        },
        extra_targets: Vec::new(),
    }
}

fn source_field(id: u32, path: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: path.iter().map(|segment| (*segment).into()).collect(),
        },
    }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
    }
}

fn binding(target: &str, expression: u32, target_type: ScalarType) -> Binding {
    Binding {
        target_field: target.into(),
        expression,
        target_type,
        repeating: false,
    }
}

#[test]
fn emits_each_grouping_mode_after_filtering_and_before_windows() {
    let options = Options {
        package_name: "grouping-map".into(),
        runtime_dependency: RuntimeDependency::Version("1".into()),
    };
    let source = emit(&grouping_program(), &options)
        .expect("group-by emits")
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated source is UTF-8")
        .to_string();
    assert!(source.contains("GroupedItems::by(grouping_candidates, Some(\"Rows\"))"));
    assert!(source.contains("candidates = apply_sequence_windows(candidates, &windows);"));
    assert!(source.contains("with_compact_last_position(outputs.len() + 1)"));

    let mut starting = grouping_program();
    starting.root.children[0].iteration = starting.root.children[0]
        .iteration
        .take()
        .map(|iteration| iteration.with_grouping(GroupingPlan::StartingWith { predicate: 5 }));
    let starting_source = emit(&starting, &options).expect("starting-with emits");
    assert!(starting_source.files().iter().any(|file| {
        std::str::from_utf8(&file.contents)
            .is_ok_and(|source| source.contains("GroupedItems::starting_with"))
    }));

    let mut blocks = grouping_program();
    blocks.root.children[0].iteration = blocks.root.children[0]
        .iteration
        .take()
        .map(|iteration| iteration.with_grouping(GroupingPlan::IntoBlocks { size: 6 }));
    let block_source = emit(&blocks, &options).expect("block grouping emits");
    assert!(block_source.files().iter().any(|file| {
        std::str::from_utf8(&file.contents)
            .is_ok_and(|source| source.contains("GroupedItems::into_blocks"))
    }));
}

#[test]
fn generated_package_exposes_group_members_and_compacts_post_window_position() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path exists");
    let output = TempDir::new("rust_grouping_codegen");
    let artifacts = emit(
        &grouping_program(),
        &Options {
            package_name: "grouping-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("grouping program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, Value, field, group, repeated, scalar, string};

fn row(key: &str, value: i64, keep: bool) -> Instance {
    group([
        field("Key", scalar(string(key))),
        field("Value", scalar(Value::Int(value))),
        field("Keep", scalar(Value::Bool(keep))),
    ])
}

fn main() {
    let source = group([field("Rows", repeated([
        row("B", 1, true),
        row("skip", 100, false),
        row("A", 2, true),
        row("B", 3, true),
    ]))]);
    let output = grouping_map::execute(&source).unwrap();
    let buckets = output.field("Bucket").and_then(Instance::as_repeated).unwrap();
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0].field("First").and_then(Instance::as_scalar), Some(&Value::Int(2)));
    assert_eq!(buckets[0].field("Sum").and_then(Instance::as_scalar), Some(&Value::Int(2)));
    assert_eq!(buckets[0].field("Position").and_then(Instance::as_scalar), Some(&Value::Int(1)));
    let members = buckets[0].field("Member").and_then(Instance::as_repeated).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].field("Value").and_then(Instance::as_scalar), Some(&Value::Int(2)));
}
"#,
    )
    .expect("write generated grouping harness");

    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .status()
        .expect("run generated grouping package");
    assert!(status.success());
}
