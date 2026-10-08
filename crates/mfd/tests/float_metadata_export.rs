use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{
    FiniteF64, NumberBound, NumberRange, NumericRange, ScalarType, ScalarTypeSet, SchemaKind,
    SchemaNode, StringLengthRange, Value,
};
use mapping::{
    Binding, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, ExternalSourceOrigin,
    FormatOptions, Graph, HttpTimeoutSeconds, NamedSource, NamedTarget, Node, Project, Scope,
};
use mfd::{ExportCompatibility, ExportCompatibilityFeature, ExportProfile, MfdError};

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
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "Retained finite metadata export artifacts: {}",
                self.0.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let error = mfd::export_with_profile(project, &output, profile).unwrap_err();
        assert!(matches!(error, MfdError::SchemaFidelity(_)), "{error}");
        assert!(error.to_string().contains(owner), "{error}");
        assert!(!output.parent().unwrap().exists());
    }
}

// Compare all finite slots, then the complete public schema text: equality of
// SchemaNode alone hides signed zero, and checking just one bound misses siblings.
fn finite_metadata(schema: &SchemaNode) -> BTreeMap<String, u64> {
    fn visit(value: &serde_json::Value, path: &str, bits: &mut BTreeMap<String, u64>) {
        match value {
            serde_json::Value::Number(number) if number.is_f64() => {
                bits.insert(path.into(), number.as_f64().unwrap().to_bits());
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    visit(value, &format!("{path}/{index}"), bits);
                }
            }
            serde_json::Value::Object(values) => {
                for (name, value) in values {
                    visit(
                        value,
                        &format!("{path}/{}", name.replace('~', "~0").replace('/', "~1")),
                        bits,
                    );
                }
            }
            _ => {}
        }
    }
    let mut bits = BTreeMap::new();
    visit(&serde_json::to_value(schema).unwrap(), "", &mut bits);
    bits
}

fn assert_schema_roundtrip(before: &SchemaNode, after: &SchemaNode) {
    assert_eq!(finite_metadata(after), finite_metadata(before));
    assert_eq!(
        format_json::json_schema::export(after).unwrap(),
        format_json::json_schema::export(before).unwrap(),
        "all published constraints and field identities are retained",
    );
}

fn assert_profiled_roundtrip(
    project: &Project,
    extension: Option<ExportCompatibilityFeature>,
    check: impl Fn(&Project),
) {
    for (name, profile) in [
        ("extensions", ExportProfile::FerruleExtensions),
        ("native", ExportProfile::NativeMfd),
    ] {
        let temp = TempDir::new();
        let output = temp.0.join(name).join("mapping.mfd");
        let preflight = mfd::preflight_export(project, &output);
        std::fs::write(
            temp.0.join("preflight-result.txt"),
            format!("{preflight:?}\n"),
        )
        .unwrap();
        let report = preflight.unwrap();
        std::fs::write(
            temp.0.join("preflight-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        assert!(!output.parent().unwrap().exists());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        if let Some(feature) = extension {
            assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
            assert!(!report.issues.is_empty());
            assert!(
                report.issues.iter().all(|issue| issue.feature == feature),
                "{:?}",
                report.issues
            );
        } else {
            assert_eq!(report.compatibility, ExportCompatibility::NativeMfd);
            assert!(report.issues.is_empty(), "{:?}", report.issues);
        }
        if profile == ExportProfile::NativeMfd && extension.is_some() {
            let outcome = mfd::export_with_profile(project, &output, profile);
            std::fs::write(temp.0.join("native-result.txt"), format!("{outcome:?}\n")).unwrap();
            let error = outcome.unwrap_err();
            let MfdError::IncompatibleExport(actual) = error else {
                panic!("strict refusal must retain its compatibility report");
            };
            assert_eq!(*actual, report);
            assert!(!output.parent().unwrap().exists());
            continue;
        }
        let outcome = mfd::export_with_profile(project, &output, profile);
        std::fs::write(temp.0.join("export-result.txt"), format!("{outcome:?}\n")).unwrap();
        let exported = outcome.unwrap();
        std::fs::write(
            temp.0.join("export-report.json"),
            serde_json::to_vec_pretty(&exported).unwrap(),
        )
        .unwrap();
        assert_eq!(exported, report);
        let outcome = mfd::import(&output);
        match &outcome {
            Ok(imported) => {
                std::fs::write(
                    temp.0.join("raw-import-project.json"),
                    serde_json::to_vec_pretty(&imported.project).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    temp.0.join("raw-import-warnings.json"),
                    serde_json::to_vec_pretty(&imported.warnings).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    temp.0.join("raw-import-mapping-path.txt"),
                    format!("{:?}\n", imported.mapping_path),
                )
                .unwrap();
            }
            Err(error) => {
                std::fs::write(temp.0.join("raw-import-error.txt"), format!("{error:?}\n")).unwrap()
            }
        }
        let restored = outcome.unwrap();
        std::fs::write(
            temp.0.join("imported-project.json"),
            serde_json::to_vec_pretty(&restored.project).unwrap(),
        )
        .unwrap();
        std::fs::write(
            temp.0.join("import-warnings.json"),
            serde_json::to_vec_pretty(&restored.warnings).unwrap(),
        )
        .unwrap();
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        check(&restored.project);
    }
}

#[test]
fn adjacent_finite_number_ranges_preserve_bits_in_both_profiles() {
    for bits in [0x0031_fa18_2c40_c60d, 0x0031_fa18_2c40_c60e] {
        let project = project_with_json_source(f64::from_bits(bits));
        assert_eq!(
            finite_metadata(&project.source)
                .get("/kind/children/0/numeric_range/bounds/minimum/value"),
            Some(&bits),
        );
        assert_profiled_roundtrip(&project, None, |restored| {
            assert_schema_roundtrip(&project.source, &restored.source);
            let Some(NumericRange::Number(range)) =
                restored.source.child("Amount").unwrap().numeric_range
            else {
                panic!("restored JSON source retains its range");
            };
            assert_eq!(range.minimum().unwrap().value().get().to_bits(), bits);
            assert!(range.contains(f64::from_bits(bits)));
            assert!(!range.contains(f64::from_bits(bits - 1)));
        });
    }
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
    for bits in [0x0031_fa18_2c40_c60d, 0x0031_fa18_2c40_c60e] {
        let low = f64::from_bits(bits);

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
        assert_profiled_roundtrip(&primary_target, None, |restored| {
            assert_schema_roundtrip(&primary_target.source, &restored.source);
            assert_schema_roundtrip(&primary_target.target, &restored.target);
        });

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
        named_source.graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["Secondary".into(), "Amount".into()],
                frame: None,
            },
        );
        assert_profiled_roundtrip(&named_source, None, |restored| {
            assert_schema_roundtrip(&named_source.source, &restored.source);
            let actual = restored
                .extra_sources
                .iter()
                .find(|source| source.name == "Secondary")
                .unwrap();
            assert_schema_roundtrip(&named_source.extra_sources[0].schema, &actual.schema);
        });

        let mut named_target = project_with_json_source(1.5);
        named_target.extra_targets.push(NamedTarget {
            name: "SecondaryOutput".into(),
            path: Some("secondary-output.json".into()),
            schema: project_with_json_source(low).source,
            options: FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Amount".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        });
        assert_profiled_roundtrip(&named_target, None, |restored| {
            assert_schema_roundtrip(&named_target.source, &restored.source);
            let actual = restored
                .extra_targets
                .iter()
                .find(|target| target.name == "SecondaryOutput")
                .unwrap();
            assert_schema_roundtrip(&named_target.extra_targets[0].schema, &actual.schema);
        });

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
        assert_profiled_roundtrip(&dynamic_source, None, |restored| {
            assert_schema_roundtrip(&dynamic_source.source, &restored.source);
            assert!(
                restored
                    .graph
                    .nodes
                    .values()
                    .any(|node| matches!(node, Node::DynamicSourceField { .. }))
            );
        });
    }
}

#[test]
fn external_json_request_and_user_function_siblings_are_checked() {
    for bits in [0x0031_fa18_2c40_c60d, 0x0031_fa18_2c40_c60e] {
        let low = f64::from_bits(bits);
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
        assert_profiled_roundtrip(
            &request,
            Some(ExportCompatibilityFeature::CapturedHttpPost),
            |restored| {
                assert_eq!(
                    restored.source_options.external_source,
                    request.source_options.external_source
                );
                let ExternalSourceOrigin::HttpPost {
                    request_schema: Some(actual),
                    ..
                } = restored
                    .source_options
                    .external_source
                    .as_ref()
                    .unwrap()
                    .origin()
                else {
                    panic!("captured POST retains its typed request schema");
                };
                assert_schema_roundtrip(&project_with_json_source(low).source, actual);
                assert_eq!(
                    finite_metadata(actual)
                        .get("/kind/children/0/numeric_range/bounds/minimum/value"),
                    Some(&bits)
                );
            },
        );

        let mut user_function = project_with_json_source(low);
        user_function.source_options.external_source = Some(
            ExternalSourceOptions::user_function(
                "Opaque",
                "external implementation",
                ExternalPayloadFormat::Json,
            )
            .unwrap(),
        );
        assert_profiled_roundtrip(
            &user_function,
            Some(ExportCompatibilityFeature::CapturedUserFunction),
            |restored| {
                assert_schema_roundtrip(&user_function.source, &restored.source);
                assert_eq!(
                    restored.source_options.external_source,
                    user_function.source_options.external_source
                );
            },
        );
    }
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
fn graph_json_parser_finite_schema_round_trips_and_signed_zero_refuses() {
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
    let Node::Const {
        value: Value::String(original),
    } = &project.graph.nodes[&1]
    else {
        unreachable!()
    };
    let original = original.clone();
    for (text, bits) in [
        ("1e-307", 0x0031_fa18_2c40_c60d),
        ("1.0000000000000001e-307", 0x0031_fa18_2c40_c60e),
    ] {
        let descriptor = original.replace("1.0000000000000001e-307", text);
        let schema: SchemaNode = serde_json::from_str(&descriptor).unwrap();
        project.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::String(descriptor),
            },
        );
        assert_profiled_roundtrip(&project, None, |restored| {
            let args = restored
                .graph
                .nodes
                .values()
                .find_map(|node| {
                    if let Node::Call { function, args } = node {
                        (function == "json_parse_field").then_some(args)
                    } else {
                        None
                    }
                })
                .expect("public import retains the JSON parser call");
            assert_eq!(args.len(), 3);
            let Node::Const {
                value: Value::String(actual),
            } = &restored.graph.nodes[&args[1]]
            else {
                panic!("parser retains its schema descriptor");
            };
            let actual: SchemaNode = serde_json::from_str(actual).unwrap();
            assert_schema_roundtrip(&schema, &actual);
            assert_eq!(
                finite_metadata(&actual).get("/kind/children/0/numeric_range/bounds/minimum/value"),
                Some(&bits)
            );
            let Node::Const {
                value: Value::String(path),
            } = &restored.graph.nodes[&args[2]]
            else {
                panic!("parser retains its selected field path");
            };
            assert_eq!(
                serde_json::from_str::<Vec<String>>(path).unwrap(),
                ["Amount"]
            );
        });
    }
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(original.replace("1.0000000000000001e-307", "-0.0")),
        },
    );
    assert_fidelity_rejection(&project, "JSON string parser node 3");
}
