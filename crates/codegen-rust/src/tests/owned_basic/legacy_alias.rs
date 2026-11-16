use super::*;

/// p04's legacy `Position` names `Ref/Rows` while the owned cursor walks the
/// named `Ref` rows. Like the nested-alias cohort's alias positions, it does
/// not follow the owned cursor: the interpreter returns 1 for both rows, which
/// `CHECK_P04` expects of the generated host.
#[test]
fn p04_legacy_alias_position_matches_the_interpreter() {
    use ir::{Instance, Value};
    let s = |text: &str| Instance::Scalar(Value::String(text.into()));
    let i = |value: i64| Instance::Scalar(Value::Int(value));
    let g = |fields: Vec<(&str, Instance)>| {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<Vec<_>>()
                .into(),
        )
    };
    let (_, p04, _) = generated_programs()
        .into_iter()
        .find(|(name, _, _)| *name == "p04")
        .expect("p04 case");
    let mut graph = mapping::Graph::default();
    graph.nodes.insert(
        1,
        mapping::Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: named("Ref"),
                path: path(&["Rows", "Value"]),
            },
            frame: None,
        }),
    );
    graph.nodes.insert(
        2,
        mapping::Node::OwnedPosition(OwnedPosition {
            owner: named("Ref"),
            collection: Some(path(&["Rows"])),
        }),
    );
    graph.nodes.insert(
        3,
        mapping::Node::Position {
            collection: path(&["Ref", "Rows"]),
        },
    );
    let project = mapping::Project {
        execution_contract: None,
        source: p04.source.clone(),
        target: p04.target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: p04
            .extra_sources
            .iter()
            .map(|source| mapping::NamedSource {
                name: source.name.clone(),
                path: "unused.json".into(),
                schema: source.source.clone(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: mapping::Scope {
            iteration: mapping::ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: named("Ref"),
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: ["Value", "OwnedPosition", "LegacyPosition"]
                .into_iter()
                .zip(1..)
                .map(|(target_field, node)| mapping::Binding {
                    target_field: target_field.into(),
                    node,
                })
                .collect(),
            ..mapping::Scope::default()
        },
    };
    // The Project is exactly the generated corpus's p04 Program.
    assert_eq!(codegen::lower(&project).expect("p04 lowers"), p04);
    let row = |value: &str, counter: Instance| {
        g(vec![
            ("Value", s(value)),
            ("Score", i(1)),
            ("Details", counter),
        ])
    };
    let reference = g(vec![
        ("Value", s("N.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row("N1", Instance::Scalar(Value::Null)),
                row("N2", g(vec![("Counter", i(7))])),
            ]),
        ),
    ]);
    let primary = g(vec![
        ("Value", s("P.root")),
        ("Ref", g(vec![("Value", s("P.collision"))])),
    ]);
    let actual = engine::run_outputs_with_sources_and_context(
        &project,
        &primary,
        vec![("Ref".into(), reference)],
        &engine::ExecutionContext::new(Path::new("p04.mapping")),
    )
    .expect("p04 runs natively");
    let expected = |value: &str, owned: i64| {
        g(vec![
            ("Value", s(value)),
            ("OwnedPosition", i(owned)),
            ("LegacyPosition", i(1)),
        ])
    };
    assert_eq!(
        actual.primary,
        Instance::Repeated(vec![expected("N1", 1), expected("N2", 2)])
    );
}
