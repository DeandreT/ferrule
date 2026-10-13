use crate::owned_source_context::{
    DeclaredSource, OwnedSourceContext, RepetitionOrigin, RootEndpoint, RootReadError, RootScalar,
    SourceResolutionError, UnsupportedSourceDomain,
};
use ir::{DocumentMember, Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    Binding, Graph, NamedSource, Node, OwnedSourceField, OwnedSourceRef, Project, Scope,
    SourceOwner,
};
use std::collections::BTreeMap;

fn record_schema() -> SchemaNode {
    SchemaNode::group(
        "DeclaredRoot",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Nested",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
        ],
    )
}

fn project(source: SchemaNode, names: &[&str]) -> Project {
    Project {
        source,
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: names
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: "unused.json".into(),
                schema: record_schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}

fn field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> OwnedSourceField {
    OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|parts| parts.iter().map(|part| (*part).into()).collect()),
    }
}

fn primary<'a>(project: &'a Project, root: Option<&'a Instance>) -> RootEndpoint<'a> {
    RootEndpoint {
        declaration: DeclaredSource::Primary {
            schema: &project.source,
        },
        root,
    }
}

fn extra<'a>(project: &'a Project, index: usize, root: Option<&'a Instance>) -> RootEndpoint<'a> {
    RootEndpoint {
        declaration: DeclaredSource::Named {
            index,
            declaration: &project.extra_sources[index],
        },
        root,
    }
}

fn expect_scalar<'a, 'request>(
    actual: Result<RootScalar<'a, 'request>, RootReadError<'a, 'request>>,
    endpoint: RootEndpoint<'a>,
    request: &'request OwnedSourceField,
    value: &'a Value,
) {
    assert_eq!(
        actual,
        Ok(RootScalar {
            endpoint,
            request,
            value: Some(value)
        })
    );
    assert!(
        std::ptr::eq(actual.unwrap().value.unwrap(), value),
        "scalar must borrow the exact input leaf"
    );
}

#[test]
fn root_registry_keeps_equal_instances_and_declarations_independent() {
    let project = project(record_schema(), &["Ref", "primary"]);
    let input = group(vec![("Value", text("same"))]);
    let inputs = vec![
        ("Ref".into(), group(vec![("Value", text("same"))])),
        ("primary".into(), group(vec![("Value", text("same"))])),
    ];
    assert_eq!(input, inputs[0].1);
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    let requests = [
        field(SourceOwner::Primary, &["Value"], None),
        field(named("Ref"), &["Value"], None),
        field(named("primary"), &["Value"], None),
    ];
    let endpoints = [
        primary(&project, Some(&input)),
        extra(&project, 0, Some(&inputs[0].1)),
        extra(&project, 1, Some(&inputs[1].1)),
    ];
    for (request, endpoint) in requests.iter().zip(endpoints) {
        assert_eq!(context.resolve(&request.source.owner), Ok(endpoint));
        let value = endpoint
            .root
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar()
            .unwrap();
        expect_scalar(context.scalar(request), endpoint, request, value);
    }
    assert_ne!(endpoints[0], endpoints[1]);
    assert_ne!(endpoints[1], endpoints[2]);
    let DeclaredSource::Named { declaration, index } = endpoints[1].declaration else {
        panic!("named identity lost")
    };
    assert_eq!(index, 0);
    assert!(std::ptr::eq(declaration, &project.extra_sources[0]));
}

#[test]
fn root_scalar_never_recovers_named_owners_from_primary_fields() {
    let mut project = project(record_schema(), &["Ref", " Ref 雪 "]);
    let SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!()
    };
    children.extend([
        SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        SchemaNode::group(
            " Ref 雪 ",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
    ]);
    let input = group(vec![
        ("Value", text("primary")),
        ("Ref", group(vec![("Value", text("primary Ref field"))])),
        (
            " Ref 雪 ",
            group(vec![("Value", text("primary Unicode field"))]),
        ),
    ]);
    let inputs = vec![
        ("Ref".into(), group(vec![("Value", text("named Ref"))])),
        (
            " Ref 雪 ".into(),
            group(vec![("Value", text("named Unicode"))]),
        ),
    ];
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    for (request, endpoint, value) in [
        (
            field(SourceOwner::Primary, &["Ref", "Value"], None),
            primary(&project, Some(&input)),
            input
                .field("Ref")
                .unwrap()
                .field("Value")
                .unwrap()
                .as_scalar()
                .unwrap(),
        ),
        (
            field(named("Ref"), &["Value"], None),
            extra(&project, 0, Some(&inputs[0].1)),
            inputs[0].1.field("Value").unwrap().as_scalar().unwrap(),
        ),
        (
            field(SourceOwner::Primary, &[" Ref 雪 ", "Value"], None),
            primary(&project, Some(&input)),
            input
                .field(" Ref 雪 ")
                .unwrap()
                .field("Value")
                .unwrap()
                .as_scalar()
                .unwrap(),
        ),
        (
            field(named(" Ref 雪 "), &["Value"], None),
            extra(&project, 1, Some(&inputs[1].1)),
            inputs[1].1.field("Value").unwrap().as_scalar().unwrap(),
        ),
    ] {
        expect_scalar(context.scalar(&request), endpoint, &request, value);
    }
    let prefixed = field(named("Ref"), &["Ref", "Value"], None);
    assert_eq!(
        context.scalar(&prefixed),
        Err(RootReadError::UnknownScalarPath {
            declaration: extra(&project, 0, None).declaration,
            request: &prefixed,
            prefix_len: 1
        })
    );
}

#[test]
fn root_registry_preserves_literal_names_and_declaration_indexes() {
    let names = ["Ref", " Ref 雪 ", "ref", "primary", "雪", "é", "e\u{301}"];
    let mut project = project(SchemaNode::scalar("ScalarRoot", ScalarType::String), &names);
    for source in &mut project.extra_sources {
        source.schema = SchemaNode::scalar("ScalarRoot", ScalarType::String);
    }
    let input = text("primary");
    let inputs = names
        .iter()
        .rev()
        .map(|name| ((*name).into(), text(name)))
        .collect::<Vec<_>>();
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    for (index, name) in names.iter().enumerate() {
        let input_index = names.len() - index - 1;
        let request = field(named(name), &[], None);
        let endpoint = extra(&project, index, Some(&inputs[input_index].1));
        assert_eq!(context.resolve(&request.source.owner), Ok(endpoint));
        expect_scalar(
            context.scalar(&request),
            endpoint,
            &request,
            inputs[input_index].1.as_scalar().unwrap(),
        );
        let DeclaredSource::Named { declaration, .. } = endpoint.declaration else {
            unreachable!()
        };
        assert_eq!(declaration.name, *name);
        assert!(std::ptr::eq(
            declaration.name.as_ptr(),
            project.extra_sources[index].name.as_ptr()
        ));
    }
    for name in ["Ref 雪", " Ref 雪", "REF", "Primary", "", " "] {
        let request = field(named(name), &[], None);
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::Owner {
                request: &request,
                error: SourceResolutionError::UnknownOwner {
                    owner: &request.source.owner
                }
            })
        );
    }
}

#[test]
fn root_registry_distinguishes_unknown_and_ambiguous_declarations() {
    let project = project(record_schema(), &["Ref", "Other", "Ref"]);
    let inputs = vec![
        ("Ref".into(), group(vec![("Value", text("first"))])),
        ("Other".into(), group(vec![("Value", text("other"))])),
        ("Ref".into(), group(vec![("Value", text("second"))])),
        ("Ghost".into(), group(vec![("Value", text("host only"))])),
    ];
    let context = OwnedSourceContext::new(&project, None, &inputs);
    let duplicate = field(named("Ref"), &["Value"], None);
    assert_eq!(
        context.scalar(&duplicate),
        Err(RootReadError::Owner {
            request: &duplicate,
            error: SourceResolutionError::AmbiguousOwner {
                owner: &duplicate.source.owner,
                first_index: 0,
                second_index: 2
            }
        })
    );
    let unknown = field(named("Ghost"), &["Value"], None);
    assert_eq!(
        context.scalar(&unknown),
        Err(RootReadError::Owner {
            request: &unknown,
            error: SourceResolutionError::UnknownOwner {
                owner: &unknown.source.owner
            }
        })
    );
    let other = field(named("Other"), &["Value"], None);
    expect_scalar(
        context.scalar(&other),
        extra(&project, 1, Some(&inputs[1].1)),
        &other,
        inputs[1].1.field("Value").unwrap().as_scalar().unwrap(),
    );
}

#[test]
fn root_registry_rejects_duplicate_host_bindings_without_erasing_index() {
    let project = project(record_schema(), &["Ref", "Other"]);
    let inputs = vec![
        ("Ref".into(), group(vec![("Value", text("first"))])),
        ("Unrelated".into(), text("ignored")),
        ("Ref".into(), group(vec![("Value", text("second"))])),
    ];
    let context = OwnedSourceContext::new(&project, None, &inputs);
    let request = field(named("Ref"), &["Value"], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::Owner {
            request: &request,
            error: SourceResolutionError::AmbiguousInput {
                declaration: extra(&project, 0, None).declaration,
                first_index: 0,
                second_index: 2
            }
        })
    );
    let missing = field(named("Other"), &["Value"], None);
    assert_eq!(
        context.resolve(&missing.source.owner),
        Ok(extra(&project, 1, None))
    );
    assert_eq!(
        context.scalar(&missing),
        Err(RootReadError::MissingEndpoint {
            declaration: extra(&project, 1, None).declaration,
            request: &missing
        })
    );
}

#[test]
fn root_scalar_distinguishes_supplied_null_missing_field_and_missing_endpoint() {
    let project = project(record_schema(), &["Null", "NoField", "Missing"]);
    let input = group(vec![("Value", Instance::Scalar(Value::Null))]);
    let inputs = vec![
        (
            "Null".into(),
            group(vec![("Value", Instance::Scalar(Value::Null))]),
        ),
        ("NoField".into(), group(vec![])),
    ];
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    for (request, endpoint) in [
        (
            field(SourceOwner::Primary, &["Value"], None),
            primary(&project, Some(&input)),
        ),
        (
            field(named("Null"), &["Value"], None),
            extra(&project, 0, Some(&inputs[0].1)),
        ),
    ] {
        let value = endpoint
            .root
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar()
            .unwrap();
        assert_eq!(value, &Value::Null);
        expect_scalar(context.scalar(&request), endpoint, &request, value);
    }
    let no_field = field(named("NoField"), &["Value"], None);
    assert_eq!(
        context.scalar(&no_field),
        Ok(RootScalar {
            endpoint: extra(&project, 1, Some(&inputs[1].1)),
            request: &no_field,
            value: None
        })
    );
    let missing = field(named("Missing"), &["Value"], None);
    assert_eq!(
        context.scalar(&missing),
        Err(RootReadError::MissingEndpoint {
            declaration: extra(&project, 2, None).declaration,
            request: &missing
        })
    );
    let no_primary = OwnedSourceContext::new(&project, None, &inputs);
    let request = field(SourceOwner::Primary, &["Value"], None);
    assert_eq!(
        no_primary.scalar(&request),
        Err(RootReadError::MissingEndpoint {
            declaration: primary(&project, None).declaration,
            request: &request
        })
    );
}

#[test]
fn root_scalar_accepts_scalar_roots_and_retains_empty_frame_choice() {
    let mut project = project(
        SchemaNode::scalar("PrimaryLabel", ScalarType::String),
        &[" Ref 雪 "],
    );
    project.extra_sources[0].schema = SchemaNode::scalar("NamedLabel", ScalarType::String);
    let input = Instance::Scalar(Value::Null);
    let inputs = vec![(" Ref 雪 ".into(), text("named scalar"))];
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    for frame in [None, Some(&[][..])] {
        let p = field(SourceOwner::Primary, &[], frame);
        let n = field(named(" Ref 雪 "), &[], frame);
        expect_scalar(
            context.scalar(&p),
            primary(&project, Some(&input)),
            &p,
            input.as_scalar().unwrap(),
        );
        expect_scalar(
            context.scalar(&n),
            extra(&project, 0, Some(&inputs[0].1)),
            &n,
            inputs[0].1.as_scalar().unwrap(),
        );
        assert_eq!(
            context.scalar(&p).unwrap().request.frame.is_some(),
            frame.is_some()
        );
    }
    let label_prefix = field(SourceOwner::Primary, &["PrimaryLabel"], None);
    assert_eq!(
        context.scalar(&label_prefix),
        Err(RootReadError::NonScalarPath {
            declaration: primary(&project, None).declaration,
            request: &label_prefix,
            prefix_len: 0
        })
    );
}

#[test]
fn root_scalar_refuses_nonempty_frames_before_reading_another_owner() {
    let project = project(record_schema(), &["Ref"]);
    let input = group(vec![("Value", text("primary"))]);
    let inputs = vec![("Ref".into(), group(vec![("Value", text("named"))]))];
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    for (owner, declaration) in [
        (SourceOwner::Primary, primary(&project, None).declaration),
        (named("Ref"), extra(&project, 0, None).declaration),
    ] {
        let request = field(owner, &["Value"], Some(&["Nested"]));
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::NonRootFrame {
                declaration,
                request: &request
            })
        );
    }
}

#[test]
fn root_scalar_reads_absolute_nonrepeating_paths_without_fallback() {
    let project = project(record_schema(), &["Ref"]);
    let input = group(vec![
        ("Value", text("outer cannot rescue missing child")),
        ("Nested", group(vec![])),
    ]);
    let inputs = vec![(
        "Ref".into(),
        group(vec![(
            "Nested",
            group(vec![("Value", text("named nested"))]),
        )]),
    )];
    let context = OwnedSourceContext::new(&project, Some(&input), &inputs);
    let p = field(SourceOwner::Primary, &["Nested", "Value"], None);
    assert_eq!(
        context.scalar(&p),
        Ok(RootScalar {
            endpoint: primary(&project, Some(&input)),
            request: &p,
            value: None
        })
    );
    let n = field(named("Ref"), &["Nested", "Value"], Some(&[]));
    expect_scalar(
        context.scalar(&n),
        extra(&project, 0, Some(&inputs[0].1)),
        &n,
        inputs[0]
            .1
            .field("Nested")
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar()
            .unwrap(),
    );
    let unknown = field(SourceOwner::Primary, &["DeclaredRoot", "Value"], None);
    assert_eq!(
        context.scalar(&unknown),
        Err(RootReadError::UnknownScalarPath {
            declaration: primary(&project, None).declaration,
            request: &unknown,
            prefix_len: 1
        })
    );
    for request in [
        field(SourceOwner::Primary, &[], None),
        field(SourceOwner::Primary, &["Nested"], None),
    ] {
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::NonScalarPath {
                declaration: primary(&project, None).declaration,
                request: &request,
                prefix_len: request.source.path.len()
            })
        );
    }
}

#[test]
fn root_scalar_refuses_repeating_schema_before_missing_data() {
    for (schema, path, prefix_len) in [
        (
            SchemaNode::scalar("RepeatedRoot", ScalarType::String).repeating(),
            vec![],
            0,
        ),
        (
            SchemaNode::group(
                "Root",
                vec![SchemaNode::scalar("Value", ScalarType::String).repeating()],
            ),
            vec!["Value"],
            1,
        ),
        (
            SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            vec!["Rows", "Value"],
            1,
        ),
    ] {
        let project = project(schema, &[]);
        let input = group(vec![]);
        let context = OwnedSourceContext::new(&project, Some(&input), &[]);
        let request = field(SourceOwner::Primary, &path, None);
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::RepeatingPath {
                declaration: primary(&project, None).declaration,
                request: &request,
                prefix_len,
                origin: RepetitionOrigin::Schema
            })
        );
    }
    let mut named_project = project(record_schema(), &["Ref"]);
    named_project.extra_sources[0].schema =
        SchemaNode::scalar("NamedRoot", ScalarType::String).repeating();
    let inputs = vec![(
        "Ref".into(),
        text("a scalar value cannot erase declared repetition"),
    )];
    let context = OwnedSourceContext::new(&named_project, None, &inputs);
    let request = field(named("Ref"), &[], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::RepeatingPath {
            declaration: extra(&named_project, 0, None).declaration,
            request: &request,
            prefix_len: 0,
            origin: RepetitionOrigin::Schema
        })
    );
}

#[test]
fn root_scalar_refuses_repeating_instances_without_first_item_unwrap() {
    for input in [
        Instance::Repeated(vec![]),
        Instance::Repeated(vec![text("first must not be used"), text("second")]),
    ] {
        let project = project(SchemaNode::scalar("Root", ScalarType::String), &[]);
        let context = OwnedSourceContext::new(&project, Some(&input), &[]);
        let request = field(SourceOwner::Primary, &[], None);
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::RepeatingPath {
                declaration: primary(&project, None).declaration,
                request: &request,
                prefix_len: 0,
                origin: RepetitionOrigin::Instance
            })
        );
    }
    let project = project(record_schema(), &[]);
    let input = group(vec![(
        "Nested",
        Instance::Repeated(vec![group(vec![("Value", text("first must not be used"))])]),
    )]);
    let context = OwnedSourceContext::new(&project, Some(&input), &[]);
    let request = field(SourceOwner::Primary, &["Nested", "Value"], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::RepeatingPath {
            declaration: primary(&project, None).declaration,
            request: &request,
            prefix_len: 1,
            origin: RepetitionOrigin::Instance
        })
    );
}

#[test]
fn root_scalar_refuses_document_mapped_sequence_and_dynamic_provenance() {
    for (input, domain) in [
        (
            Instance::DocumentSet(vec![]),
            UnsupportedSourceDomain::DocumentSet,
        ),
        (
            Instance::DocumentSet(vec![
                DocumentMember::new_source(
                    "member.json",
                    "/host/member.json",
                    text("must not unwrap"),
                )
                .unwrap(),
            ]),
            UnsupportedSourceDomain::DocumentSet,
        ),
        (
            Instance::MappedSequence(vec![]),
            UnsupportedSourceDomain::MappedSequence,
        ),
        (
            Instance::MappedSequence(vec![text("must not unwrap")]),
            UnsupportedSourceDomain::MappedSequence,
        ),
    ] {
        let project = project(SchemaNode::scalar("Root", ScalarType::String), &[]);
        let context = OwnedSourceContext::new(&project, Some(&input), &[]);
        let request = field(SourceOwner::Primary, &[], None);
        assert_eq!(
            context.scalar(&request),
            Err(RootReadError::UnsupportedProvenance {
                declaration: primary(&project, None).declaration,
                request: &request,
                prefix_len: 0,
                domain
            })
        );
    }
    let ordinary = project(record_schema(), &[]);
    let input = group(vec![(
        "Value",
        Instance::DocumentSet(vec![
            DocumentMember::new("nested.json", text("must not unwrap")).unwrap(),
        ]),
    )]);
    let context = OwnedSourceContext::new(&ordinary, Some(&input), &[]);
    let request = field(SourceOwner::Primary, &["Value"], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::UnsupportedProvenance {
            declaration: primary(&ordinary, None).declaration,
            request: &request,
            prefix_len: 1,
            domain: UnsupportedSourceDomain::DocumentSet
        })
    );
    let mut dynamic = project(record_schema(), &["Ref"]);
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 91,
        iteration: vec![],
    });
    let inputs = vec![(
        "Ref".into(),
        group(vec![(
            "Value",
            text("provided once is not a dynamic cohort"),
        )]),
    )];
    let context = OwnedSourceContext::new(&dynamic, None, &inputs);
    let request = field(named("Ref"), &["Value"], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::UnsupportedProvenance {
            declaration: extra(&dynamic, 0, None).declaration,
            request: &request,
            prefix_len: 0,
            domain: UnsupportedSourceDomain::DynamicSource
        })
    );
}

#[test]
fn root_scalar_reports_complete_invalid_instance_shapes() {
    let project = project(record_schema(), &[]);
    let input = text("not a declared group");
    let context = OwnedSourceContext::new(&project, Some(&input), &[]);
    let request = field(SourceOwner::Primary, &["Value"], None);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::InvalidInstanceShape {
            declaration: primary(&project, None).declaration,
            request: &request,
            prefix_len: 0,
            value: &input
        })
    );
    let input = group(vec![("Value", group(vec![]))]);
    let context = OwnedSourceContext::new(&project, Some(&input), &[]);
    assert_eq!(
        context.scalar(&request),
        Err(RootReadError::InvalidInstanceShape {
            declaration: primary(&project, None).declaration,
            request: &request,
            prefix_len: 1,
            value: input.field("Value").unwrap()
        })
    );
}

#[test]
fn test_scoped_owner_context_keeps_production_refusals_and_legacy_outputs() {
    let mut project = project(record_schema(), &["Ref"]);
    project.graph.nodes.insert(
        7,
        Node::OwnedSourceField(field(named("Ref"), &["Value"], None)),
    );
    project.root.bindings.push(Binding {
        target_field: "Value".into(),
        node: 7,
    });
    let expected = crate::EngineError::UnsupportedSourceOwnership {
        node: 7,
        function: None,
        kind: "owned_source_field",
    };
    assert_eq!(
        crate::check_source_ownership_capability(&project, None),
        Err(expected)
    );
    let input = group(vec![("Value", text("legacy primary"))]);
    assert_eq!(
        crate::run(&project, &input),
        Err(crate::EngineError::UnsupportedSourceOwnership {
            node: 7,
            function: None,
            kind: "owned_source_field"
        })
    );
    project.graph.nodes.insert(
        8,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    project.root.bindings[0].node = 8;
    assert_eq!(
        crate::check_source_ownership_capability(&project, None),
        Ok(())
    );
    assert_eq!(
        crate::run_with_sources(
            &project,
            &input,
            vec![("Ref".into(), group(vec![("Value", text("legacy named"))]))]
        ),
        Ok(group(vec![("Value", text("legacy primary"))]))
    );
}
