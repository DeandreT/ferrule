//! Self-authored EDI document plans, with shared cross-document grouping.
use super::*;
use mapping::{
    DocumentProject, EdiBoundaryKind, EmptyFileSelection, FormatOptions, PrimaryFileSelection,
    PrimarySourcePlan,
};

pub const MODELS: [&str; 20] = [
    "x12_direct",
    "x12_wildcard",
    "edifact_direct",
    "edifact_wildcard",
    "hl7_direct",
    "hl7_wildcard",
    "tradacoms_direct",
    "tradacoms_wildcard",
    "idoc_direct",
    "idoc_wildcard",
    "edifact_paths",
    "edifact_lexical",
    "edifact_autocomplete",
    "x12_autocomplete",
    "edifact_lenient_partial",
    "edifact_v4",
    "edifact_v3_literal",
    "hl7_short_msh",
    "hl7_short_fhs",
    "hl7_short_bhs",
];

pub fn document(model: &str) -> DocumentProject {
    if let Some(header) = match model {
        "hl7_short_msh" => Some("MSH"),
        "hl7_short_fhs" => Some("FHS"),
        "hl7_short_bhs" => Some("BHS"),
        _ => None,
    } {
        let original = document("hl7_direct");
        let mut project = original.project().clone();
        let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
            unreachable!()
        };
        children[0].name = header.into();
        let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
            unreachable!()
        };
        children.truncate(1);
        project.root.children[0].target_field = header.into();
        project.root.children[0]
            .bindings
            .retain(|binding| binding.target_field == "F0");
        return DocumentProject::new(project, original.primary_source().clone());
    }
    if matches!(model, "edifact_v4" | "edifact_v3_literal") {
        let original = document("edifact_direct");
        let mut project = original.project().clone();
        let field = |name: &str| SchemaNode::scalar(name, ScalarType::String);
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "UNB",
                    vec![
                        SchemaNode::group(
                            "Syntax",
                            vec![
                                field("Name"),
                                SchemaNode::scalar("Version", ScalarType::Int),
                            ],
                        ),
                        field("Sender"),
                        field("Receiver"),
                        SchemaNode::group("DateTime", vec![field("Date"), field("Time")]),
                        field("Reference"),
                    ],
                ),
                SchemaNode::group("UNH", vec![field("Reference")]),
                SchemaNode::group(
                    "BGM",
                    vec![
                        field("F1"),
                        if model == "edifact_v4" {
                            field("F2").repeating()
                        } else {
                            field("F2")
                        },
                    ],
                ),
                SchemaNode::group(
                    "UNT",
                    vec![
                        SchemaNode::scalar("Count", ScalarType::Int),
                        field("Reference"),
                    ],
                ),
                SchemaNode::group(
                    "UNZ",
                    vec![
                        SchemaNode::scalar("Count", ScalarType::Int),
                        field("Reference"),
                    ],
                ),
            ],
        );
        project.source = schema.clone();
        project.target = schema;
        project.graph.nodes.retain(|id, _| *id == 20);
        project.root = Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        };
        return DocumentProject::new(project, original.primary_source().clone());
    }
    let index = MODELS
        .iter()
        .position(|name| *name == model)
        .expect("authored model");
    let wildcard = index < 10 && index % 2 == 1 || index == 10;
    let kind = [
        EdiBoundaryKind::X12,
        EdiBoundaryKind::Edifact,
        EdiBoundaryKind::Hl7,
        EdiBoundaryKind::Tradacoms,
        EdiBoundaryKind::Idoc,
    ][if index < 10 {
        index / 2
    } else if index == 13 {
        0
    } else {
        1
    }];
    let mut project = super::xml_projects::project(
        if wildcard {
            Node::RuntimeParameter {
                name: "Input".into(),
                ty: ScalarType::String,
                preview: Some("never-parameter-preview.edi".into()),
            }
        } else {
            Node::Const {
                value: Value::String("orders/input.edi".into()),
            }
        },
        false,
    );
    let options = FormatOptions {
        edi_kind: Some(kind),
        ..Default::default()
    };
    project.source_options = options.clone();
    project.target_options = options;
    project.source_path = Some("never-preview.edi".into());
    project.target_path = Some("never-preview-output.edi".into());
    for schema in [&mut project.source, &mut project.target] {
        let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        children[0].name = "DTL".into();
    }
    for node in project.graph.nodes.values_mut() {
        match node {
            Node::SourceField {
                frame: Some(frame), ..
            } => frame[0] = "DTL".into(),
            Node::Aggregate { collection, .. } => collection[0] = "DTL".into(),
            _ => {}
        }
    }
    project.root.children[0].target_field = "DTL".into();
    project.root.children[0].iteration = ScopeIteration::Source(vec!["DTL".into()]);
    let header = match kind {
        EdiBoundaryKind::X12 => Some((
            "ISA",
            vec![
                "00",
                "          ",
                "00",
                "          ",
                "ZZ",
                "SENDER         ",
                "ZZ",
                "RECEIVER       ",
                "261025",
                "1200",
                "U",
                "00401",
                "000000001",
                "0",
                "P",
                ":",
            ],
        )),
        EdiBoundaryKind::Edifact => Some(("UNH", vec!["1"])),
        EdiBoundaryKind::Hl7 => Some(("MSH", vec!["|", "^~\\&"])),
        EdiBoundaryKind::Tradacoms => Some(("STX", vec!["ANA"])),
        _ => None,
    };
    if let Some((name, fields)) = header {
        let schema = SchemaNode::group(
            name,
            fields
                .iter()
                .enumerate()
                .map(|(at, _)| string(&format!("F{at}")))
                .collect(),
        );
        for endpoint in [&mut project.source, &mut project.target] {
            let ir::SchemaKind::Group { children, .. } = &mut endpoint.kind else {
                unreachable!()
            };
            children.insert(0, schema.clone());
        }
        let bindings = fields
            .iter()
            .enumerate()
            .map(|(at, text)| {
                let node = 100 + at as u32;
                project.graph.nodes.insert(
                    node,
                    Node::Const {
                        value: Value::String((*text).into()),
                    },
                );
                Binding {
                    target_field: format!("F{at}"),
                    node,
                }
            })
            .collect();
        project.root.children.insert(
            0,
            Scope {
                target_field: name.into(),
                bindings,
                ..Default::default()
            },
        );
    }
    if kind == EdiBoundaryKind::Idoc {
        fn layout(output: bool) -> mapping::IdocLayout {
            let mut names = vec![("Key", 5, 6), ("Text", 7, 18), ("Count", 19, 21)];
            if output {
                names.push(("Status", 22, 25));
            }
            mapping::IdocLayout::new(vec![
                mapping::IdocSegmentLayout::new(
                    "DTL",
                    names
                        .into_iter()
                        .map(|(name, first, last)| {
                            mapping::IdocFieldLayout::new(
                                name,
                                std::num::NonZeroU32::new(first).unwrap(),
                                std::num::NonZeroU32::new(last).unwrap(),
                            )
                            .unwrap()
                        })
                        .collect(),
                )
                .unwrap(),
            ])
            .unwrap()
        }
        project.source_options.idoc = Some(layout(false));
        project.target_options.idoc = Some(layout(true));
    }
    if index == 10 {
        project.graph.nodes.insert(21, Node::SourceDocumentPath);
        project.graph.nodes.insert(
            4,
            Node::Call {
                function: "remove_folder".into(),
                args: vec![21],
            },
        );
        for (id, field) in [(0, "Key"), (2, "Text"), (3, "Count")] {
            project.graph.nodes.insert(
                id,
                Node::SourceField {
                    path: vec!["DTL".into(), field.into()],
                    frame: None,
                },
            );
        }
        let scope = project.root.children.last_mut().unwrap();
        scope.iteration = ScopeIteration::Source(vec![]);
        scope.group_by = None;
    }
    if index == 11 {
        for schema in [&mut project.source, &mut project.target] {
            let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            let ir::SchemaKind::Group { children, .. } = &mut children[1].kind else {
                unreachable!()
            };
            children[2] = SchemaNode::scalar("Count", ScalarType::Float);
        }
        project
            .source_options
            .edi_implied_decimals
            .push(mapping::EdiImpliedDecimal::new(vec!["DTL".into(), "Count".into()], 2).unwrap());
        project.target_options.edi_lexical_formats.push(
            mapping::EdiLexicalFormat::new(
                vec!["DTL".into(), "Text".into()],
                mapping::EdiLexicalKind::CompactDate8,
            )
            .unwrap(),
        );
        project.target_options.edi_value_constraints.push(
            mapping::EdiValueConstraint::new(
                vec!["DTL".into(), "Text".into()],
                8,
                8,
                vec!["20261025".into()],
            )
            .unwrap(),
        );
    }
    if index == 12 {
        project.target_options.edi_autocomplete = Some(mapping::EdiAutocomplete::Edifact(
            mapping::EdifactAutocomplete {
                syntax_level: None,
                syntax_version: None,
                controlling_agency: Some("UNO".into()),
                message_type: Some("INVOIC".into()),
            },
        ));
    }
    if index == 13 {
        for schema in [&mut project.source, &mut project.target] {
            schema.name = "ISA".into();
            let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[0].name = "MF_ISA_2".into();
        }
        project.root.children[0].target_field = "MF_ISA_2".into();
        project.target_options.edi_autocomplete =
            Some(mapping::EdiAutocomplete::X12(mapping::X12Autocomplete {
                request_acknowledgement: false,
                transaction_set: Some("810".into()),
            }));
        for (position, name, values) in [
            (
                1,
                "GS",
                vec![
                    "IN", "SENDER", "RECEIVER", "20261025", "120000", "1", "X", "004010",
                ],
            ),
            (2, "ST", vec!["810", "0001"]),
        ] {
            let schema = SchemaNode::group(
                name,
                values
                    .iter()
                    .enumerate()
                    .map(|(at, _)| string(&format!("F{at}")))
                    .collect(),
            );
            let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
                unreachable!()
            };
            children.insert(position, schema);
            let bindings = values
                .iter()
                .enumerate()
                .map(|(at, value)| {
                    let node = 200 + position as u32 * 20 + at as u32;
                    project.graph.nodes.insert(
                        node,
                        Node::Const {
                            value: Value::String((*value).into()),
                        },
                    );
                    Binding {
                        target_field: format!("F{at}"),
                        node,
                    }
                })
                .collect();
            project.root.children.insert(
                position,
                Scope {
                    target_field: name.into(),
                    bindings,
                    ..Default::default()
                },
            );
        }
    }
    if index == 14 {
        let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
            unreachable!()
        };
        children.remove(0);
        project.source_options.lenient_segments = true;
    }
    DocumentProject::new(
        project,
        PrimarySourcePlan::new(20).with_selection(if wildcard {
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::Refuse,
            }
        } else {
            PrimaryFileSelection::Direct
        }),
    )
}
