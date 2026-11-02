use super::fixtures::*;
use ir::{ScalarType, SchemaNode};
use mapping::*;
use web_demo::runtime::{DataFormat, DataSide, RuntimeError, parse_source, run, serialize_target};

fn identity_options() -> Vec<FormatOptions> {
    let mut options = vec![
        FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        FormatOptions {
            json_document: true,
            ..Default::default()
        },
    ];
    options.extend(
        [TabularBoundaryKind::Csv, TabularBoundaryKind::Xlsx].map(|kind| FormatOptions {
            tabular_kind: Some(kind),
            ..Default::default()
        }),
    );
    options.extend(
        [
            EdiBoundaryKind::X12,
            EdiBoundaryKind::Edifact,
            EdiBoundaryKind::Hl7,
            EdiBoundaryKind::Tradacoms,
            EdiBoundaryKind::Idoc,
            EdiBoundaryKind::SwiftMt,
        ]
        .map(|kind| FormatOptions {
            edi_kind: Some(kind),
            ..Default::default()
        }),
    );
    options
}

#[test]
fn manual_selectors_can_override_identity_only_flags_and_saved_suffixes() {
    for options in identity_options() {
        for suffix in [
            "xml", "json", "csv", "xlsx", "mft", "pdf", "bin", "idoc", "mt", "db", "x12",
        ] {
            let mut project = project();
            project.source_options = options.clone();
            project.target_options = options.clone();
            project.source_path = Some(format!("source.{suffix}"));
            project.target_path = Some(format!("target.{suffix}"));
            assert_eq!(
                run(&project, JSON, DataFormat::Json, DataFormat::Json).unwrap(),
                PRETTY_JSON
            );
            for format in FORMATS {
                // Missing XBRL metadata/invalid text/CSV row shape may fail,
                // but identities alone do not declare an unavailable codec.
                for error in [
                    parse_source(&project, "bad", format).err(),
                    serialize_target(&project, &name("Ada"), format).err(),
                ]
                .into_iter()
                .flatten()
                {
                    assert!(
                        !matches!(
                            error,
                            RuntimeError::UnsupportedBoundaryContract { .. }
                                | RuntimeError::FixedWidthNotExecutable { .. }
                        ),
                        "{error}"
                    );
                }
            }
        }
    }
}

#[test]
fn direct_helpers_admit_only_their_own_side() {
    for (_, _, options) in cases() {
        let mut source = project();
        source.target_options = options.clone();
        assert_eq!(
            parse_source(&source, JSON, DataFormat::Json).unwrap(),
            name("Ada")
        );
        let mut target = project();
        target.source_options = options;
        assert_eq!(
            serialize_target(&target, &name("Ada"), DataFormat::Json).unwrap(),
            PRETTY_JSON
        );
    }
}

#[test]
fn interchange_provenance_is_not_a_new_runtime_contract() {
    let options = FormatOptions {
        json_schema_unresolved_reference: Some("unavailable.schema.json".into()),
        mfd_decimal_input_names: [(41, "authored-decimal".into())].into(),
        mfd_optional_parser_input_names: [(42, "authored-optional-input".into())].into(),
        ..Default::default()
    };
    let mut project = project();
    project.source_options = options.clone();
    project.target_options = options;
    assert_eq!(
        parse_source(&project, JSON, DataFormat::Json).unwrap(),
        name("Ada")
    );
    assert_eq!(
        serialize_target(&project, &name("Ada"), DataFormat::Json).unwrap(),
        PRETTY_JSON
    );
    assert_eq!(
        run(&project, JSON, DataFormat::Json, DataFormat::Json).unwrap(),
        PRETTY_JSON
    );
}

#[test]
fn idoc_certification_metadata_is_not_itself_an_execution_declaration() {
    use std::num::NonZeroU32;
    let nz = |value| NonZeroU32::new(value).unwrap();
    let descriptor = IdocNativeConfig::new(
        "PEOPLE",
        vec![IdocNativeNode::Segment(
            IdocNativeSegment::new(
                "RECORD",
                "E1RECORD",
                false,
                nz(1),
                IdocNativeStatus::Mandatory,
                1,
                1,
                vec![
                    IdocNativeField::new(
                        "name",
                        "Name",
                        IdocNativeFieldType::Character,
                        nz(3),
                        nz(1),
                        nz(8),
                        nz(10),
                        vec![],
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        )],
    )
    .unwrap();
    let settings = IdocNativeTextSettings::new_observed_profile(
        false,
        false,
        false,
        IdocNativeValidationKind::ALL
            .into_iter()
            .map(|kind| IdocNativeValidationCase::new(kind, IdocNativeValidationAction::Stop))
            .collect(),
    )
    .unwrap();
    for metadata in [
        FormatOptions {
            idoc_native_config: Some(descriptor.clone()),
            ..Default::default()
        },
        FormatOptions {
            idoc_native_text_settings: Some(settings.clone()),
            ..Default::default()
        },
    ] {
        let mut project = project();
        project.source_options = metadata.clone();
        project.target_options = metadata;
        assert_eq!(
            parse_source(&project, JSON, DataFormat::Json).unwrap(),
            name("Ada")
        );
        assert_eq!(
            serialize_target(&project, &name("Ada"), DataFormat::Json).unwrap(),
            PRETTY_JSON
        );
        // Incomplete certification is still a validation error, not fabricated
        // browser runtime behavior or a newly supported IDoc boundary.
        assert!(matches!(
            run(&project, JSON, DataFormat::Json, DataFormat::Json),
            Err(RuntimeError::InvalidProject(_))
        ));
    }
    let (schema, layout) = descriptor.project().unwrap();
    let mut project = project();
    project.source = schema;
    project.source_options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Idoc),
        idoc: Some(layout),
        idoc_native_config: Some(descriptor),
        idoc_native_text_settings: Some(settings),
        ..Default::default()
    };
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("Ada".into()),
        },
    );
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    assert_eq!(
        run(&project, "bad", DataFormat::Json, DataFormat::Json),
        Err(RuntimeError::UnsupportedBoundaryContract {
            side: DataSide::Source,
            family: web_demo::runtime::UnsupportedContractFamily::Idoc,
        })
    );
}

#[test]
fn transport_metadata_does_not_require_network_acquisition_of_host_text() {
    let mut http = project();
    http.source_path = Some("https://invalid.example/source.json".into());
    http.source_options.http_get = Some(HttpGetOptions::default());
    let mut udf = project();
    udf.source_options.external_source = Some(
        ExternalSourceOptions::user_function(
            "captured",
            "host supplied result",
            ExternalPayloadFormat::Json,
        )
        .unwrap(),
    );
    let mut post = project();
    post.source_options.external_source = Some(
        ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::default(),
            None,
            None,
            ExternalPayloadFormat::Json,
            vec![],
        )
        .unwrap(),
    );
    for project in [http, udf, post] {
        assert!(
            engine::validate(&project).is_empty(),
            "{:?}",
            engine::validate(&project)
        );
        assert_eq!(
            parse_source(&project, JSON, DataFormat::Json).unwrap(),
            name("Ada")
        );
        assert_eq!(
            run(&project, JSON, DataFormat::Json, DataFormat::Json).unwrap(),
            PRETTY_JSON
        );
    }
}

#[test]
fn supported_xml_json_csv_and_json_dialects_keep_complete_bytes() {
    let mut project = project();
    assert_eq!(
        run(
            &project,
            "<Source><name>Ada</name></Source>",
            DataFormat::Xml,
            DataFormat::Json
        )
        .unwrap()
        .as_bytes(),
        PRETTY_JSON.as_bytes()
    );
    assert_eq!(
        run(&project, JSON, DataFormat::Json, DataFormat::Xml)
            .unwrap()
            .as_bytes(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Target>\n  <name>Ada</name>\n</Target>"
    );
    project.source_options.json5 = true;
    project.target_options.json5 = true;
    assert_eq!(
        run(
            &project,
            "// note\n{name:'Ada',}",
            DataFormat::Json,
            DataFormat::Json
        )
        .unwrap()
        .as_bytes(),
        b"{\n  name: \"Ada\",\n}\n"
    );
    project.source_options = FormatOptions {
        json_lines: true,
        ..Default::default()
    };
    project.target_options = project.source_options.clone();
    project.root.iteration = ScopeIteration::Source(vec![]);
    assert_eq!(
        run(
            &project,
            "{\"name\":\"Ada\"}\n{\"name\":\"Grace\"}\n",
            DataFormat::Json,
            DataFormat::Json
        )
        .unwrap()
        .as_bytes(),
        b"{\"name\":\"Ada\"}\n{\"name\":\"Grace\"}\n"
    );
    project.source_options = FormatOptions {
        delimiter: Some(';'),
        csv_quote: Some('\''),
        csv_preserve_empty_strings: true,
        ..Default::default()
    };
    project.target_options = FormatOptions {
        delimiter: Some('|'),
        has_header_row: Some(false),
        csv_utf8_bom: true,
        ..Default::default()
    };
    assert_eq!(
        run(
            &project,
            "name\n'Ada;Lovelace'\n''\n",
            DataFormat::Csv,
            DataFormat::Csv
        )
        .unwrap()
        .as_bytes(),
        b"\xef\xbb\xbfAda;Lovelace\n\"\"\n"
    );
}

#[test]
fn supported_xbrl_source_keeps_the_complete_mapping_result() {
    let mut project = project();
    project.source = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "rows",
                vec![
                    SchemaNode::group(
                        "period",
                        vec![SchemaNode::scalar("instant", ScalarType::String)],
                    ),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.source_options.xbrl = Some(
        XbrlBoundaryOptions::external_source("taxonomy.xsd")
            .unwrap()
            .with_namespace_bindings(vec![
                XbrlNamespaceBinding::new(
                    vec!["rows".into(), "period".into()],
                    "http://www.xbrl.org/2003/instance",
                )
                .unwrap(),
                XbrlNamespaceBinding::new(
                    vec!["rows".into(), "period".into(), "instant".into()],
                    "http://www.xbrl.org/2003/instance",
                )
                .unwrap(),
                XbrlNamespaceBinding::new(vec!["rows".into(), "Label".into()], "urn:facts")
                    .unwrap(),
            ])
            .unwrap(),
    );
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["rows".into(), "Label".into()],
            frame: None,
        },
    );
    let text = r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:f="urn:facts"><xbrli:context id="c"><xbrli:entity><xbrli:identifier scheme="urn:id">Entity</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context><f:Label contextRef="c">Ada</f:Label></xbrli:xbrl>"#;
    assert_eq!(
        run(&project, text, DataFormat::Xbrl, DataFormat::Json)
            .unwrap()
            .as_bytes(),
        PRETTY_JSON.as_bytes()
    );
}

#[test]
fn dependency_errors_keep_their_specific_diagnostics_at_every_entry_point() {
    for (_, _, unsupported) in cases() {
        for side in [DataSide::Source, DataSide::Target] {
            for json5 in [true, false] {
                let mut project = project();
                let options = match side {
                    DataSide::Source => &mut project.source_options,
                    DataSide::Target => &mut project.target_options,
                };
                *options = unsupported.clone();
                let expected = if json5 {
                    let dependency =
                        Json5InterchangeDependency::ComponentSetting { value: "1".into() };
                    options.json5_interchange_dependency = Some(dependency.clone());
                    RuntimeError::Json5InterchangeDependency { side, dependency }
                } else {
                    let dependency = CsvTextRepairDependency::new(CsvTextRepairCause::Encoding);
                    options.csv_text_repair_dependency = Some(dependency);
                    RuntimeError::CsvRepairDependency { side, dependency }
                };
                for format in FORMATS {
                    let direct = match side {
                        DataSide::Source => parse_source(&project, "bad", format).map(|_| ()),
                        DataSide::Target => {
                            serialize_target(&project, &name("Ada"), format).map(|_| ())
                        }
                    };
                    assert_eq!(direct, Err(expected.clone()));
                    assert_eq!(run(&project, "bad", format, format), Err(expected.clone()));
                }
            }
        }
    }
}

#[test]
fn xbrl_extra_source_preflight_priority_is_unchanged() {
    let mut project = project();
    project.source_options.flextext = Some(flextext("Source"));
    project.extra_sources.push(NamedSource {
        name: "filing".into(),
        path: "filing.xbrl".into(),
        schema: SchemaNode::group("filing", vec![]),
        options: FormatOptions {
            xbrl: Some(XbrlBoundaryOptions::external_source("taxonomy.xsd").unwrap()),
            ..Default::default()
        },
        dynamic_path: None,
    });
    assert_eq!(
        run(&project, "bad", DataFormat::Json, DataFormat::Json),
        Err(RuntimeError::XbrlExtraSourceNotExecutable {
            name: "filing".into()
        })
    );
}
