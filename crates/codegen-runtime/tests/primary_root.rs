use codegen_runtime::{NamedInput, RuntimeError, ScopeContext};
use ir::{Instance, InstanceGroup, PrimaryRootError, Value, XmlTypeOrigin};

fn scalar(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn marked(fields: Vec<(String, Instance)>, origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(fields)
            .with_xml_type_origin(origin)
            .unwrap(),
    )
}
fn failure(result: Result<Value, RuntimeError>, expected: PrimaryRootError) {
    assert_eq!(
        result,
        Err(RuntimeError::PrimaryRoot {
            node: 42,
            source: expected
        })
    );
}

#[test]
fn primary_root_annotation_reads_observed_fact_instead_of_selected_writer_data() {
    for (origin, expected) in [
        (XmlTypeOrigin::Absent, false),
        (XmlTypeOrigin::Explicit("Derived"), true),
        (XmlTypeOrigin::Explicit("Other"), false),
        (XmlTypeOrigin::Explicit("{urn:exact}Derived"), false),
    ] {
        let root = marked(vec![(ir::XML_TYPE_FIELD.into(), scalar("Derived"))], origin);
        assert_eq!(
            ScopeContext::new(&root).source_root_xml_type_equals(42, "Derived"),
            Ok(Value::Bool(expected))
        );
    }
    let root = marked(vec![], XmlTypeOrigin::Explicit("{urn:exact}Derived"));
    assert_eq!(
        ScopeContext::new(&root).source_root_xml_type_equals(42, "{urn:exact}Derived"),
        Ok(Value::Bool(true))
    );
}

#[test]
fn primary_root_unknown_and_malformed_identity_fail_with_exact_node_and_source() {
    let unknown = marked(vec![], XmlTypeOrigin::Unknown);
    failure(
        ScopeContext::new(&unknown).source_root_xml_type_equals(42, "Derived"),
        PrimaryRootError::UnknownXmlTypeOrigin,
    );
    let malformed = marked(vec![], XmlTypeOrigin::Explicit("p:Derived"));
    failure(
        ScopeContext::new(&malformed).source_root_xml_type_equals(42, "Derived"),
        PrimaryRootError::InvalidTypeIdentity,
    );
    for identity in ["", "{}Derived", "p:Derived", "{a b}Derived", "9Bad"] {
        failure(
            ScopeContext::new(&unknown).source_root_xml_type_equals(42, identity),
            PrimaryRootError::InvalidTypeIdentity,
        );
    }
}

#[test]
fn primary_root_scalar_missing_null_and_wrong_shapes_are_distinct() {
    let root = marked(
        vec![
            ("Null".into(), Instance::Scalar(Value::Null)),
            (
                "Child".into(),
                marked(
                    vec![("Code".into(), scalar("child"))],
                    XmlTypeOrigin::Absent,
                ),
            ),
            ("Rows".into(), Instance::Repeated(vec![scalar("first")])),
        ],
        XmlTypeOrigin::Absent,
    );
    let context = ScopeContext::new(&root);
    for path in [&["Missing"][..], &["Missing", "Code"][..], &["Null"][..]] {
        assert_eq!(context.source_root_field(42, path), Ok(Value::Null));
    }
    assert_eq!(
        context.source_root_field(42, &["Child", "Code"]),
        Ok(Value::String("child".into()))
    );
    failure(
        context.source_root_field(42, &["Null", "Code"]),
        PrimaryRootError::ExpectedGroupAt {
            path: vec!["Null".into()],
            found: "scalar",
        },
    );
    failure(
        context.source_root_field(42, &["Rows"]),
        PrimaryRootError::ExpectedScalar {
            path: vec!["Rows".into()],
            found: "repeated",
        },
    );
    failure(
        context.source_root_field(42, &["Child"]),
        PrimaryRootError::ExpectedScalar {
            path: vec!["Child".into()],
            found: "group",
        },
    );
}

#[test]
fn primary_root_never_unwraps_collection_document_or_mapped_owner() {
    let item = marked(
        vec![("Code".into(), scalar("first"))],
        XmlTypeOrigin::Explicit("Derived"),
    );
    let document = ir::DocumentMember::new("first.xml", item.clone()).unwrap();
    for (root, found) in [
        (scalar("root"), "scalar"),
        (Instance::Repeated(vec![item.clone()]), "repeated"),
        (Instance::DocumentSet(vec![document]), "document set"),
        (Instance::MappedSequence(vec![item]), "mapped sequence"),
    ] {
        let context = ScopeContext::new(&root);
        failure(
            context.source_root_field(42, &["Code"]),
            PrimaryRootError::ExpectedGroup { found },
        );
        failure(
            context.source_root_xml_type_equals(42, "Derived"),
            PrimaryRootError::ExpectedGroup { found },
        );
    }
}

#[test]
fn primary_root_duplicate_fields_and_bounded_paths_fail_before_fallback() {
    let duplicate = marked(
        vec![
            ("Code".into(), scalar("first")),
            ("Code".into(), scalar("second")),
        ],
        XmlTypeOrigin::Absent,
    );
    failure(
        ScopeContext::new(&duplicate).source_root_field(42, &["Code"]),
        PrimaryRootError::DuplicateField {
            path: vec!["Code".into()],
        },
    );
    let oversized = marked(
        (0..4097).map(|i| (format!("F{i}"), scalar("x"))).collect(),
        XmlTypeOrigin::Absent,
    );
    failure(
        ScopeContext::new(&oversized).source_root_field(42, &["Missing"]),
        PrimaryRootError::FieldLimit,
    );
    let root = marked(vec![], XmlTypeOrigin::Absent);
    for path in [
        vec![],
        vec!["a"; 17],
        vec![ir::XML_TYPE_FIELD],
        vec!["element()"],
    ] {
        failure(
            ScopeContext::new(&root).source_root_field(42, &path),
            PrimaryRootError::InvalidScalarPath,
        );
    }
}

#[test]
fn primary_root_owner_survives_item_and_named_context_collisions() {
    let item = marked(
        vec![("Code".into(), scalar("item"))],
        XmlTypeOrigin::Explicit("Other"),
    );
    let root = marked(
        vec![
            ("Code".into(), scalar("primary")),
            ("Rows".into(), Instance::Repeated(vec![item])),
        ],
        XmlTypeOrigin::Explicit("Derived"),
    );
    let named = marked(
        vec![("Code".into(), scalar("named"))],
        XmlTypeOrigin::Explicit("Other"),
    );
    let inputs = [NamedInput {
        name: "Named",
        instance: &named,
    }];
    let context = ScopeContext::with_named_inputs(&root, &inputs);
    for nested in context.walk_source(&["Rows"]) {
        assert_eq!(
            nested.source_root_field(42, &["Code"]),
            Ok(Value::String("primary".into()))
        );
        assert_eq!(
            nested.source_root_xml_type_equals(42, "Derived"),
            Ok(Value::Bool(true))
        );
    }
    assert_eq!(context.source_root_field(42, &["Missing"]), Ok(Value::Null));
    assert_eq!(
        context.source_root_field(42, &["Named", "Code"]),
        Ok(Value::Null)
    );
}

#[test]
fn primary_root_historical_origin_spelling_stays_ordinary_data() {
    let root = marked(
        vec![(ir::XML_TYPE_ORIGIN_FIELD.into(), scalar("ordinary"))],
        XmlTypeOrigin::Absent,
    );
    assert_eq!(
        ScopeContext::new(&root).source_root_field(42, &[ir::XML_TYPE_ORIGIN_FIELD]),
        Ok(Value::String("ordinary".into()))
    );
}
