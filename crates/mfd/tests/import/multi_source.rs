use ir::{Instance, Value};
use mapping::Node;

use super::{fixture, scalar};

fn row(name: &str) -> Instance {
    Instance::Group(vec![(
        "Name".into(),
        Instance::Scalar(Value::String(name.into())),
    )])
}

#[test]
fn primary_scoring_and_named_secondary_frames_are_executable() {
    let imported = mfd::import(&fixture("multi-source.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Alpha");
    assert_eq!(project.source_path.as_deref(), Some("alpha.xml"));
    assert_eq!(project.extra_sources.len(), 1);
    assert_eq!(project.extra_sources[0].name, "Beta");
    assert_eq!(project.extra_sources[0].path, "beta.xml");

    let secondary = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "B")
        .unwrap();
    assert_eq!(
        secondary.source().map(|path| path.to_vec()),
        Some(vec!["Beta".into(), "Rows".into()])
    );
    let binding = secondary
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Name")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&binding.node],
        Node::SourceField { path, frame }
            if path == &["Name"] && frame.as_deref() == Some(&["Beta".into(), "Rows".into()])
    ));

    let primary = Instance::Group(vec![
        ("RowsA".into(), Instance::Repeated(vec![row("a1")])),
        ("RowsB".into(), Instance::Repeated(vec![row("a2")])),
    ]);
    let beta = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("b1"), row("b2")]),
    )]);
    let target = engine::run_with_sources(project, &primary, vec![("Beta".into(), beta)]).unwrap();
    let rows = target.field("B").and_then(Instance::as_repeated).unwrap();
    assert_eq!(
        rows.iter()
            .map(|item| scalar(item, "Name"))
            .collect::<Vec<_>>(),
        vec![Value::String("b1".into()), Value::String("b2".into())]
    );
}

#[test]
fn primary_scoring_unwraps_supported_iteration_controls() {
    let imported = mfd::import(&fixture("multi-source-controlled.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Alpha");
    assert_eq!(project.source_path.as_deref(), Some("alpha.xml"));
    assert_eq!(project.extra_sources.len(), 1);
    assert_eq!(project.extra_sources[0].name, "Beta");
    assert_eq!(project.extra_sources[0].path, "beta.xml");

    let primary = Instance::Group(vec![
        (
            "RowsA".into(),
            Instance::Repeated(vec![row("second"), row("first")]),
        ),
        ("RowsB".into(), Instance::Repeated(Vec::new())),
    ]);
    let beta = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("secondary")]),
    )]);
    let target = engine::run_with_sources(project, &primary, vec![("Beta".into(), beta)]).unwrap();
    let rows = target
        .field("AOne")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(scalar(&rows[0], "Name"), Value::String("first".into()));
}

#[test]
fn nested_xml_file_instances_retain_executable_source_and_target_paths() {
    let imported = mfd::import(&fixture("multi-source-nested-files.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("alpha.xml"));
    assert_eq!(project.target_path.as_deref(), Some("output.xml"));
    let [secondary] = project.extra_sources.as_slice() else {
        panic!("expected one secondary XML source");
    };
    assert_eq!(secondary.name, "Beta");
    assert_eq!(secondary.path, "beta.xml");

    let primary = Instance::Group(vec![
        ("RowsA".into(), Instance::Repeated(vec![row("a1")])),
        ("RowsB".into(), Instance::Repeated(vec![row("a2")])),
    ]);
    let beta = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("b1"), row("b2")]),
    )]);
    let target = engine::run_with_sources(project, &primary, vec![("Beta".into(), beta)]).unwrap();

    let rows = target.field("B").and_then(Instance::as_repeated).unwrap();
    assert_eq!(
        rows.iter()
            .map(|item| scalar(item, "Name"))
            .collect::<Vec<_>>(),
        vec![Value::String("b1".into()), Value::String("b2".into())]
    );
}
