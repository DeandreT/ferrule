use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiBoundaryKind, FormatOptions, Graph, Node, Project, Scope, ScopeIteration,
    ScopeSequence,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/enclosing-frame-clones")
        .join(name)
}

fn bounded(node: SchemaNode) -> SchemaNode {
    node.repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}

fn scalar(name: &str, kind: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, kind)
}

fn source_schema(design: &str) -> SchemaNode {
    let mut rows = SchemaNode::group(
        "Rows",
        vec![
            scalar("Count", ScalarType::Int),
            scalar("Note", ScalarType::String),
            bounded(SchemaNode::group(
                "Items",
                vec![scalar("Value", ScalarType::Int)],
            )),
        ],
    );
    if design == "guaranteed" {
        rows = rows.with_required_fields(vec!["Count".into()]).unwrap();
    }
    let rows = bounded(rows);
    SchemaNode::group(
        "Source",
        if design == "nested" {
            vec![bounded(SchemaNode::group("Sections", vec![rows]))]
        } else {
            vec![rows]
        },
    )
}

fn target_schema(design: &str) -> SchemaNode {
    if design == "edi" {
        let text_segment = |name| SchemaNode::group(name, vec![scalar("Text", ScalarType::String)]);
        let st_text = SchemaNode::scalar_fixed("Text", ScalarType::String, "850");
        return SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        text_segment("ISA"),
                        SchemaNode::group(
                            "Group",
                            vec![
                                text_segment("GS"),
                                SchemaNode::group(
                                    "Message_850",
                                    vec![
                                        SchemaNode::group("ST", vec![st_text]),
                                        SchemaNode::group(
                                            "Loop0100",
                                            vec![SchemaNode::group(
                                                "N1",
                                                vec![
                                                    scalar("Text", ScalarType::String),
                                                    scalar("Count", ScalarType::Int),
                                                ],
                                            )],
                                        )
                                        .repeating(),
                                        text_segment("SE"),
                                    ],
                                )
                                .repeating(),
                                text_segment("GE").repeating(),
                            ],
                        )
                        .repeating(),
                        text_segment("IEA").repeating(),
                    ],
                )
                .repeating(),
            ],
        );
    }
    let optional = |name, kind| {
        let mut node = scalar(name, kind);
        node.xml_optional = true;
        node
    };
    let batch = SchemaNode::group(
        "Batch",
        vec![
            SchemaNode::group(
                "Record",
                vec![
                    optional(
                        "Type",
                        if design == "false-zero-empty" {
                            ScalarType::Bool
                        } else {
                            ScalarType::String
                        },
                    ),
                    optional("Count", ScalarType::Int),
                    optional("Note", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    SchemaNode::group(
        "Result",
        if design == "nested" {
            vec![SchemaNode::group("Section", vec![batch]).repeating()]
        } else {
            vec![batch]
        },
    )
}

/// Import derives each XML target's output schema-location hint from its schema.
fn with_schema_hint(design: &str, mut project: Project) -> Project {
    if design == "edi" {
        return project;
    }
    let location = match design {
        "false-zero-empty" => "bool-target.xsd",
        "nested" => "nested-target.xsd",
        _ => "target.xsd",
    };
    project.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some(location.into()),
        derivation: Some(ir::XmlSchemaHintDerivation {
            schema_reference: location.into(),
        }),
        ..Default::default()
    });
    project
}

fn expected(design: &str) -> Project {
    if design == "false-zero-empty" {
        let mut project = expected("implicit");
        project.target = target_schema(design);
        project.graph.nodes = BTreeMap::from([
            (
                0,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::Int(0),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String(String::new()),
                },
            ),
        ]);
        return project;
    }
    if design == "many-branches" {
        let mut project = expected("literal-only");
        project.graph.nodes.remove(&1);
        let segment = Scope {
            bindings: vec![Binding {
                target_field: "Type".into(),
                node: 0,
            }],
            ..Default::default()
        };
        project.root.children[0].children[0].iteration =
            ScopeIteration::Concatenate(ScopeSequence::new(segment.clone(), vec![segment; 63]));
        return project;
    }
    let prefix = if design == "nested" {
        vec!["Sections".into(), "Rows".into()]
    } else {
        vec!["Rows".into()]
    };
    let mut nodes = BTreeMap::new();
    if design != "guaranteed" {
        nodes.insert(
            0,
            Node::Const {
                value: Value::String("left".into()),
            },
        );
        nodes.insert(
            1,
            Node::Const {
                value: Value::String("right".into()),
            },
        );
    }
    let binding = |field: &str, node| Binding {
        target_field: field.into(),
        node,
    };
    let mut left = Scope::default();
    let mut right = Scope::default();
    let mut count = 2;
    let mut note = 3;
    if design == "guaranteed" {
        count = 0;
        note = 1;
    }
    if design == "wrapped-markers" {
        count = 3;
        note = 2;
    }
    if design != "literal-only" {
        if design == "mixed" {
            let mut items = prefix.clone();
            items.push("Items".into());
            nodes.insert(
                2,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: Some(items),
                },
            );
            count = 3;
            note = 4;
            left.iteration = ScopeIteration::Source(vec!["Items".into()]);
        }
        nodes.insert(
            count,
            Node::SourceField {
                path: vec!["Count".into()],
                frame: Some(prefix.clone()),
            },
        );
        if design != "edi" {
            nodes.insert(
                note,
                Node::SourceField {
                    path: vec!["Note".into()],
                    frame: Some(prefix),
                },
            );
        }
    }
    let text = if design == "edi" { "Text" } else { "Type" };
    if design != "guaranteed" {
        left.bindings.push(binding(text, 0));
        right.bindings.push(binding(text, 1));
    }
    if design != "literal-only" {
        left.bindings
            .push(binding("Count", if design == "mixed" { 2 } else { count }));
        right.bindings.push(binding("Count", count));
        if design != "edi" {
            if design != "mixed" {
                left.bindings.push(binding("Note", note));
            }
            right.bindings.push(binding("Note", note));
        }
    }
    if design == "wrapped-markers" {
        left.bindings.reverse();
    }
    let field = if design == "edi" {
        left.children.push(Scope {
            target_field: "N1".into(),
            bindings: std::mem::take(&mut left.bindings),
            ..Default::default()
        });
        right.children.push(Scope {
            target_field: "N1".into(),
            bindings: std::mem::take(&mut right.bindings),
            ..Default::default()
        });
        "Loop0100"
    } else {
        "Record"
    };
    let cloned = Scope {
        target_field: field.into(),
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(left, vec![right])),
        ..Default::default()
    };
    let mut parent = Scope {
        target_field: if design == "edi" {
            "Message_850"
        } else {
            "Batch"
        }
        .into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        children: vec![cloned],
        ..Default::default()
    };
    if design == "nested" {
        parent = Scope {
            target_field: "Section".into(),
            iteration: ScopeIteration::Source(vec!["Sections".into()]),
            children: vec![parent],
            ..Default::default()
        };
    } else if design == "edi" {
        parent = Scope {
            target_field: "Interchange".into(),
            children: vec![Scope {
                target_field: "Group".into(),
                children: vec![parent],
                ..Default::default()
            }],
            ..Default::default()
        };
    }
    Project {
        source: source_schema(design),
        target: target_schema(design),
        source_path: Some("source.json".into()),
        target_path: Some(
            if design == "edi" {
                "report.x12"
            } else {
                "report.xml"
            }
            .into(),
        ),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: if design == "edi" {
            FormatOptions {
                lenient_segments: true,
                edi_kind: Some(EdiBoundaryKind::X12),
                ..Default::default()
            }
        } else {
            FormatOptions {
                xml_document: true,
                ..Default::default()
            }
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![parent],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

#[test]
fn declared_enclosing_frame_clones_preserve_complete_projects_and_values() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/enclosing-frame-clones/cases.json")).unwrap();
    for design in [
        "implicit",
        "literal-only",
        "guaranteed",
        "mixed",
        "wrapped-markers",
        "nested",
        "edi",
        "many-branches",
        "false-zero-empty",
        "unconnected-declaration",
        "idempotent-physical",
    ] {
        let imported = mfd::import(&fixture(&format!("{design}.mfd"))).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(with_schema_hint(design, expected(design))).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        for case in cases.iter().filter(|case| {
            case["design"] == design || design != "nested" && case["design"] == "all-flat"
        }) {
            let source =
                format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if case["reject"] == true {
                assert!(source.is_err(), "{design}: {case}");
                continue;
            }
            let actual = engine::run(&imported.project, &source.unwrap()).unwrap();
            let actual: serde_json::Value = serde_json::from_str(
                &format_json::to_string(&imported.project.target, &actual).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, case["expected"], "{design}: {case}");
        }
    }
}

#[test]
fn unproved_clone_presence_frames_controls_and_competing_feeds_remain_refusals() {
    let original = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    let no_literals = original
        .replace("<edge from=\"101\" to=\"51\"/>", "")
        .replace("<edge from=\"102\" to=\"61\"/>", "");
    let duplicate = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="101"/></targets><data><constant value="other" datatype="string"/></data></component>"#;
    let hidden_output = |key| {
        format!(
            r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="999"/><datapoint pos="0" key="{key}"/></targets><data><constant value="other" datatype="string"/></data></component>"#
        )
    };
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="120"/><datapoint pos="1" key="121"/></sources><targets><datapoint key="110"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint key="105"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let controls = [
        ("unproved optional presence", no_literals),
        ("uniterated source frame", original.replace("<edge from=\"10\" to=\"40\"/>", "")),
        ("competing literal producers", original.replace("</children>", &format!("{duplicate}</children>"))),
        ("uniterated descendant frame", original.replace("<edge from=\"12\" to=\"52\"/>", "<edge from=\"18\" to=\"52\"/>")),
        ("physical source collision", original.replace("outkey=\"12\"", "outkey=\"101\"")),
        ("controlled parent", original.replace("</children>", &format!("{filter}</children>")).replace("<edge from=\"10\" to=\"40\"/>", "<edge from=\"10\" to=\"120\"/><edge from=\"105\" to=\"121\"/><edge from=\"110\" to=\"40\"/>")),
        ("competing fields in one branch", original.replace("<entry name=\"Count\" inpkey=\"52\"/>", "<entry name=\"Count\" inpkey=\"52\"/><entry name=\"Count\" inpkey=\"54\"/>").replace("</connections>", "<edge from=\"12\" to=\"54\"/></connections>")),
        ("unproved descendant group presence", original.replace("<entry name=\"Note\" inpkey=\"53\"/>", "<entry name=\"Detail\"><entry name=\"Value\" inpkey=\"53\"/></entry>")),
        ("branch bound exceeded", std::fs::read_to_string(fixture("many-branches.mfd")).unwrap().replace("</connections>", "<edge from=\"101\" to=\"364\"/></connections>").replace("</entry></entry></root><document", "<entry name=\"Record\" clone=\"1\"><entry name=\"Type\" inpkey=\"364\"/></entry></entry></entry></root><document")),
        ("nullable scalar assignment", original.clone()),
        ("one input key crosses declared branches", original.replace("inpkey=\"62\"", "inpkey=\"52\"").replace("<edge from=\"12\" to=\"62\"/>", "")),
        ("literal has undeclared output pin", original.replace("<datapoint key=\"101\"/>", "<datapoint key=\"101\"/><datapoint/>")),
        ("target input node default", original.replace("<entry name=\"Count\" inpkey=\"52\"/>", "<entry name=\"Count\" inpkey=\"52\"><inputnodefunctions><rule applyto=\"self\"><default value=\"7\"/></rule></inputnodefunctions></entry>")),
        ("unconnected clone has a target default", original.replace("<entry name=\"Count\" inpkey=\"52\"/>", "<entry name=\"Count\" inpkey=\"52\"><inputnodefunctions><rule applyto=\"self\"><default value=\"7\"/></rule></inputnodefunctions></entry>").replace("<edge from=\"101\" to=\"51\"/>", "").replace("<edge from=\"12\" to=\"52\"/>", "").replace("<edge from=\"14\" to=\"53\"/>", "").replace("<edge from=\"102\" to=\"61\"/>", "").replace("<edge from=\"12\" to=\"62\"/>", "").replace("<edge from=\"14\" to=\"63\"/>", "")),
        ("target cast policy", original.replace("<entry name=\"Result\">", "<entry name=\"document\" casttotargettypemode=\"cast-in-subtree\"><entry name=\"Result\">").replace("</entry></entry></root><document", "</entry></entry></entry></root><document")),
        ("parent selected a different structural object", original.replace("<entry name=\"Items\" type=\"json-property\">", "<entry name=\"Detail\" type=\"json-property\"><entry name=\"object\" outkey=\"19\"><entry name=\"Value\" type=\"json-property\"><entry name=\"integer\" outkey=\"20\"/></entry></entry></entry><entry name=\"Items\" type=\"json-property\">").replace("<edge from=\"10\" to=\"40\"/>", "<edge from=\"19\" to=\"40\"/>").replace("<edge from=\"12\" to=\"52\"/>", "<edge from=\"20\" to=\"52\"/>").replace("<edge from=\"12\" to=\"62\"/>", "<edge from=\"20\" to=\"62\"/>")),
        ("raw hidden literal producer", original.replace("</children>", &format!("{}</children>", hidden_output(101)))),
        ("raw hidden scalar producer", original.replace("</children>", &format!("{}</children>", hidden_output(12)))),
        ("raw hidden parent frame producer", original.replace("</children>", &format!("{}</children>", hidden_output(10)))),
        ("same physical path has differing raw roles", original.replace("<entry name=\"integer\" outkey=\"12\"/>", "<entry name=\"integer\" outkey=\"12\"/><entry name=\"integer\" outkey=\"12\" expanded=\"1\"/>")),
        ("physical output key declares different paths", original.replace("outkey=\"14\"", "outkey=\"12\"")),
        ("physical output key has an unlowered declaration", original.replacen("</root>", "<entry name=\"ignored\" outkey=\"12\"/></root>", 1)),
        ("target scalar key has an unlowered declaration", original.replace("</root><document schema=\"target.xsd\"", "<entry name=\"ignored\" inpkey=\"52\"/></root><document schema=\"target.xsd\"")),
        ("target scalar normalization hides a selected branch input", original.replace("</entry></entry></root><document schema=\"target.xsd\"", "</entry><entry name=\"Unused\" inpkey=\"52\"/></entry></root><document schema=\"target.xsd\"")),
        ("target parent key has an unlowered declaration", original.replace("</root><document schema=\"target.xsd\"", "<entry name=\"ignored\" inpkey=\"40\"/></root><document schema=\"target.xsd\"")),
        ("literal raw input declaration", original.replace("<targets><datapoint key=\"101\"/>", "<sources><datapoint/></sources><targets><datapoint key=\"101\"/>")),
        ("literal raw output position", original.replace("<datapoint key=\"101\"/>", "<datapoint pos=\"bad\" key=\"101\"/>")),
        ("physical scalar key crosses raw directions", original.replace("outkey=\"12\"", "outkey=\"12\" inpkey=\"12\"")),
    ];
    for (index, (label, design)) in controls.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_enclosing_clone_refusal_{}_{}",
            std::process::id(),
            index
        ));
        std::fs::create_dir(&path).unwrap();
        for file in ["source.schema.json", "target.xsd"] {
            std::fs::copy(fixture(file), path.join(file)).unwrap();
        }
        if label == "nullable scalar assignment" {
            let mut source: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.join("source.schema.json")).unwrap())
                    .unwrap();
            source["properties"]["Rows"]["items"]["properties"]["Count"]["type"] =
                serde_json::json!(["integer", "null"]);
            std::fs::write(
                path.join("source.schema.json"),
                serde_json::to_vec_pretty(&source).unwrap(),
            )
            .unwrap();
        }
        if label == "parent selected a different structural object" {
            let mut source: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.join("source.schema.json")).unwrap())
                    .unwrap();
            source["properties"]["Rows"]["items"]["properties"]["Detail"] = serde_json::json!({"type":"object","additionalProperties":false,"properties":{"Value":{"type":"integer"}}});
            std::fs::write(
                path.join("source.schema.json"),
                serde_json::to_vec_pretty(&source).unwrap(),
            )
            .unwrap();
        }
        let schema = std::fs::read_to_string(path.join("target.xsd")).unwrap().replace(
            "<xs:element name=\"Note\" type=\"xs:string\" minOccurs=\"0\"/>",
            "<xs:element name=\"Note\" type=\"xs:string\" minOccurs=\"0\"/><xs:element name=\"Detail\" minOccurs=\"0\"><xs:complexType><xs:sequence><xs:element name=\"Value\" type=\"xs:string\" minOccurs=\"0\"/></xs:sequence></xs:complexType></xs:element>",
        );
        std::fs::write(path.join("target.xsd"), schema).unwrap();
        let mapping = path.join("mapping.mfd");
        std::fs::write(&mapping, design).unwrap();
        let imported = mfd::import(&mapping).unwrap();
        assert!(
            imported.warnings.iter().any(|warning| warning ==
                "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"),
            "{label}: unqualified declared clones were admitted: {:?}", imported.warnings
        );
        assert!(
            matches!(
                mfd::import_with_profile(
                    &mapping,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                ),
                Err(mfd::MfdError::IncompatibleImport(_))
            ),
            "{label}"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}
