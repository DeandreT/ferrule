use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{
    FiniteF64, NumberBound, NumberRange, NumericRange, ScalarType, ScalarTypeSet, SchemaKind,
    SchemaNode, StringLengthRange, Value,
};
use mapping::{
    Binding, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, FormatOptions, Graph,
    HttpTimeoutSeconds, NamedSource, NamedTarget, Node, Project, Scope,
};
use mfd::{ExportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join("mfd-float-export").join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project_with_json_source(bound: f64) -> Project {
    let mut amount = SchemaNode::scalar("Amount", ScalarType::Float);
    amount.numeric_range = Some(NumericRange::Number(
        NumberRange::new(
            Some(NumberBound::inclusive(FiniteF64::new(bound).unwrap())),
            None,
        )
        .unwrap(),
    ));
    Project {
        source: SchemaNode::group("Source", vec![amount]),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Amount", ScalarType::Float)],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("target.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Amount".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Amount".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn set_source_bound(project: &mut Project, bound: Option<f64>) {
    let SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!()
    };
    children[0].numeric_range = bound.map(|bound| {
        NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(FiniteF64::new(bound).unwrap())),
                None,
            )
            .unwrap(),
        )
    });
}

fn assert_fidelity_rejection(project: &Project, owner: &str) {
    let temp = TempDir::new();
    let output = temp.0.join("new-parent/mapping.mfd");
    let error = mfd::preflight_export(project, &output).unwrap_err();
    assert!(matches!(error, MfdError::SchemaFidelity(_)), "{error}");
    assert!(error.to_string().contains(owner), "{error}");
    assert!(!output.parent().unwrap().exists());
}

#[test]
fn unstable_number_range_rejects_both_profiles_before_creating_a_directory() {
    const LOW: u64 = 0x0031_fa18_2c40_c60d;
    let temp = TempDir::new();
    let project = project_with_json_source(f64::from_bits(LOW));
    let output = temp.0.join("new-parent/mapping.mfd");

    let error = mfd::preflight_export(&project, &output).unwrap_err();
    assert!(matches!(error, MfdError::SchemaFidelity(_)), "{error}");
    assert!(error.to_string().contains("numeric_range"), "{error}");
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let error = mfd::export_with_profile(&project, &output, profile).unwrap_err();
        assert!(matches!(error, MfdError::SchemaFidelity(_)), "{error}");
    }
    assert!(!output.parent().unwrap().exists());
}

#[test]
fn stable_number_range_still_exports_and_reimports() {
    let temp = TempDir::new();
    let project = project_with_json_source(1.5);
    let output = temp.0.join("mapping.mfd");
    mfd::export(&project, &output).unwrap();
    let restored = mfd::import(&output).unwrap();
    let SchemaKind::Group {
        children: before, ..
    } = &project.source.kind
    else {
        unreachable!()
    };
    let SchemaKind::Group {
        children: after, ..
    } = &restored.project.source.kind
    else {
        unreachable!()
    };
    assert_eq!(after[0].numeric_range, before[0].numeric_range);
}

#[test]
fn signed_zero_range_cannot_be_silently_rewritten() {
    let project = project_with_json_source(-0.0);
    assert_fidelity_rejection(&project, "JSON boundary");
}

#[test]
fn named_and_dynamic_json_boundaries_share_fidelity_check() {
    const LOW: u64 = 0x0031_fa18_2c40_c60d;
    let low = f64::from_bits(LOW);

    let mut primary_target = project_with_json_source(1.5);
    let SchemaKind::Group { children, .. } = &mut primary_target.target.kind else {
        unreachable!()
    };
    children[0].numeric_range = Some(NumericRange::Number(
        NumberRange::new(
            Some(NumberBound::inclusive(FiniteF64::new(low).unwrap())),
            None,
        )
        .unwrap(),
    ));
    primary_target.target_path = Some("target.json".into());
    primary_target.target_options.json_document = true;
    assert_fidelity_rejection(&primary_target, "JSON boundary");

    let mut named_source = project_with_json_source(1.5);
    named_source.extra_sources.push(NamedSource {
        name: "Secondary".into(),
        path: "secondary.json".into(),
        schema: project_with_json_source(low).source,
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: None,
    });
    assert_fidelity_rejection(&named_source, "JSON boundary `Secondary`");

    let mut named_target = project_with_json_source(1.5);
    named_target.extra_targets.push(NamedTarget {
        name: "SecondaryOutput".into(),
        path: Some("secondary-output.json".into()),
        schema: project_with_json_source(low).source,
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: Scope::default(),
    });
    assert_fidelity_rejection(&named_target, "JSON boundary `SecondaryOutput`");

    let mut dynamic_source = project_with_json_source(1.5);
    set_source_bound(&mut dynamic_source, None);
    let SchemaKind::Group { dynamic, .. } = &mut dynamic_source.source.kind else {
        unreachable!()
    };
    let SchemaKind::Group {
        children: low_children,
        ..
    } = project_with_json_source(low).source.kind
    else {
        unreachable!()
    };
    *dynamic = Some(Box::new(low_children.into_iter().next().unwrap()));
    dynamic_source.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("Other".into()),
        },
    );
    dynamic_source.graph.nodes.insert(
        1,
        Node::DynamicSourceField {
            object: Vec::new(),
            frame: None,
            key: 0,
        },
    );
    dynamic_source.root.bindings[0].node = 1;
    assert_fidelity_rejection(&dynamic_source, "dynamic JSON boundary");
}

#[test]
fn external_json_request_and_user_function_siblings_are_checked() {
    const LOW: u64 = 0x0031_fa18_2c40_c60d;
    let low = f64::from_bits(LOW);
    let mut request = project_with_json_source(1.5);
    set_source_bound(&mut request, None);
    request.source_path = Some("https://example.test/query".into());
    request.source_options.external_source = Some(
        ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::new(20).unwrap(),
            Some(ExternalPayloadFormat::Json),
            Some(project_with_json_source(low).source),
            ExternalPayloadFormat::Json,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_fidelity_rejection(&request, "captured HTTP POST request");

    let mut user_function = project_with_json_source(low);
    user_function.source_options.external_source = Some(
        ExternalSourceOptions::user_function(
            "Opaque",
            "external implementation",
            ExternalPayloadFormat::Json,
        )
        .unwrap(),
    );
    assert_fidelity_rejection(&user_function, "captured user-function response");
}

#[test]
fn inline_http_response_rejects_constraints_even_when_the_number_is_stable() {
    let mut project = project_with_json_source(1.5);
    project.source_path = Some("https://example.test/query".into());
    project.source_options.external_source = Some(
        ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::new(20).unwrap(),
            None,
            None,
            ExternalPayloadFormat::Json,
            Vec::new(),
        )
        .unwrap(),
    );
    let temp = TempDir::new();
    let output = temp.0.join("new-parent/mapping.mfd");
    let error = mfd::preflight_export(&project, &output).unwrap_err();
    assert!(matches!(error, MfdError::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("inline entry tree"), "{error}");
    assert!(!output.parent().unwrap().exists());

    let mut text = SchemaNode::scalar("Amount", ScalarType::String);
    text.string_length_range = StringLengthRange::new(1, None);
    {
        let SchemaKind::Group { children, .. } = &mut project.source.kind else {
            unreachable!()
        };
        children[0] = text;
    }
    let error = mfd::preflight_export(&project, &output).unwrap_err();
    assert!(matches!(error, MfdError::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("inline entry tree"), "{error}");
    assert!(!output.parent().unwrap().exists());

    {
        let SchemaKind::Group { children, .. } = &mut project.source.kind else {
            unreachable!()
        };
        children[0] = SchemaNode::scalar_union(
            "Amount",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        );
    }
    let error = mfd::preflight_export(&project, &output).unwrap_err();
    assert!(matches!(error, MfdError::Unsupported(_)), "{error}");
    assert!(error.to_string().contains("scalar union"), "{error}");
    assert!(!output.parent().unwrap().exists());
}

#[test]
fn graph_json_parser_schema_drift_is_a_hard_export_error() {
    let mut project = project_with_json_source(1.5);
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Payload", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Amount", ScalarType::Float)],
    );
    project.source_path = Some("source.xml".into());
    project.target_path = Some("target.xml".into());
    project.source_options = FormatOptions::default();
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Payload".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(
                    r#"{"name":"Payload","kind":{"kind":"group","children":[{"name":"Amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.0000000000000001e-307}}},"kind":{"kind":"scalar","ty":"float"}}]}}"#.into(),
                ),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(r#"["Amount"]"#.into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![0, 1, 2],
            },
        ),
    ]);
    project.root.bindings[0].node = 3;
    assert_fidelity_rejection(&project, "JSON string parser node 3");
}
