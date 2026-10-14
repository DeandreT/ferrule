use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiBoundaryKind, FormatOptions, Graph, Node, Project, Scope, ScopeIteration,
    ScopeSequence, TabularBoundaryKind,
};

const HEADERS: &[(&str, &[&str])] = &[
    (
        "ISA",
        &[
            "00",
            "          ",
            "00",
            "          ",
            "ZZ",
            "AUTHOR-SENDER  ",
            "ZZ",
            "AUTHOR-RECEIVER",
            "260102",
            "0304",
            "U",
            "00401",
            "000000007",
            "0",
            "T",
            ":",
        ],
    ),
    (
        "GS",
        &[
            "OW",
            "AUTHOR-SENDER",
            "AUTHOR-RECEIVER",
            "20260102",
            "0304",
            "0007",
            "X",
            "004010",
        ],
    ),
    ("ST", &["940", "0011"]),
    ("SE", &["4", "0011"]),
    ("GE", &["1", "0007"]),
    ("IEA", &["1", "000000007"]),
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/csv-x12-occurrences")
        .join(name)
}

fn segment_schema(segment: &str) -> SchemaNode {
    let count = HEADERS
        .iter()
        .find(|(name, _)| *name == segment)
        .map_or(2, |(_, values)| values.len());
    let children = (1..=count)
        .map(|position| {
            let mut node =
                SchemaNode::scalar(format!("{segment}{position:02}"), ScalarType::String);
            node.fixed = match node.name.as_str() {
                "ISA12" => Some("00401".into()),
                "GS08" => Some("004010".into()),
                "ST01" => Some("940".into()),
                _ => None,
            };
            node
        })
        .collect();
    SchemaNode::group(segment, children)
}

fn expected_project(preserve: bool) -> Project {
    let target = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::group(
                "Interchange",
                vec![
                    segment_schema("ISA"),
                    SchemaNode::group(
                        "Group",
                        vec![
                            segment_schema("GS"),
                            SchemaNode::group(
                                "Message_940",
                                vec![
                                    segment_schema("ST"),
                                    SchemaNode::group(
                                        "Rows",
                                        vec![segment_schema("NTE").repeating()],
                                    )
                                    .repeating(),
                                    segment_schema("SE"),
                                ],
                            )
                            .repeating(),
                            segment_schema("GE"),
                        ],
                    )
                    .repeating(),
                    segment_schema("IEA"),
                ],
            )
            .repeating(),
        ],
    );
    let mut nodes = BTreeMap::new();
    let mut scopes = BTreeMap::new();
    let mut next = 0;
    for (segment, values) in HEADERS {
        let bindings = values
            .iter()
            .enumerate()
            .map(|(position, value)| {
                let node = next;
                next += 1;
                nodes.insert(
                    node,
                    Node::Const {
                        value: Value::String((*value).into()),
                    },
                );
                Binding {
                    target_field: format!("{segment}{:02}", position + 1),
                    node,
                }
            })
            .collect();
        scopes.insert(
            *segment,
            Scope {
                target_field: (*segment).into(),
                bindings,
                ..Default::default()
            },
        );
    }
    assert_eq!(next, 32);
    nodes.insert(
        32,
        Node::Const {
            value: Value::String("GEN".into()),
        },
    );
    nodes.insert(
        33,
        Node::Const {
            value: Value::String("ALT".into()),
        },
    );
    nodes.insert(
        34,
        Node::SourceField {
            path: vec!["Key".into()],
            frame: None,
        },
    );
    nodes.insert(
        35,
        Node::SourceField {
            path: vec!["Optional".into()],
            frame: None,
        },
    );
    let branch = |code, data| Scope {
        bindings: vec![
            Binding {
                target_field: "NTE01".into(),
                node: code,
            },
            Binding {
                target_field: "NTE02".into(),
                node: data,
            },
        ],
        ..Default::default()
    };
    let rows = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Source(Vec::new()),
        children: vec![Scope {
            target_field: "NTE".into(),
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                branch(32, 34),
                vec![branch(33, 35)],
            )),
            ..Default::default()
        }],
        ..Default::default()
    };
    // The row scope is constructed first; header binding traversal then appends
    // siblings without changing either saved occurrence or physical schema order.
    let message = Scope {
        target_field: "Message_940".into(),
        children: vec![
            rows,
            scopes.remove("ST").unwrap(),
            scopes.remove("SE").unwrap(),
        ],
        ..Default::default()
    };
    let group = Scope {
        target_field: "Group".into(),
        children: vec![
            message,
            scopes.remove("GS").unwrap(),
            scopes.remove("GE").unwrap(),
        ],
        ..Default::default()
    };
    let interchange = Scope {
        target_field: "Interchange".into(),
        children: vec![
            group,
            scopes.remove("ISA").unwrap(),
            scopes.remove("IEA").unwrap(),
        ],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Key", ScalarType::String),
                SchemaNode::scalar("Optional", ScalarType::String),
            ],
        ),
        target,
        source_path: Some("rows.csv".into()),
        target_path: Some("report.x12".into()),
        source_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(','),
            has_header_row: Some(true),
            csv_preserve_empty_strings: preserve,
            ..Default::default()
        },
        target_options: FormatOptions {
            lenient_segments: true,
            edi_kind: Some(EdiBoundaryKind::X12),
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![interchange],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn single(instance: &Instance) -> &Instance {
    let items = instance.as_repeated().expect("repeated physical metadata");
    assert_eq!(
        items.len(),
        1,
        "constructed ancestor must produce exactly one item"
    );
    &items[0]
}

fn mapped_notes(project: &Project, csv: &str) -> (Instance, Vec<Vec<(Value, Value)>>) {
    let input =
        format_csv::from_str_with_options(csv, &project.source, &(&project.source_options).into())
            .unwrap();
    let output = engine::run(project, &Instance::Repeated(input)).unwrap();
    let interchange = single(output.field("Interchange").unwrap());
    let group = single(interchange.field("Group").unwrap());
    let message = single(group.field("Message_940").unwrap());
    let rows = message.field("Rows").unwrap().as_repeated().unwrap();
    let notes = rows
        .iter()
        .map(|row| {
            let notes = row.field("NTE").unwrap().as_repeated().unwrap();
            assert_eq!(notes.len(), 2);
            notes
                .iter()
                .map(|note| {
                    (
                        note.field("NTE01").unwrap().as_scalar().unwrap().clone(),
                        note.field("NTE02").unwrap().as_scalar().unwrap().clone(),
                    )
                })
                .collect()
        })
        .collect();
    (output, notes)
}

#[test]
fn csv_cloned_occurrences_preserve_complete_projects_order_and_cell_presence() {
    for (name, preserve) in [("literal-direct.mfd", false), ("preserve-empty.mfd", true)] {
        let imported = mfd::import(&fixture(name)).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected_project(preserve)).unwrap()
        );
        assert!(engine::validate(&imported.project).is_empty());
        let (output, notes) = mapped_notes(&imported.project, "Key,Optional\na,A\nb,\nc\n,\n");
        assert_eq!(
            notes,
            vec![
                vec![
                    (Value::String("GEN".into()), Value::String("a".into())),
                    (Value::String("ALT".into()), Value::String("A".into()))
                ],
                vec![
                    (Value::String("GEN".into()), Value::String("b".into())),
                    (
                        Value::String("ALT".into()),
                        if preserve {
                            Value::String(String::new())
                        } else {
                            Value::Null
                        }
                    )
                ],
                vec![
                    (Value::String("GEN".into()), Value::String("c".into())),
                    (Value::String("ALT".into()), Value::Null)
                ],
                vec![
                    (
                        Value::String("GEN".into()),
                        if preserve {
                            Value::String(String::new())
                        } else {
                            Value::Null
                        }
                    ),
                    (
                        Value::String("ALT".into()),
                        if preserve {
                            Value::String(String::new())
                        } else {
                            Value::Null
                        }
                    )
                ],
            ]
        );
        assert!(
            mapped_notes(&imported.project, "Key,Optional\n")
                .1
                .is_empty()
        );
        // Serialization is a complete typed output control, independent of wire.
        let restored: Instance =
            serde_json::from_value(serde_json::to_value(output).unwrap()).unwrap();
        assert_eq!(
            mapped_notes(&imported.project, "Key,Optional\na,A\nb,\nc\n,\n").0,
            restored
        );
    }
}

#[test]
fn csv_quoted_delimiters_newlines_and_unicode_remain_typed_data() {
    let imported = mfd::import(&fixture("literal-direct.mfd")).unwrap();
    let (_, notes) = mapped_notes(
        &imported.project,
        "Key,Optional\n\"a,b\",\"line1\nline2 λ\"\n",
    );
    assert_eq!(
        notes,
        vec![vec![
            (Value::String("GEN".into()), Value::String("a,b".into())),
            (
                Value::String("ALT".into()),
                Value::String("line1\nline2 λ".into())
            )
        ]]
    );
    // CSV parsing/typed mapping success does not assert language-specific wire
    // eligibility. The native delimiter refusal has its own physical control.
}

#[test]
fn csv_explicit_separator_keeps_the_same_whole_row_project_and_order() {
    let imported = mfd::import(&fixture("semicolon.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = expected_project(false);
    expected.source_options.delimiter = Some(';');
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let (_, notes) = mapped_notes(&imported.project, "Key;Optional\na;A\nb;B\n");
    assert_eq!(
        notes,
        vec![
            vec![
                (Value::String("GEN".into()), Value::String("a".into())),
                (Value::String("ALT".into()), Value::String("A".into()))
            ],
            vec![
                (Value::String("GEN".into()), Value::String("b".into())),
                (Value::String("ALT".into()), Value::String("B".into()))
            ],
        ]
    );
}

#[test]
fn csv_single_row_native_wire_and_default_classic_descriptor_are_separate() {
    let imported = mfd::import(&fixture("literal-direct.mfd")).unwrap();
    let (output, _) = mapped_notes(&imported.project, "Key,Optional\na,A\n");
    let wire = format_edi::x12::to_string(&imported.project.target, &output).unwrap();
    assert!(
        wire.contains("~ST*940*0011~NTE*GEN*a~NTE*ALT*A~SE*4*0011~GE*1*0007~IEA*1*000000007~"),
        "{wire}"
    );
    let policy = codegen::CsvX12BoundaryPolicy {
        source: codegen::CsvInputPolicy::from_format_options(&imported.project.source_options)
            .unwrap(),
        target: codegen::X12BoundaryOptions::from_format_options(&imported.project.target_options)
            .unwrap(),
    };
    assert!(matches!(
        codegen::prepare_csv_x12_project_boundary(&imported.project, &policy),
        Err(codegen::CsvX12BoundaryError::TargetX12(_))
    ));
    let (reserved, _) = mapped_notes(&imported.project, "Key,Optional\na*unsafe,A\n");
    assert!(matches!(
        format_edi::x12::to_string(&imported.project.target, &reserved),
        Err(format_edi::EdiFormatError::UnescapableDelimiter { delimiter: '*', .. })
    ));
}

#[test]
fn csv_occurrences_emit_ordinary_typed_rust_and_csharp_without_new_facades() {
    let imported = mfd::import(&fixture("literal-direct.mfd")).unwrap();
    let program = codegen::lower(&imported.project).unwrap();
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-csv-x12-occurrence-typed".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()
                    .unwrap()
                    .display()
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let csharp = codegen_csharp::emit(&program).unwrap();
    assert!(!rust.files().is_empty());
    assert!(!csharp.files().is_empty());
    if let Some(root) = std::env::var_os("FERRULE_FEATURE_INTEGRATION_ARTIFACT_ROOT") {
        let output = PathBuf::from(root).join("csv-x12-occurrences-typed");
        std::fs::create_dir(&output).unwrap();
        for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
            for file in artifacts.files() {
                let path = output.join(language).join(file.path.as_str());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.contents).unwrap();
            }
        }
    }
}

fn expected_legacy_project(preserve: bool) -> Project {
    let target = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::group(
                "Interchange",
                vec![
                    segment_schema("ISA"),
                    SchemaNode::group(
                        "Group",
                        vec![
                            segment_schema("GS"),
                            SchemaNode::group(
                                "Message_940",
                                vec![
                                    segment_schema("ST"),
                                    SchemaNode::group(
                                        "Rows",
                                        vec![segment_schema("NTE").repeating()],
                                    )
                                    .repeating(),
                                    segment_schema("SE"),
                                ],
                            )
                            .repeating(),
                            segment_schema("GE"),
                        ],
                    )
                    .repeating(),
                    segment_schema("IEA"),
                ],
            )
            .repeating(),
        ],
    );
    let mut nodes = BTreeMap::new();
    let mut scopes = BTreeMap::new();
    let mut next = 0;
    for (segment, values) in HEADERS {
        let bindings = values
            .iter()
            .enumerate()
            .map(|(position, value)| {
                let node = next;
                next += 1;
                nodes.insert(
                    node,
                    Node::Const {
                        value: Value::String((*value).into()),
                    },
                );
                Binding {
                    target_field: format!("{segment}{:02}", position + 1),
                    node,
                }
            })
            .collect();
        scopes.insert(
            *segment,
            Scope {
                target_field: (*segment).into(),
                bindings,
                ..Default::default()
            },
        );
    }
    assert_eq!(next, 32);
    nodes.insert(
        32,
        Node::Const {
            value: Value::String("GEN".into()),
        },
    );
    nodes.insert(
        33,
        Node::Const {
            value: Value::String("ALT".into()),
        },
    );
    nodes.insert(
        34,
        Node::SourceField {
            path: vec!["Key".into()],
            frame: None,
        },
    );
    nodes.insert(
        35,
        Node::SourceField {
            path: vec!["Optional".into()],
            frame: None,
        },
    );
    // Source-authored legacy fallback: both saved aliases occupy one scope.
    // These values are declared before import; never derive them from output.
    let notes = Scope {
        target_field: "NTE".into(),
        bindings: vec![
            Binding {
                target_field: "NTE01".into(),
                node: 32,
            },
            Binding {
                target_field: "NTE02".into(),
                node: 34,
            },
            Binding {
                target_field: "NTE01".into(),
                node: 33,
            },
            Binding {
                target_field: "NTE02".into(),
                node: 35,
            },
        ],
        ..Default::default()
    };
    let rows = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Source(Vec::new()),
        children: vec![notes],
        ..Default::default()
    };
    // The row scope is constructed first; header binding traversal then appends
    // siblings without changing either saved occurrence or physical schema order.
    let message = Scope {
        target_field: "Message_940".into(),
        children: vec![
            rows,
            scopes.remove("ST").unwrap(),
            scopes.remove("SE").unwrap(),
        ],
        ..Default::default()
    };
    let group = Scope {
        target_field: "Group".into(),
        children: vec![
            message,
            scopes.remove("GS").unwrap(),
            scopes.remove("GE").unwrap(),
        ],
        ..Default::default()
    };
    let interchange = Scope {
        target_field: "Interchange".into(),
        children: vec![
            group,
            scopes.remove("ISA").unwrap(),
            scopes.remove("IEA").unwrap(),
        ],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Key", ScalarType::String),
                SchemaNode::scalar("Optional", ScalarType::String),
            ],
        ),
        target,
        source_path: Some("rows.csv".into()),
        target_path: Some("report.x12".into()),
        source_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(','),
            has_header_row: Some(true),
            csv_preserve_empty_strings: preserve,
            ..Default::default()
        },
        target_options: FormatOptions {
            lenient_segments: true,
            edi_kind: Some(EdiBoundaryKind::X12),
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![interchange],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn expected_legacy_report() -> mfd::ImportReport {
    let location = "scope `Interchange/Group/Message_940/Rows/NTE`";
    mfd::ImportReport {
        executable: false,
        issues: vec![
            mfd::ImportIssue {
                kind: mfd::ImportIssueKind::Validation,
                message: format!("{location}: target field `NTE01` is bound more than once"),
            },
            mfd::ImportIssue {
                kind: mfd::ImportIssueKind::Validation,
                message: format!("{location}: target field `NTE02` is bound more than once"),
            },
        ],
    }
}

struct AuthoredFallbackPackage(PathBuf);

impl AuthoredFallbackPackage {
    fn new(name: &str, text: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-csv-x12-authored-fallback-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let package = Self(root);
        for file in ["Envelope.Config", "940.Config", "Defs.Segment"] {
            std::fs::copy(fixture(file), package.0.join(file)).unwrap();
        }
        std::fs::write(package.0.join(name), text).unwrap();
        std::fs::write(package.0.join("rows.csv"), "Key,Optional\na,A\n").unwrap();
        package
    }
}

impl Drop for AuthoredFallbackPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn csv_hidden_document_members_keep_complete_ordinary_partial_import_and_typed_refusal() {
    let extensions = [
        r#"<extension><component name="hidden" library="core" kind="2"><targets><datapoint key="2101"/></targets><data><constant value="OTHER" datatype="string"/></data></component></extension>"#,
        r#"<extension><connections><edge from="12" to="52"/></connections></extension>"#,
    ];
    for (name, preserve) in [("literal-direct.mfd", false), ("preserve-empty.mfd", true)] {
        let original = std::fs::read_to_string(fixture(name)).unwrap();
        for extension in extensions {
            assert_eq!(original.matches("</structure></component>").count(), 1);
            let text = original.replace(
                "</structure></component>",
                &format!("</structure>{extension}</component>"),
            );
            let package = AuthoredFallbackPackage::new(name, &text);
            let path = package.0.join(name);
            let canonical = path.canonicalize().unwrap();
            let expected = expected_legacy_project(preserve);
            let ordinary = mfd::import(&path).unwrap();
            assert_eq!(ordinary.mapping_path, canonical);
            assert_eq!(ordinary.warnings, Vec::<String>::new());
            assert_eq!(
                serde_json::to_value(&ordinary.project).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(mfd::assess_import(&ordinary), expected_legacy_report());
            let options = mfd::ImportOptions::default();
            let best =
                mfd::import_with_profile(&path, &options, mfd::ImportProfile::BestEffort).unwrap();
            assert_eq!(best.imported.mapping_path, canonical);
            assert_eq!(best.imported.warnings, Vec::<String>::new());
            assert_eq!(
                serde_json::to_value(&best.imported.project).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(best.report, expected_legacy_report());
            match mfd::import_with_profile(&path, &options, mfd::ImportProfile::Executable) {
                Err(mfd::MfdError::IncompatibleImport(report)) => {
                    assert_eq!(*report, expected_legacy_report())
                }
                Err(error) => panic!("unexpected typed import refusal: {error:?}"),
                Ok(_) => panic!("duplicate scalar bindings must not become executable"),
            }
        }
    }
}
