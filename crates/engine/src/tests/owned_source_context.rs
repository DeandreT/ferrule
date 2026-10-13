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

use crate::owned_source_context::{
    ContextError, ContextFault, ContextRequest, FrameProvenance, FrameScalar, PositionPhase,
    SourceFrame, SourcePosition, SourceWalk,
};
use mapping::OwnedPosition;
use std::num::NonZeroUsize;
use std::rc::Rc;

fn nz604(index: usize) -> NonZeroUsize {
    NonZeroUsize::new(index).unwrap()
}

fn schema604() -> SchemaNode {
    SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn project604(names: &[&str]) -> Project {
    let mut p = project(schema604(), names);
    for source in &mut p.extra_sources {
        source.schema = schema604();
    }
    p
}

fn collection604(owner: SourceOwner, path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

fn frame604<'a>(
    endpoint: RootEndpoint<'a>,
    path: &[&str],
    item: &'a Instance,
    parent: Option<Rc<SourceFrame<'a>>>,
    index: usize,
) -> Rc<SourceFrame<'a>> {
    Rc::new(SourceFrame {
        endpoint,
        collection: path.iter().map(|part| (*part).into()).collect(),
        item,
        parent,
        structural: nz604(index),
        selection: nz604(index),
        output: None,
        provenance: FrameProvenance::Basic,
    })
}

fn expect_walk604<'a, 'request>(
    actual: Result<Vec<SourceWalk<'a>>, ContextError<'a, 'request>>,
    expected: Vec<SourceWalk<'a>>,
) {
    assert_eq!(actual, Ok(expected.clone()));
    for (actual, expected) in actual.unwrap().iter().zip(&expected) {
        assert!(std::ptr::eq(actual.item, expected.item));
        for (actual, expected) in actual.frames.iter().zip(&expected.frames) {
            assert!(std::ptr::eq(actual.item, expected.item));
        }
    }
}

#[test]
fn owned_walks_keep_equal_paths_different_counts_and_exact_endpoint_names() {
    let p = project604(&[" Ref 雪 ", "primary"]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Value", text("p1"))]),
            group(vec![("Value", text("p2"))]),
        ]),
    )]);
    let inputs = vec![
        (
            "primary".into(),
            group(vec![("Rows", Instance::Repeated(vec![]))]),
        ),
        (
            " Ref 雪 ".into(),
            group(vec![(
                "Rows",
                Instance::Repeated(vec![group(vec![("Value", text("n1"))])]),
            )]),
        ),
    ];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let primary_items = input.field("Rows").unwrap().as_repeated().unwrap();
    let named_items = inputs[1].1.field("Rows").unwrap().as_repeated().unwrap();
    let pe = primary(&p, Some(&input));
    let ne = extra(&p, 0, Some(&inputs[1].1));
    let ps = collection604(SourceOwner::Primary, &["Rows"]);
    let ns = collection604(named(" Ref 雪 "), &["Rows"]);
    expect_walk604(
        context.walk(&ps, &[]),
        vec![
            SourceWalk {
                endpoint: pe,
                frames: vec![frame604(pe, &["Rows"], &primary_items[0], None, 1)],
                item: &primary_items[0],
                introduced: Some(0),
            },
            SourceWalk {
                endpoint: pe,
                frames: vec![frame604(pe, &["Rows"], &primary_items[1], None, 2)],
                item: &primary_items[1],
                introduced: Some(0),
            },
        ],
    );
    expect_walk604(
        context.walk(&ns, &[]),
        vec![SourceWalk {
            endpoint: ne,
            frames: vec![frame604(ne, &["Rows"], &named_items[0], None, 1)],
            item: &named_items[0],
            introduced: Some(0),
        }],
    );
    let empty = collection604(named("primary"), &["Rows"]);
    assert_eq!(context.walk(&empty, &[]), Ok(vec![]));
    let unknown = collection604(named("Ref 雪"), &["Rows"]);
    assert_eq!(
        context.walk(&unknown, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&unknown),
            fault: ContextFault::Owner(SourceResolutionError::UnknownOwner {
                owner: &unknown.owner
            })
        })
    );
}

#[test]
fn owned_walks_preserve_repeating_root_ancestry_and_pinned_parent_occurrences() {
    let mut p = project604(&[]);
    p.source.repeating = true;
    let input = Instance::Repeated(vec![
        group(vec![
            ("Value", text("parent1")),
            (
                "Rows",
                Instance::Repeated(vec![
                    group(vec![("Value", text("a"))]),
                    group(vec![("Value", text("b"))]),
                ]),
            ),
        ]),
        group(vec![
            ("Value", text("parent2")),
            (
                "Rows",
                Instance::Repeated(vec![group(vec![("Value", text("c"))])]),
            ),
        ]),
    ]);
    let roots = input.as_repeated().unwrap();
    let a = roots[0].field("Rows").unwrap().as_repeated().unwrap();
    let b = roots[1].field("Rows").unwrap().as_repeated().unwrap();
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let endpoint = primary(&p, Some(&input));
    let first_parent = frame604(endpoint, &[], &roots[0], None, 1);
    let second_parent = frame604(endpoint, &[], &roots[1], None, 2);
    let source = collection604(SourceOwner::Primary, &["Rows"]);
    let expected = vec![
        SourceWalk {
            endpoint,
            frames: vec![
                Rc::clone(&first_parent),
                frame604(
                    endpoint,
                    &["Rows"],
                    &a[0],
                    Some(Rc::clone(&first_parent)),
                    1,
                ),
            ],
            item: &a[0],
            introduced: Some(1),
        },
        SourceWalk {
            endpoint,
            frames: vec![
                Rc::clone(&first_parent),
                frame604(
                    endpoint,
                    &["Rows"],
                    &a[1],
                    Some(Rc::clone(&first_parent)),
                    2,
                ),
            ],
            item: &a[1],
            introduced: Some(1),
        },
        SourceWalk {
            endpoint,
            frames: vec![
                Rc::clone(&second_parent),
                frame604(
                    endpoint,
                    &["Rows"],
                    &b[0],
                    Some(Rc::clone(&second_parent)),
                    1,
                ),
            ],
            item: &b[0],
            introduced: Some(1),
        },
    ];
    expect_walk604(context.walk(&source, &[]), expected);
    let walked = context.walk(&source, &[]).unwrap();
    let pinned = field(SourceOwner::Primary, &["Value"], Some(&[]));
    assert_eq!(
        context.framed_scalar(&pinned, &walked[2].frames),
        Ok(FrameScalar {
            endpoint,
            request: &pinned,
            frame: Some(Rc::clone(&walked[2].frames[0])),
            value: roots[1].field("Value").unwrap().as_scalar()
        })
    );
    let position = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: Some(vec![]),
    };
    assert_eq!(
        context.position(&position, &walked[2].frames, PositionPhase::Structural),
        Ok(SourcePosition {
            endpoint,
            request: &position,
            frame: Rc::clone(&walked[2].frames[0]),
            phase: PositionPhase::Structural,
            index: nz604(2)
        })
    );
}

#[test]
fn owned_nested_same_name_frames_keep_outer_pins_and_missing_inner_values() {
    let p = project604(&[]);
    let input = group(vec![
        ("Value", text("root rescue forbidden")),
        (
            "Rows",
            Instance::Repeated(vec![
                group(vec![
                    ("Value", text("outer1")),
                    (
                        "Rows",
                        Instance::Repeated(vec![
                            group(vec![("Value", text("inner1"))]),
                            group(vec![]),
                        ]),
                    ),
                ]),
                group(vec![
                    ("Value", text("outer2")),
                    (
                        "Rows",
                        Instance::Repeated(vec![group(vec![(
                            "Value",
                            Instance::Scalar(Value::Null),
                        )])]),
                    ),
                ]),
            ]),
        ),
    ]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Rows", "Rows"]);
    let walked = context.walk(&source, &[]).unwrap();
    assert_eq!(walked.len(), 3);
    let endpoint = primary(&p, Some(&input));
    let parent_items = input.field("Rows").unwrap().as_repeated().unwrap();
    let children = parent_items[0]
        .field("Rows")
        .unwrap()
        .as_repeated()
        .unwrap();
    let expected_parent = frame604(endpoint, &["Rows"], &parent_items[0], None, 1);
    expect_walk604(
        Ok(vec![walked[1].clone()]),
        vec![SourceWalk {
            endpoint,
            frames: vec![
                Rc::clone(&expected_parent),
                frame604(
                    endpoint,
                    &["Rows", "Rows"],
                    &children[1],
                    Some(expected_parent),
                    2,
                ),
            ],
            item: &children[1],
            introduced: Some(1),
        }],
    );
    let inner = field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None);
    assert_eq!(
        context.framed_scalar(&inner, &walked[1].frames),
        Ok(FrameScalar {
            endpoint,
            request: &inner,
            frame: Some(Rc::clone(&walked[1].frames[1])),
            value: None
        })
    );
    let outer = field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"]));
    assert_eq!(
        context.framed_scalar(&outer, &walked[1].frames),
        Ok(FrameScalar {
            endpoint,
            request: &outer,
            frame: Some(Rc::clone(&walked[1].frames[0])),
            value: parent_items[0].field("Value").unwrap().as_scalar()
        })
    );
    assert_eq!(
        context.framed_scalar(&inner, &walked[2].frames),
        Ok(FrameScalar {
            endpoint,
            request: &inner,
            frame: Some(Rc::clone(&walked[2].frames[1])),
            value: walked[2].item.field("Value").unwrap().as_scalar()
        })
    );
    assert_eq!(
        walked[2].item.field("Value").unwrap().as_scalar(),
        Some(&Value::Null)
    );
    let outer_crossing = field(
        SourceOwner::Primary,
        &["Rows", "Rows", "Value"],
        Some(&["Rows"]),
    );
    assert_eq!(
        context.framed_scalar(&outer_crossing, &walked[1].frames),
        Err(ContextError {
            request: ContextRequest::Scalar(&outer_crossing),
            fault: ContextFault::UniteratedCollection {
                declaration: endpoint.declaration,
                prefix_len: 2
            }
        })
    );
}

#[test]
fn owned_frame_lookup_has_no_suffix_cross_owner_or_root_first_rescue() {
    let p = project604(&["Ref"]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Value", text("first root row"))]),
            group(vec![]),
        ]),
    )]);
    let inputs = vec![(
        "Ref".into(),
        group(vec![(
            "Rows",
            Instance::Repeated(vec![group(vec![("Value", text("named rescue forbidden"))])]),
        )]),
    )];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let ps = collection604(SourceOwner::Primary, &["Rows"]);
    let ns = collection604(named("Ref"), &["Rows"]);
    let pw = context.walk(&ps, &[]).unwrap();
    let nw = context.walk(&ns, &[]).unwrap();
    let request = field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"]));
    let mut active = pw[1].frames.clone();
    active.extend(nw[0].frames.iter().map(Rc::clone));
    assert_eq!(
        context.framed_scalar(&request, &active),
        Ok(FrameScalar {
            endpoint: primary(&p, Some(&input)),
            request: &request,
            frame: Some(Rc::clone(&pw[1].frames[0])),
            value: None
        })
    );
    assert_eq!(
        context.framed_scalar(&request, &nw[0].frames),
        Err(ContextError {
            request: ContextRequest::Scalar(&request),
            fault: ContextFault::InactiveFrame {
                declaration: primary(&p, None).declaration
            }
        })
    );
    let pin = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: Some(vec!["Rows".into()]),
    };
    let nested_input = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![(
            "Rows",
            Instance::Repeated(vec![group(vec![])]),
        )])]),
    )]);
    let nested_context = OwnedSourceContext::new(&p, Some(&nested_input), &[]);
    let nested_source = collection604(SourceOwner::Primary, &["Rows", "Rows"]);
    let nested = nested_context.walk(&nested_source, &[]).unwrap();
    assert_eq!(
        nested_context.position(
            &pin,
            &[Rc::clone(&nested[0].frames[1])],
            PositionPhase::Structural
        ),
        Err(ContextError {
            request: ContextRequest::Position(&pin),
            fault: ContextFault::InactiveFrame {
                declaration: primary(&p, None).declaration
            }
        })
    );
}

#[test]
fn owned_pinned_reads_require_absolute_paths_inside_the_exact_frame() {
    let p = project604(&[]);
    let input = group(vec![
        ("Value", text("root")),
        (
            "Rows",
            Instance::Repeated(vec![group(vec![("Value", text("row"))])]),
        ),
    ]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Rows"]);
    let walked = context.walk(&source, &[]).unwrap();
    let request = field(SourceOwner::Primary, &["Value"], Some(&["Rows"]));
    assert_eq!(
        context.framed_scalar(&request, &walked[0].frames),
        Err(ContextError {
            request: ContextRequest::Scalar(&request),
            fault: ContextFault::PathOutsideFrame {
                frame: Rc::clone(&walked[0].frames[0])
            }
        })
    );
    let unpinned = field(SourceOwner::Primary, &["Rows", "Value"], None);
    assert_eq!(
        context.framed_scalar(&unpinned, &[]),
        Err(ContextError {
            request: ContextRequest::Scalar(&unpinned),
            fault: ContextFault::UniteratedCollection {
                declaration: primary(&p, None).declaration,
                prefix_len: 1
            }
        })
    );
    let empty_pin = field(SourceOwner::Primary, &["Value"], Some(&[]));
    assert_eq!(
        context.framed_scalar(&empty_pin, &[]),
        Ok(FrameScalar {
            endpoint: primary(&p, Some(&input)),
            request: &empty_pin,
            frame: None,
            value: input.field("Value").unwrap().as_scalar()
        })
    );
    expect_scalar(
        context.scalar(&empty_pin),
        primary(&p, Some(&input)),
        &empty_pin,
        input.field("Value").unwrap().as_scalar().unwrap(),
    );
}

#[test]
fn owned_walk_identity_reentry_does_not_duplicate_or_renumber_outer_frames() {
    let p = project604(&["Ref"]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![]), group(vec![])]),
    )]);
    let inputs = vec![(
        "Ref".into(),
        group(vec![("Rows", Instance::Repeated(vec![group(vec![])]))]),
    )];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let source = collection604(SourceOwner::Primary, &["Rows"]);
    let named_source = collection604(named("Ref"), &["Rows"]);
    let primary_walk = context.walk(&source, &[]).unwrap();
    let named_walk = context.walk(&named_source, &[]).unwrap();
    let mut active = primary_walk[1].frames.clone();
    active.extend(named_walk[0].frames.iter().map(Rc::clone));
    let identity = context.walk(&source, &active).unwrap();
    assert_eq!(
        identity,
        vec![SourceWalk {
            endpoint: primary(&p, Some(&input)),
            frames: active.clone(),
            item: primary_walk[1].item,
            introduced: None
        }]
    );
    assert_eq!(
        identity[0].clone().sorted(nz604(8)).produced(nz604(9)),
        identity[0]
    );
    let query = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: None,
    };
    assert_eq!(
        context.position(&query, &identity[0].frames, PositionPhase::Selection),
        Ok(SourcePosition {
            endpoint: primary(&p, Some(&input)),
            request: &query,
            frame: Rc::clone(&active[0]),
            phase: PositionPhase::Selection,
            index: nz604(2)
        })
    );
}

#[test]
fn owned_walk_root_identity_keeps_present_null_and_explicit_empty_collection() {
    let p = project(SchemaNode::scalar("Root", ScalarType::String), &[]);
    let input = Instance::Scalar(Value::Null);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &[]);
    let endpoint = primary(&p, Some(&input));
    expect_walk604(
        context.walk(&source, &[]),
        vec![SourceWalk {
            endpoint,
            frames: vec![frame604(endpoint, &[], &input, None, 1)],
            item: &input,
            introduced: Some(0),
        }],
    );
    let walked = context.walk(&source, &[]).unwrap();
    let request = field(SourceOwner::Primary, &[], Some(&[]));
    assert_eq!(
        context.framed_scalar(&request, &walked[0].frames),
        Ok(FrameScalar {
            endpoint,
            request: &request,
            frame: Some(Rc::clone(&walked[0].frames[0])),
            value: input.as_scalar()
        })
    );
    let query = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: Some(vec![]),
    };
    assert_eq!(
        context.position(&query, &walked[0].frames, PositionPhase::Structural),
        Ok(SourcePosition {
            endpoint,
            request: &query,
            frame: Rc::clone(&walked[0].frames[0]),
            phase: PositionPhase::Structural,
            index: nz604(1)
        })
    );
}

#[test]
fn owned_walks_distinguish_missing_endpoints_null_collections_and_inactive_frames() {
    let p = project604(&["Missing", "Null"]);
    let input = group(vec![]);
    let inputs = vec![("Null".into(), Instance::Scalar(Value::Null))];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let absent = collection604(SourceOwner::Primary, &["Rows"]);
    assert_eq!(context.walk(&absent, &[]), Ok(vec![]));
    let missing = collection604(named("Missing"), &["Rows"]);
    assert_eq!(
        context.walk(&missing, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&missing),
            fault: ContextFault::MissingEndpoint {
                declaration: extra(&p, 0, None).declaration
            }
        })
    );
    let null = collection604(named("Null"), &["Rows"]);
    assert_eq!(
        context.walk(&null, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&null),
            fault: ContextFault::InvalidShape {
                declaration: extra(&p, 1, None).declaration,
                prefix_len: 0,
                value: &inputs[0].1
            }
        })
    );
    let null_input = group(vec![("Rows", Instance::Scalar(Value::Null))]);
    let null_context = OwnedSourceContext::new(&p, Some(&null_input), &[]);
    assert_eq!(
        null_context.walk(&absent, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&absent),
            fault: ContextFault::InvalidShape {
                declaration: primary(&p, None).declaration,
                prefix_len: 1,
                value: null_input.field("Rows").unwrap()
            }
        })
    );
    for collection in [None, Some(vec![]), Some(vec!["Rows".into()])] {
        let query = OwnedPosition {
            owner: SourceOwner::Primary,
            collection,
        };
        assert_eq!(
            context.position(&query, &[], PositionPhase::Structural),
            Err(ContextError {
                request: ContextRequest::Position(&query),
                fault: ContextFault::InactiveFrame {
                    declaration: primary(&p, None).declaration
                }
            })
        );
    }
}

#[test]
fn owned_positions_keep_structural_selection_output_and_outer_phases_separate() {
    let p = project604(&[]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![(
            "Rows",
            Instance::Repeated(vec![group(vec![]), group(vec![]), group(vec![])]),
        )])]),
    )]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Rows", "Rows"]);
    let walked = context.walk(&source, &[]).unwrap();
    let candidate = walked[2].clone().sorted(nz604(2));
    let query = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: None,
    };
    let endpoint = primary(&p, Some(&input));
    for (phase, index) in [
        (PositionPhase::Structural, 3),
        (PositionPhase::Selection, 2),
    ] {
        assert_eq!(
            context.position(&query, &candidate.frames, phase),
            Ok(SourcePosition {
                endpoint,
                request: &query,
                frame: Rc::clone(&candidate.frames[1]),
                phase,
                index: nz604(index)
            })
        );
    }
    assert_eq!(
        context.position(&query, &candidate.frames, PositionPhase::Output),
        Err(ContextError {
            request: ContextRequest::Position(&query),
            fault: ContextFault::PhaseUnavailable {
                frame: Rc::clone(&candidate.frames[1]),
                phase: PositionPhase::Output
            }
        })
    );
    let produced = candidate.produced(nz604(1));
    assert_eq!(
        context.position(&query, &produced.frames, PositionPhase::Output),
        Ok(SourcePosition {
            endpoint,
            request: &query,
            frame: Rc::clone(&produced.frames[1]),
            phase: PositionPhase::Output,
            index: nz604(1)
        })
    );
    assert!(Rc::ptr_eq(&walked[2].frames[0], &produced.frames[0]));
    assert!(std::ptr::eq(walked[2].item, produced.item));
    let filtered_only = walked[2].clone().produced(nz604(1));
    assert_eq!(
        context.position(&query, &filtered_only.frames, PositionPhase::Selection),
        Ok(SourcePosition {
            endpoint,
            request: &query,
            frame: Rc::clone(&filtered_only.frames[1]),
            phase: PositionPhase::Selection,
            index: nz604(3)
        })
    );
}

#[test]
fn owned_nested_walks_do_not_restart_root_for_an_absent_selected_collection() {
    let p = project604(&["Ref"]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![(
                "Rows",
                Instance::Repeated(vec![group(vec![(
                    "Value",
                    text("first row rescue forbidden"),
                )])]),
            )]),
            group(vec![]),
        ]),
    )]);
    let inputs = vec![(
        "Ref".into(),
        group(vec![(
            "Rows",
            Instance::Repeated(vec![group(vec![(
                "Rows",
                Instance::Repeated(vec![group(vec![])]),
            )])]),
        )]),
    )];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let outer = collection604(SourceOwner::Primary, &["Rows"]);
    let nested = collection604(SourceOwner::Primary, &["Rows", "Rows"]);
    let named_source = collection604(named("Ref"), &["Rows"]);
    let primary_walk = context.walk(&outer, &[]).unwrap();
    let named_walk = context.walk(&named_source, &[]).unwrap();
    let mut active = primary_walk[1].frames.clone();
    active.extend(named_walk[0].frames.iter().map(Rc::clone));
    assert_eq!(context.walk(&nested, &active), Ok(vec![]));
}

#[test]
fn owned_frames_reject_foreign_bindings_forged_ancestry_and_nonbasic_origins() {
    let p = project604(&[]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Value", text("same"))]),
            group(vec![("Value", text("same"))]),
        ]),
    )]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Rows"]);
    let walked = context.walk(&source, &[]).unwrap();
    let query = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: Some(vec!["Rows".into()]),
    };
    for provenance in [
        FrameProvenance::Generated,
        FrameProvenance::Function,
        FrameProvenance::Join,
        FrameProvenance::Document,
        FrameProvenance::Dynamic,
    ] {
        let mut metadata = (*walked[0].frames[0]).clone();
        metadata.provenance = provenance;
        let frame = Rc::new(metadata);
        assert_eq!(
            context.position(&query, &[Rc::clone(&frame)], PositionPhase::Structural),
            Err(ContextError {
                request: ContextRequest::Position(&query),
                fault: ContextFault::UnsupportedFrame { frame }
            })
        );
    }
    let mut metadata = (*walked[0].frames[0]).clone();
    metadata.item = walked[1].item;
    let forged = Rc::new(metadata);
    assert_eq!(
        context.position(&query, &[Rc::clone(&forged)], PositionPhase::Structural),
        Err(ContextError {
            request: ContextRequest::Position(&query),
            fault: ContextFault::InvalidFrame { frame: forged }
        })
    );
    let equal_other_input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Value", text("same"))]),
            group(vec![("Value", text("same"))]),
        ]),
    )]);
    let other_context = OwnedSourceContext::new(&p, Some(&equal_other_input), &[]);
    assert_eq!(
        other_context.position(&query, &walked[0].frames, PositionPhase::Structural),
        Err(ContextError {
            request: ContextRequest::Position(&query),
            fault: ContextFault::InvalidFrame {
                frame: Rc::clone(&walked[0].frames[0])
            }
        })
    );
}

#[test]
fn owned_walks_reject_document_dynamic_and_noncollection_domains_completely() {
    let mut p = project604(&["Dynamic"]);
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 77,
        iteration: vec![],
    });
    let document =
        Instance::DocumentSet(vec![DocumentMember::new("a.json", group(vec![])).unwrap()]);
    let mapped = Instance::MappedSequence(vec![group(vec![])]);
    for (input, domain) in [
        (&document, UnsupportedSourceDomain::DocumentSet),
        (&mapped, UnsupportedSourceDomain::MappedSequence),
    ] {
        let context = OwnedSourceContext::new(&p, Some(input), &[]);
        let source = collection604(SourceOwner::Primary, &[]);
        assert_eq!(
            context.walk(&source, &[]),
            Err(ContextError {
                request: ContextRequest::Walk(&source),
                fault: ContextFault::UnsupportedDomain {
                    declaration: primary(&p, None).declaration,
                    prefix_len: 0,
                    domain
                }
            })
        );
    }
    let input = group(vec![
        ("Value", text("root")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    let inputs = vec![("Dynamic".into(), group(vec![]))];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let dynamic = collection604(named("Dynamic"), &[]);
    assert_eq!(
        context.walk(&dynamic, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&dynamic),
            fault: ContextFault::UnsupportedDomain {
                declaration: extra(&p, 0, None).declaration,
                prefix_len: 0,
                domain: UnsupportedSourceDomain::DynamicSource
            }
        })
    );
    let scalar = collection604(SourceOwner::Primary, &["Value"]);
    assert_eq!(
        context.walk(&scalar, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&scalar),
            fault: ContextFault::NonCollection {
                declaration: primary(&p, None).declaration
            }
        })
    );
    let unknown = collection604(SourceOwner::Primary, &["Absent"]);
    assert_eq!(
        context.walk(&unknown, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&unknown),
            fault: ContextFault::UnknownPath {
                declaration: primary(&p, None).declaration,
                prefix_len: 1
            }
        })
    );
}

#[test]
fn owned_walk_raw_positions_match_existing_engine_borrowed_source_walks() {
    let p = project604(&[]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![]), group(vec![]), group(vec![])]),
    )]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Rows"]);
    let owned = context.walk(&source, &[]).unwrap();
    let legacy = crate::source_iteration::walk(&input, &["Rows".into()], &[], &[], &[]);
    assert_eq!(owned.len(), legacy.len());
    for (owned, legacy) in owned.iter().zip(legacy) {
        assert!(std::ptr::eq(owned.item, legacy.instances[0]));
        assert_eq!(
            (
                &owned.frames[0].collection,
                owned.frames[0].structural.get(),
                legacy.positions[0].grouped,
                legacy.positions[0].join,
                legacy.positions[0].join_position,
                &legacy.positions[0].document_path
            ),
            (
                &legacy.positions[0].collection,
                legacy.positions[0].index,
                false,
                None,
                None,
                &None
            )
        );
    }
}

#[test]
fn owned_position_phases_match_legacy_filter_sort_and_window_outputs() {
    for (order, value) in [
        (mapping::SortFilterOrder::FilterThenSort, 30),
        (mapping::SortFilterOrder::SortThenFilter, 20),
    ] {
        let mut p = project604(&[]);
        p.source = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        );
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Value", ScalarType::Int),
                SchemaNode::scalar("Position", ScalarType::Int),
            ],
        )
        .repeating();
        p.graph.nodes.extend([
            (
                10,
                Node::SourceField {
                    path: vec!["Rows".into(), "Value".into()],
                    frame: None,
                },
            ),
            (
                11,
                Node::Position {
                    collection: vec!["Rows".into()],
                },
            ),
            (
                12,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                13,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                14,
                Node::Call {
                    function: "less_or_equal".into(),
                    args: vec![11, 12],
                },
            ),
        ]);
        p.root = Scope {
            iteration: mapping::ScopeIteration::Source(vec!["Rows".into()]),
            sort_by: Some(10),
            sort_filter_order: order,
            filter: Some(14),
            windows: vec![mapping::SequenceWindow::SkipFirst { count: 13 }],
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 10,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 11,
                },
            ],
            ..Scope::default()
        };
        let input = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                group(vec![("Value", Instance::Scalar(Value::Int(30)))]),
                group(vec![("Value", Instance::Scalar(Value::Int(10)))]),
                group(vec![("Value", Instance::Scalar(Value::Int(20)))]),
            ]),
        )]);
        assert_eq!(
            crate::run(&p, &input),
            Ok(Instance::Repeated(vec![group(vec![
                ("Value", Instance::Scalar(Value::Int(value))),
                ("Position", Instance::Scalar(Value::Int(1)))
            ])]))
        );
        let context = OwnedSourceContext::new(&p, Some(&input), &[]);
        let source = collection604(SourceOwner::Primary, &["Rows"]);
        let raw = context.walk(&source, &[]).unwrap();
        let selected_raw_index = if value == 30 { 0 } else { 2 };
        let selected = raw[selected_raw_index]
            .clone()
            .sorted(nz604(2))
            .produced(nz604(1));
        let query = OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        };
        let endpoint = primary(&p, Some(&input));
        for (phase, index) in [
            (PositionPhase::Structural, selected_raw_index + 1),
            (PositionPhase::Selection, 2),
            (PositionPhase::Output, 1),
        ] {
            assert_eq!(
                context.position(&query, &selected.frames, phase),
                Ok(SourcePosition {
                    endpoint,
                    request: &query,
                    frame: Rc::clone(&selected.frames[0]),
                    phase,
                    index: nz604(index)
                })
            );
        }
    }
}

#[test]
fn owned_empty_pins_read_exact_primary_and_named_roots_with_only_row_frames_active() {
    let p = project604(&[" Ref 雪 "]);
    let input = group(vec![
        ("Value", text("primary root")),
        (
            "Rows",
            Instance::Repeated(vec![group(vec![(
                "Value",
                text("primary row cannot rescue root"),
            )])]),
        ),
    ]);
    let inputs = vec![(
        " Ref 雪 ".into(),
        group(vec![
            ("Value", text("named root")),
            (
                "Rows",
                Instance::Repeated(vec![group(vec![(
                    "Value",
                    text("named row cannot rescue root"),
                )])]),
            ),
        ]),
    )];
    let context = OwnedSourceContext::new(&p, Some(&input), &inputs);
    let primary_source = collection604(SourceOwner::Primary, &["Rows"]);
    let named_source = collection604(named(" Ref 雪 "), &["Rows"]);
    let primary_walk = context.walk(&primary_source, &[]).unwrap();
    let named_walk = context.walk(&named_source, &[]).unwrap();
    let mut active = primary_walk[0].frames.clone();
    active.extend(named_walk[0].frames.iter().map(Rc::clone));
    for (request, endpoint, value) in [
        (
            field(SourceOwner::Primary, &["Value"], Some(&[])),
            primary(&p, Some(&input)),
            input.field("Value").unwrap().as_scalar(),
        ),
        (
            field(named(" Ref 雪 "), &["Value"], Some(&[])),
            extra(&p, 0, Some(&inputs[0].1)),
            inputs[0].1.field("Value").unwrap().as_scalar(),
        ),
    ] {
        assert_eq!(
            context.framed_scalar(&request, &active),
            Ok(FrameScalar {
                endpoint,
                request: &request,
                frame: None,
                value
            })
        );
    }
    let repeated_input = Instance::Repeated(vec![group(vec![(
        "Value",
        text("no implicit first occurrence"),
    )])]);
    let repeated = OwnedSourceContext::new(&p, Some(&repeated_input), &inputs);
    let request = field(SourceOwner::Primary, &["Value"], Some(&[]));
    assert_eq!(
        repeated.framed_scalar(&request, &named_walk[0].frames),
        Err(ContextError {
            request: ContextRequest::Scalar(&request),
            fault: ContextFault::InactiveFrame {
                declaration: primary(&p, None).declaration
            }
        })
    );
}

#[test]
fn owned_walk_rejects_unexpected_intermediate_repetition_before_emitting_frames() {
    let mut p = project604(&[]);
    p.source = SchemaNode::group(
        "Root",
        vec![SchemaNode::group(
            "Parent",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                )
                .repeating(),
            ],
        )],
    );
    let input = group(vec![(
        "Parent",
        Instance::Repeated(vec![group(vec![(
            "Rows",
            Instance::Repeated(vec![group(vec![("Value", text("malformed parent"))])]),
        )])]),
    )]);
    let context = OwnedSourceContext::new(&p, Some(&input), &[]);
    let source = collection604(SourceOwner::Primary, &["Parent", "Rows"]);
    assert_eq!(
        context.walk(&source, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&source),
            fault: ContextFault::InvalidShape {
                declaration: primary(&p, None).declaration,
                prefix_len: 1,
                value: input.field("Parent").unwrap()
            }
        })
    );
    let malformed_root = Instance::Repeated(vec![group(vec![])]);
    let root_context = OwnedSourceContext::new(&p, Some(&malformed_root), &[]);
    let root_source = collection604(SourceOwner::Primary, &[]);
    assert_eq!(
        root_context.walk(&root_source, &[]),
        Err(ContextError {
            request: ContextRequest::Walk(&root_source),
            fault: ContextFault::InvalidShape {
                declaration: primary(&p, None).declaration,
                prefix_len: 0,
                value: &malformed_root
            }
        })
    );
    let endpoint = primary(&p, Some(&malformed_root));
    let forged_root = frame604(
        endpoint,
        &[],
        &malformed_root.as_repeated().unwrap()[0],
        None,
        1,
    );
    let position = OwnedPosition {
        owner: SourceOwner::Primary,
        collection: Some(vec![]),
    };
    assert_eq!(
        root_context.position(
            &position,
            &[Rc::clone(&forged_root)],
            PositionPhase::Structural
        ),
        Err(ContextError {
            request: ContextRequest::Position(&position),
            fault: ContextFault::InvalidFrame { frame: forged_root }
        })
    );
}
