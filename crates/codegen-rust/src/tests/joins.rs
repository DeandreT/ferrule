use super::*;
use codegen::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, SortFilterOrder, SortKey,
    SortPlan,
};

fn join_program() -> Program {
    let a = SchemaNode::group(
        "A",
        vec![
            SchemaNode::scalar("Id", ScalarType::Int),
            SchemaNode::scalar("Region", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    )
    .repeating();
    let b = SchemaNode::group(
        "B",
        vec![
            SchemaNode::scalar("Aid", ScalarType::String),
            SchemaNode::scalar("Region", ScalarType::String),
            SchemaNode::scalar("Tag", ScalarType::String),
            SchemaNode::scalar("Rank", ScalarType::Int),
        ],
    )
    .repeating();
    let b_collection = vec!["Catalog".into(), "B".into()];
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(b_collection.clone()),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["Aid".into()],
        ))
        .and(JoinKey::new(
            vec!["A".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        )),
    )
    .expect("valid join plan");
    let row = TargetScope {
        target_field: "Row".into(),
        repeating: true,
        iteration: Some(IterationPlan::new(
            InnerJoin::new(JoinId::new(7), plan),
            Some(8),
            Some(SortPlan::new(
                SortKey {
                    expression: 3,
                    descending: true,
                },
                Vec::new(),
                SortFilterOrder::SortThenFilter,
            )),
            vec![SequenceWindow::First { count: 9 }],
            IterationOutput::Repeated,
        )),
        construction: TargetConstruction::Group,
        bindings: [
            ("Label", 1, ScalarType::String),
            ("Tag", 2, ScalarType::String),
            ("JoinPosition", 4, ScalarType::Int),
            ("APosition", 5, ScalarType::Int),
            ("BPosition", 6, ScalarType::Int),
        ]
        .into_iter()
        .map(|(target_field, expression, target_type)| Binding {
            target_field: target_field.into(),
            expression,
            target_type,
            repeating: false,
        })
        .collect(),
        children: vec![TargetScope {
            target_field: "Details".into(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Summary".into(),
                expression: 11,
                target_type: ScalarType::String,
                repeating: false,
            }],
            children: Vec::new(),
        }],
    };

    Program {
        source: SchemaNode::group("Source", vec![a]),
        extra_sources: vec![NamedSourceProgram {
            name: "Catalog".into(),
            source: SchemaNode::group("Catalog", vec![b]),
        }],
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Label", ScalarType::String),
                        SchemaNode::scalar("Tag", ScalarType::String),
                        SchemaNode::scalar("JoinPosition", ScalarType::Int),
                        SchemaNode::scalar("APosition", ScalarType::Int),
                        SchemaNode::scalar("BPosition", ScalarType::Int),
                        SchemaNode::group(
                            "Details",
                            vec![SchemaNode::scalar("Summary", ScalarType::String)],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
        expressions: vec![
            join_field(1, 7, &["A"], &["Label"]),
            join_field(2, 7, &["Catalog", "B"], &["Tag"]),
            join_field(3, 7, &["Catalog", "B"], &["Rank"]),
            ExpressionNode {
                id: 4,
                expression: Expression::JoinPosition {
                    join: JoinId::new(7),
                },
            },
            position(5, &["A"]),
            position(6, &["Catalog", "B"]),
            constant(7, Value::Int(10)),
            ExpressionNode {
                id: 8,
                expression: Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![3, 7],
                },
            },
            constant(9, Value::Int(2)),
            constant(10, Value::String(":".into())),
            ExpressionNode {
                id: 11,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![1, 10, 2],
                },
            },
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: vec![row],
        },
        extra_targets: Vec::new(),
    }
}

fn join_field(id: u32, join: u64, collection: &[&str], path: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::JoinField {
            join: JoinId::new(join),
            collection: collection.iter().map(|segment| (*segment).into()).collect(),
            path: path.iter().map(|segment| (*segment).into()).collect(),
        },
    }
}

fn position(id: u32, collection: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Position {
            collection: collection.iter().map(|segment| (*segment).into()).collect(),
        },
    }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
    }
}

#[test]
fn emits_left_deep_join_fields_positions_and_controls() {
    let artifacts = emit(
        &join_program(),
        &Options {
            package_name: "join-map".into(),
            runtime_dependency: RuntimeDependency::Version("1".into()),
        },
    )
    .expect("join program emits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated source is UTF-8");

    assert!(source.contains("context.inner_join(7, &[\"A\"]"));
    assert!(source.contains("left_collection: &[\"A\"]"));
    assert!(source.contains("right_path: &[\"Region\"]"));
    assert!(source.contains("context.resolve_join_scalar(7, &[\"Catalog\", \"B\"]"));
    assert!(source.contains("context.join_position(7)?"));
    assert!(source.contains("with_compact_last_position(outputs.len() + 1)"));
}

#[test]
fn generated_join_package_preserves_named_sources_compaction_and_static_children() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    let output = TempDir::new("rust_join_codegen");
    let artifacts = emit(
        &join_program(),
        &Options {
            package_name: "join-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, NamedInput, Value, field, group, repeated, scalar, string};

fn row(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    group(fields.into_iter().map(|(name, value)| field(name, scalar(value))))
}

fn main() {
    let source = group([field("A", repeated([
        row([("Id", Value::Int(1)), ("Region", string("west")), ("Label", string("A1"))]),
        row([("Id", Value::Int(1)), ("Region", string("west")), ("Label", string("A2"))]),
        row([("Id", Value::Null), ("Region", string("west")), ("Label", string("AN"))]),
    ]))]);
    let catalog = group([field("B", repeated([
        row([("Aid", string("1")), ("Region", string("west")), ("Tag", string("high")), ("Rank", Value::Int(30))]),
        row([("Aid", Value::Int(1)), ("Region", string("west")), ("Tag", string("mid")), ("Rank", Value::Int(20))]),
        row([("Aid", Value::xml_nil()), ("Region", string("west")), ("Tag", string("nil")), ("Rank", Value::Int(50))]),
        row([("Aid", Value::Int(1)), ("Region", string("east")), ("Tag", string("east")), ("Rank", Value::Int(40))]),
    ]))]);
    let inputs = [NamedInput { name: "Catalog", instance: &catalog }];
    let output = join_map::execute_with_sources(&source, &inputs).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.field("Label").and_then(Instance::as_scalar), Some(&string(if index == 0 { "A1" } else { "A2" })));
        assert_eq!(row.field("Tag").and_then(Instance::as_scalar), Some(&string("high")));
        assert_eq!(row.field("JoinPosition").and_then(Instance::as_scalar), Some(&Value::Int((index + 1) as i64)));
        assert_eq!(row.field("APosition").and_then(Instance::as_scalar), Some(&Value::Int((index + 1) as i64)));
        assert_eq!(row.field("BPosition").and_then(Instance::as_scalar), Some(&Value::Int(1)));
        let details = row.field("Details").unwrap();
        assert_eq!(details.field("Summary").and_then(Instance::as_scalar), Some(&string(if index == 0 { "A1:high" } else { "A2:high" })));
    }
}
"#,
    )
    .unwrap();

    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated Rust join package failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
