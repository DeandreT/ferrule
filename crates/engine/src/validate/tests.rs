use super::*;
use ir::{ScalarType, Value};
use mapping::{Binding, DynamicBinding, NamedSource, ScopeConstruction, SequenceExpr};

fn valid_project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["name".into()],
        },
    );
    Project {
        source: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            bindings: vec![Binding {
                target_field: "name".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn accepts_a_valid_project_and_relative_source_paths() {
    let mut project = valid_project();
    project.extra_sources.push(NamedSource {
        name: "reference".into(),
        path: "reference.json".into(),
        schema: SchemaNode::group(
            "records",
            vec![SchemaNode::scalar("code", ScalarType::String)],
        ),
        options: Default::default(),
    });
    project.graph.nodes.insert(
        1,
        Node::SourceField {
            frame: None,
            path: vec!["reference".into(), "code".into()],
        },
    );

    assert!(validate(&project).is_empty());
}

#[test]
fn rejects_http_transport_metadata_on_a_target() {
    let mut project = valid_project();
    project.target_options.http_get = Some(mapping::HttpGetOptions::default());

    assert!(validate(&project).iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("only for mapping sources")
    }));
}

#[test]
fn validates_copy_current_source_construction_invariants() {
    let mut valid = valid_project();
    valid.root.set_source(None);
    valid.root.bindings.clear();
    valid.root.construction = ScopeConstruction::CopyCurrentSource;
    assert!(validate(&valid).is_empty());

    let mut content = valid.clone();
    content.root.bindings.push(Binding {
        target_field: "name".into(),
        node: 0,
    });
    content
        .root
        .dynamic_bindings
        .push(DynamicBinding { key: 0, value: 0 });
    content.root.children.push(Scope {
        target_field: "child".into(),
        ..Scope::default()
    });
    content.root.group_by = Some(0);
    let content_issues = validate(&content);
    assert!(content_issues.iter().any(|issue| {
        issue
            .message
            .contains("cannot contain bindings, child scopes, or dynamic target content")
    }));
    assert!(
        content_issues
            .iter()
            .any(|issue| { issue.message.contains("cannot use grouping controls") })
    );

    let mut scalar_source = valid.clone();
    scalar_source.root.set_source(Some(vec!["name".into()]));
    assert!(
        validate(&scalar_source)
            .iter()
            .any(|issue| { issue.message.contains("requires a group source item") })
    );

    let mut scalar_target = valid.clone();
    scalar_target.target = SchemaNode::scalar("result", ScalarType::String);
    assert!(
        validate(&scalar_target)
            .iter()
            .any(|issue| { issue.message.contains("requires a group target schema") })
    );

    let mut mismatched_target = valid.clone();
    mismatched_target.target =
        SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::Int)]);
    assert!(validate(&mismatched_target).iter().any(|issue| {
        issue
            .message
            .contains("requires matching source and target group fields")
    }));

    let mut generated = valid;
    generated.graph.nodes.insert(
        1,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    generated.root.set_sequence(Some(SequenceExpr::Generate {
        from: None,
        to: 0,
        item: 1,
    }));
    assert!(validate(&generated).iter().any(|issue| {
        issue
            .message
            .contains("cannot iterate a generated sequence")
    }));
}

#[test]
fn rejects_inconsistent_deserialized_group_alternatives() {
    let mut project = valid_project();
    let SchemaKind::Group { alternatives, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    *alternatives = vec![ir::GroupAlternative {
        name: "broken".into(),
        members: vec!["missing".into()],
        required: vec!["missing".into()],
    }];

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema" && issue.message.contains("group alternative metadata")
    }));
}

#[test]
fn reports_dangling_references_paths_unknown_functions_and_cycles() {
    let mut project = valid_project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "mystery".into(),
            args: vec![99],
        },
    );
    project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![2],
        },
    );
    project.graph.nodes.insert(
        3,
        Node::SourceField {
            frame: None,
            path: vec!["missing".into()],
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("unused".into()),
        },
    );
    project.root.set_source(None);
    project.root.filter = Some(88);
    project.root.group_by = Some(89);
    project.root.group_starting_with = Some(92);
    project.root.group_into_blocks = Some(93);
    project.root.sort_by = Some(90);
    project.root.take = Some(91);
    project.root.bindings.push(Binding {
        target_field: "missing".into(),
        node: 77,
    });
    project.root.children.push(Scope {
        target_field: "absent".into(),
        ..Scope::default()
    });

    let rendered: Vec<String> = validate(&project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect();
    for expected in [
        "unknown function `mystery`",
        "argument 0 references missing node 99",
        "cycle reaches node 2",
        "source field `missing` matches no scalar",
        "filter references missing node 88",
        "group-by key references missing node 89",
        "group-starting-with predicate references missing node 92",
        "group block size references missing node 93",
        "group-starting-with predicate has no iterated source",
        "group block size has no iterated source",
        "scope grouping modes are mutually exclusive",
        "sort key references missing node 90",
        "take count references missing node 91",
        "filter has no iterated source",
        "sort key has no iterated source",
        "take count has no iterated source",
        "binding target `missing` does not exist",
        "binding for `missing` references missing node 77",
        "target scope does not exist",
    ] {
        assert!(
            rendered.iter().any(|issue| issue.contains(expected)),
            "missing `{expected}` in {rendered:#?}"
        );
    }
}
