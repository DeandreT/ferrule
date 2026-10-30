use super::*;
use ir::ScalarType;
use std::num::NonZeroU32;

#[test]
fn failure_rules_roundtrip_in_declaration_order() {
    let project = Project {
        source: ir::SchemaNode::group("Source", Vec::new()),
        target: ir::SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["Rows".into()],
                },
                selection: FailureSelection::WhenFalse { predicate: 4 },
                message: Some(5),
            },
            FailureRule {
                iteration: FailureIteration::Sequence {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 6,
                        item: 7,
                    },
                },
                selection: FailureSelection::All,
                message: None,
            },
        ],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };

    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();

    assert_eq!(encoded.matches("\"message\"").count(), 1);
    assert!(!encoded.contains("user_functions"));
    assert_eq!(decoded.failure_rules, project.failure_rules);
    assert!(decoded.user_functions.is_empty());
}

#[test]
fn user_defined_functions_roundtrip_with_stable_identities() {
    let function_id = FunctionId::new(17);
    let left = FunctionParameterId::new(23);
    let right = FunctionParameterId::new(29);
    let function = UserFunction {
        library: "invoice".into(),
        name: "format_line".into(),
        description: Some("Formats one invoice line".into()),
        parameters: vec![
            FunctionParameter {
                id: left,
                name: "sku".into(),
                ty: ScalarType::String,
            },
            FunctionParameter {
                id: right,
                name: "quantity".into(),
                ty: ScalarType::Int,
            },
        ],
        output_name: "line".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: [
                (1, Node::FunctionParameter { parameter: left }),
                (2, Node::FunctionParameter { parameter: right }),
                (
                    3,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![1, 2],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        output: 3,
    };
    let project = Project {
        source: ir::SchemaNode::group("Source", Vec::new()),
        target: ir::SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: [(function_id, function)].into_iter().collect(),
        graph: Graph {
            nodes: [
                (
                    10,
                    Node::Const {
                        value: ir::Value::String("ABC".into()),
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: ir::Value::Int(2),
                    },
                ),
                (
                    12,
                    Node::UserFunctionCall {
                        function: function_id,
                        args: vec![10, 11],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope::default(),
    };

    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();
    let decoded_function = &decoded.user_functions[&function_id];

    assert_eq!(function_id.get(), 17);
    assert_eq!(decoded_function.library, "invoice");
    assert_eq!(decoded_function.name, "format_line");
    assert_eq!(
        decoded_function.description.as_deref(),
        Some("Formats one invoice line")
    );
    assert_eq!(decoded_function.parameters[0].id.get(), 23);
    assert_eq!(decoded_function.parameters[1].id.get(), 29);
    assert_eq!(decoded_function.output_name, "line");
    assert_eq!(decoded_function.output_type, ScalarType::String);
    assert_eq!(decoded_function.output, 3);
    assert!(matches!(
        decoded_function.body.nodes.get(&1),
        Some(Node::FunctionParameter { parameter }) if *parameter == left
    ));
    assert!(matches!(
        decoded.graph.nodes.get(&12),
        Some(Node::UserFunctionCall { function, args })
            if *function == function_id && args == &[10, 11]
    ));
}

#[test]
fn user_defined_function_nodes_report_graph_dependencies() {
    let parameter = Node::FunctionParameter {
        parameter: FunctionParameterId::new(5),
    };
    let call = Node::UserFunctionCall {
        function: FunctionId::new(8),
        args: vec![13, 21, 34],
    };

    assert!(parameter.dependencies().is_empty());
    assert_eq!(call.dependencies(), vec![13, 21, 34]);
}

#[test]
fn json_lines_format_option_defaults_off_and_roundtrips_when_enabled() {
    let defaults: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(!defaults.json_lines);
    assert!(defaults.edi_kind.is_none());
    assert!(defaults.edi_config_reference.is_none());
    assert!(defaults.edi_implied_decimals.is_empty());
    assert!(defaults.edi_lexical_formats.is_empty());
    assert!(defaults.edi_value_constraints.is_empty());
    assert!(defaults.x12_separators.is_none());
    assert!(defaults.x12_interchange_version.is_none());
    assert!(!defaults.xml_document);
    assert!(!defaults.local_xml_file_set);
    assert!(!defaults.json_document);
    assert!(defaults.tabular_kind.is_none());
    assert!(defaults.fixed_width.is_none());
    assert!(defaults.flextext.is_none());
    assert!(defaults.idoc.is_none());
    assert!(defaults.idoc_native_config.is_none());
    assert!(defaults.idoc_native_text_settings.is_none());
    assert!(defaults.swift_mt.is_none());
    assert!(defaults.swift_mt_output.is_none());
    assert!(defaults.pdf.is_none());
    assert!(defaults.http_get.is_none());
    assert!(defaults.external_source.is_none());
    assert!(defaults.protobuf.is_none());
    assert!(
        !serde_json::to_string(&defaults)
            .unwrap()
            .contains("json_lines")
    );

    let options = FormatOptions {
        json_lines: true,
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&options).unwrap();
    assert!(encoded.contains("\"json_lines\":true"));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.json_lines);
}

#[test]
fn idoc_native_provenance_format_options_roundtrip_and_default_off() {
    let field = IdocNativeField::new(
        "name",
        "Name",
        IdocNativeFieldType::Character,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(1).unwrap(),
        NonZeroU32::new(12).unwrap(),
        NonZeroU32::new(15).unwrap(),
        vec![IdocNativeCode::new("A", "Active").unwrap()],
    )
    .unwrap();
    let descriptor = IdocNativeConfig::new(
        "ORDERS01",
        vec![IdocNativeNode::Segment(
            IdocNativeSegment::new(
                "HEADER0001",
                "E1HEADER",
                true,
                NonZeroU32::new(1).unwrap(),
                IdocNativeStatus::Mandatory,
                1,
                9_999_999_999,
                vec![field],
            )
            .unwrap(),
        )],
    )
    .unwrap();
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Idoc),
        edi_autocomplete: Some(EdiAutocomplete::Idoc),
        idoc: Some(descriptor.project().unwrap().1),
        idoc_native_config: Some(descriptor),
        idoc_native_text_settings: Some(
            IdocNativeTextSettings::new_observed_profile(
                false,
                true,
                false,
                IdocNativeValidationKind::ALL
                    .into_iter()
                    .map(|kind| {
                        IdocNativeValidationCase::new(kind, IdocNativeValidationAction::Stop)
                    })
                    .collect(),
            )
            .unwrap(),
        ),
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, options);

    let defaults: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(defaults.idoc_native_config.is_none());
    assert!(defaults.idoc_native_text_settings.is_none());
    assert!(
        !serde_json::to_string(&defaults)
            .unwrap()
            .contains("idoc_native_config")
    );
}

#[test]
fn native_decimal_input_names_default_off_and_roundtrip_in_source_options() {
    let mut options: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(options.mfd_decimal_input_names.is_empty());
    assert!(
        !serde_json::to_string(&options)
            .unwrap()
            .contains("mfd_decimal_input_names")
    );

    options.mfd_decimal_input_names.insert(7, "Markup".into());
    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.mfd_decimal_input_names.get(&7),
        Some(&"Markup".into())
    );
}

#[test]
fn optional_runtime_parameter_serializes_without_changing_required_parameter_shape() {
    let optional = crate::Node::RuntimeParameterDefault {
        name: "NamePrefix".into(),
        ty: ScalarType::String,
        default: 7,
        preview: None,
    };
    let encoded = serde_json::to_value(&optional).unwrap();
    assert_eq!(encoded["kind"], "runtime_parameter_default");
    assert_eq!(encoded["name"], "NamePrefix");
    assert_eq!(encoded["default"], 7);
    assert_eq!(optional.dependencies(), vec![7]);
    assert!(matches!(
        serde_json::from_value::<crate::Node>(encoded).unwrap(),
        crate::Node::RuntimeParameterDefault { default: 7, .. }
    ));

    let required: crate::Node =
        serde_json::from_str(r#"{"kind":"runtime_parameter","name":"required","ty":"int"}"#)
            .unwrap();
    assert!(matches!(required, crate::Node::RuntimeParameter { .. }));
}

#[test]
fn host_preview_metadata_preserves_raw_lexicals_and_legacy_absence() {
    for kind in ["runtime_parameter", "runtime_parameter_default"] {
        let default = if kind.ends_with("default") {
            r#", "default":7"#
        } else {
            ""
        };
        let legacy = format!(r#"{{"kind":"{kind}","name":"control","ty":"int"{default}}}"#);
        let node: crate::Node = serde_json::from_str(&legacy).unwrap();
        assert!(!serde_json::to_string(&node).unwrap().contains("preview"));
        for lexical in ["", "not an integer", "  17  "] {
            let mut encoded = serde_json::from_str::<serde_json::Value>(&legacy).unwrap();
            encoded["preview"] = lexical.into();
            let node: crate::Node = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(serde_json::to_value(node).unwrap(), encoded);
        }
    }
}

#[test]
fn host_preview_metadata_bounds_utf8_bytes_on_read_and_write() {
    let mut node = crate::Node::RuntimeParameter {
        name: "control".into(),
        ty: ScalarType::String,
        preview: Some("é".repeat(4 * 1024 * 1024)),
    };
    let at_limit = serde_json::to_string(&node).unwrap();
    assert!(serde_json::from_str::<crate::Node>(&at_limit).is_ok());
    let crate::Node::RuntimeParameter {
        preview: Some(preview),
        ..
    } = &mut node
    else {
        panic!("preview");
    };
    preview.push('X');
    assert!(
        serde_json::to_string(&node)
            .unwrap_err()
            .to_string()
            .contains("preview exceeds")
    );
    let mut encoded: serde_json::Value = serde_json::from_str(&at_limit).unwrap();
    encoded["preview"] = format!("{}X", encoded["preview"].as_str().unwrap()).into();
    assert!(
        serde_json::from_value::<crate::Node>(encoded)
            .unwrap_err()
            .to_string()
            .contains("preview exceeds")
    );
}

#[test]
fn xml_document_identity_roundtrips() {
    let options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    assert!(encoded.contains("\"xml_document\":true"));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.xml_document);
}

#[test]
fn local_xml_file_set_identity_roundtrips() {
    let options = FormatOptions {
        xml_document: true,
        local_xml_file_set: true,
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    assert!(encoded.contains("\"local_xml_file_set\":true"));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.local_xml_file_set);
}

#[test]
fn tabular_boundary_kind_roundtrips() {
    let options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Xlsx),
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    assert!(encoded.contains("\"tabular_kind\":\"xlsx\""));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.tabular_kind, Some(TabularBoundaryKind::Xlsx));
}

#[test]
fn edi_boundary_kind_roundtrips() {
    let options = FormatOptions {
        lenient_segments: true,
        edi_kind: Some(EdiBoundaryKind::X12),
        edi_config_reference: Some("EDI/Envelope.Config".into()),
        edi_implied_decimals: vec![
            EdiImpliedDecimal::new(vec!["Interchange".into(), "Amount".into()], 3).unwrap(),
        ],
        edi_value_constraints: vec![
            crate::EdiValueConstraint::new(
                vec!["Interchange".into(), "Code".into()],
                2,
                3,
                vec!["AA".into(), "BB".into(), "AA".into()],
            )
            .unwrap(),
        ],
        x12_separators: Some(X12Separators {
            element: '+',
            component: ':',
            segment: '\'',
            repetition: Some('!'),
            release: Some('?'),
        }),
        x12_interchange_version: Some("00505".into()),
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, options);
}

#[test]
fn edi_configuration_dependencies_preserve_legacy_references_and_typed_missing_state()
-> Result<(), Box<dyn std::error::Error>> {
    let legacy: FormatOptions =
        serde_json::from_str(r#"{"edi_config_reference":"EDI/Envelope.Config"}"#)?;
    assert_eq!(
        legacy
            .edi_config_reference
            .as_ref()
            .and_then(EdiConfigDependency::reference),
        Some("EDI/Envelope.Config")
    );
    assert_eq!(
        serde_json::to_value(&legacy)?["edi_config_reference"],
        "EDI/Envelope.Config"
    );

    let missing = FormatOptions {
        edi_config_reference: Some(EdiConfigDependency::MissingConfiguration),
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_value(&missing)?;
    assert_eq!(
        encoded["edi_config_reference"]["kind"],
        "missing_configuration"
    );
    assert_eq!(serde_json::from_value::<FormatOptions>(encoded)?, missing);
    Ok(())
}

#[test]
fn protobuf_format_option_roundtrips_embedded_schema() {
    let options = FormatOptions {
        protobuf: Some(ProtobufOptions {
            schema: "message Result { required string value = 1; }".into(),
            root_message: "Result".into(),
            schema_path: Some("api/result.proto".into()),
            imports: vec![crate::ProtobufSchemaFile {
                path: "shared/value.proto".into(),
                source: "message Value { optional string text = 1; }".into(),
            }],
        }),
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.protobuf, options.protobuf);
}

#[test]
fn flextext_format_option_roundtrips_validated_layout() {
    let layout = FlexTextLayout::new(
        "document",
        FlexCommand::store("value", ScalarType::String, None),
        FlexLineEnding::Crlf,
        false,
    )
    .unwrap();
    let options = FormatOptions {
        flextext: Some(layout.clone()),
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.flextext, Some(layout));
}

#[test]
fn fixed_width_layout_validates_and_roundtrips() {
    let layout = FixedWidthLayout::new(
        vec![
            FixedFieldWidth::new(6).unwrap(),
            FixedFieldWidth::new(12).unwrap(),
        ],
        '@',
        true,
        true,
    )
    .unwrap();
    let options = FormatOptions {
        fixed_width: Some(layout.clone()),
        ..FormatOptions::default()
    };

    assert_eq!(layout.record_width(), 18);
    assert_eq!(layout.field_widths()[0].get(), 6);
    assert_eq!(layout.fill_char(), '@');
    assert!(layout.record_delimiters());
    assert!(layout.treat_empty_as_absent());

    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.fixed_width, Some(layout));
}

#[test]
fn fixed_width_layout_rejects_invalid_construction_and_json() {
    assert!(FixedFieldWidth::new(0).is_none());
    assert!(matches!(
        FixedWidthLayout::new(Vec::new(), ' ', true, false),
        Err(FixedWidthLayoutError::EmptyFieldWidths)
    ));
    assert!(matches!(
        FixedWidthLayout::new(vec![FixedFieldWidth::new(1).unwrap()], '\n', true, false),
        Err(FixedWidthLayoutError::InvalidFillChar('\n'))
    ));
    assert!(serde_json::from_str::<FixedFieldWidth>("0").is_err());
    assert!(serde_json::from_str::<FixedWidthLayout>(
            r#"{"field_widths":[2],"fill_char":"\r","record_delimiters":true,"treat_empty_as_absent":false}"#
        )
        .is_err());
}

#[test]
fn xlsx_layout_options_default_empty_and_roundtrip() {
    let defaults: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(defaults.xlsx_sheet.is_none());
    assert!(defaults.xlsx_start_row.is_none());
    assert!(defaults.xlsx_columns.is_empty());
    assert!(defaults.xlsx_headers.is_empty());
    assert!(defaults.xlsx_rows.is_empty());
    assert!(defaults.xlsx_composite.is_none());
    assert!(defaults.xlsx_worksheet_set.is_none());
    assert!(defaults.xlsx_grid.is_none());
    assert!(defaults.xlsx_hierarchical.is_none());
    assert!(
        !serde_json::to_string(&defaults)
            .unwrap()
            .contains("xlsx_rows")
    );

    let options = FormatOptions {
        has_header_row: Some(false),
        xlsx_sheet: Some("Revenue".into()),
        xlsx_start_row: Some(5),
        xlsx_columns: vec![2, 4, 7],
        xlsx_headers: vec!["Item".into(), "Amount".into(), "Amount".into()],
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.has_header_row, Some(false));
    assert_eq!(decoded.xlsx_sheet.as_deref(), Some("Revenue"));
    assert_eq!(decoded.xlsx_start_row, Some(5));
    assert_eq!(decoded.xlsx_columns, vec![2, 4, 7]);
    assert_eq!(decoded.xlsx_headers, vec!["Item", "Amount", "Amount"]);

    let quoted_csv = FormatOptions {
        csv_quote: Some('\''),
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&quoted_csv).unwrap();
    assert!(encoded.contains("\"csv_quote\":\"'\""));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.csv_quote, Some('\''));
    assert_eq!(FormatOptions::default().csv_quote, None);
    assert!(!FormatOptions::default().csv_quote_disabled);

    let unquoted_csv = FormatOptions {
        csv_quote_disabled: true,
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&unquoted_csv).unwrap();
    assert!(encoded.contains("\"csv_quote_disabled\":true"));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.csv_quote_disabled);

    let transposed = FormatOptions {
        xlsx_rows: vec![1, 3, 5],
        ..FormatOptions::default()
    };
    let decoded: FormatOptions =
        serde_json::from_str(&serde_json::to_string(&transposed).unwrap()).unwrap();
    assert_eq!(decoded.xlsx_rows, vec![1, 3, 5]);
}

#[test]
fn xlsx_composite_layout_roundtrips() {
    let composite = XlsxCompositeLayout {
        table: Some(XlsxTableRegion {
            path: vec!["Staff".into()],
            sheet: Some("Roster".into()),
            start_row: XlsxRow::new(2).unwrap(),
            columns: vec![XlsxColumn::new(1).unwrap(), XlsxColumn::new(3).unwrap()],
            has_header: true,
            row_number_field: None,
        }),
        additional_tables: Vec::new(),
        records: vec![XlsxFixedRecord {
            path: vec!["Office".into()],
            sheet: Some("Office".into()),
            cells: vec![XlsxFixedCell {
                path: vec!["Name".into()],
                row: XlsxRow::new(1).unwrap(),
                column: XlsxColumn::new(2).unwrap(),
            }],
        }],
        grids: Vec::new(),
    };
    let options = FormatOptions {
        xlsx_composite: Some(composite.clone()),
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&options).unwrap();
    // Table composites keep the serialized form they had before grid regions.
    let layout = serde_json::to_value(&options).unwrap()["xlsx_composite"].clone();
    assert_eq!(
        layout,
        serde_json::json!({
            "table": {
                "path": ["Staff"],
                "sheet": "Roster",
                "start_row": 2,
                "columns": [1, 3],
                "has_header": true
            },
            "records": [{
                "path": ["Office"],
                "sheet": "Office",
                "cells": [{"path": ["Name"], "row": 1, "column": 2}]
            }]
        })
    );
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.xlsx_composite, Some(composite));
}

#[test]
fn xlsx_composite_grid_regions_roundtrip_without_a_table() {
    let grid = |sheet: &str| XlsxGridRegion {
        path: vec![sheet.into()],
        layout: XlsxGridLayout {
            sheet: Some(sheet.into()),
            header_row: XlsxRow::new(1).unwrap(),
            data_start_row: XlsxRow::new(2).unwrap(),
            header_value_field: "Range1".into(),
            header_position_field: "HeaderColumn".into(),
            rows_field: "Rows".into(),
            cells_field: "Cells".into(),
            cell_value_field: "value".into(),
            cell_position_field: "CellColumn".into(),
            fixed_cells: Vec::new(),
        },
    };
    let composite = XlsxCompositeLayout {
        table: None,
        additional_tables: Vec::new(),
        records: Vec::new(),
        grids: vec![grid("Quarterly"), grid("Annual")],
    };
    assert_eq!(composite.validate_regions(), Ok(()));
    let options = FormatOptions {
        xlsx_composite: Some(composite.clone()),
        ..FormatOptions::default()
    };
    let layout = serde_json::to_value(&options).unwrap()["xlsx_composite"].clone();
    assert!(layout.get("table").is_none(), "{layout}");
    let decoded: FormatOptions =
        serde_json::from_str(&serde_json::to_string(&options).unwrap()).unwrap();
    assert_eq!(decoded.xlsx_composite, Some(composite));

    let empty = XlsxCompositeLayout {
        table: None,
        additional_tables: Vec::new(),
        records: Vec::new(),
        grids: Vec::new(),
    };
    assert!(empty.validate_regions().is_err());
    let orphan = XlsxCompositeLayout {
        additional_tables: vec![XlsxTableRegion {
            path: vec!["Staff".into()],
            sheet: None,
            start_row: XlsxRow::new(1).unwrap(),
            columns: Vec::new(),
            has_header: true,
            row_number_field: None,
        }],
        grids: vec![grid("Quarterly")],
        ..empty
    };
    assert!(orphan.validate_regions().is_err());
}

#[test]
fn xlsx_worksheet_set_layout_roundtrips() {
    let layout = XlsxWorksheetSetLayout {
        worksheets_path: vec!["Sheets".into()],
        worksheet_name_path: vec!["Name".into()],
        rows_path: vec!["Rows".into()],
        row_number_path: Some(vec!["r".into()]),
        start_row: XlsxRow::new(2).unwrap(),
        columns: vec![XlsxColumn::new(1).unwrap(), XlsxColumn::new(4).unwrap()],
        has_header: true,
    };
    let options = FormatOptions {
        xlsx_worksheet_set: Some(layout.clone()),
        ..FormatOptions::default()
    };
    let decoded: FormatOptions =
        serde_json::from_str(&serde_json::to_string(&options).unwrap()).unwrap();
    assert_eq!(decoded.xlsx_worksheet_set, Some(layout));
}

#[test]
fn xlsx_grid_layout_roundtrips() {
    let grid = XlsxGridLayout {
        sheet: Some("Sales".into()),
        header_row: XlsxRow::new(1).unwrap(),
        data_start_row: XlsxRow::new(2).unwrap(),
        header_value_field: "Month".into(),
        header_position_field: "MonthColumn".into(),
        rows_field: "Rows".into(),
        cells_field: "Cells".into(),
        cell_value_field: "Value".into(),
        cell_position_field: "Column".into(),
        fixed_cells: vec![XlsxFixedCell {
            path: vec!["Year".into()],
            row: XlsxRow::new(1).unwrap(),
            column: XlsxColumn::new(1).unwrap(),
        }],
    };
    let options = FormatOptions {
        xlsx_grid: Some(grid.clone()),
        ..FormatOptions::default()
    };

    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.xlsx_grid, Some(grid));
}

#[test]
fn xlsx_coordinates_reject_values_outside_excel_limits() {
    assert!(XlsxRow::new(0).is_none());
    assert!(XlsxRow::new(XlsxRow::MAX + 1).is_none());
    assert!(XlsxColumn::new(0).is_none());
    assert!(XlsxColumn::new(XlsxColumn::MAX + 1).is_none());
    assert!(serde_json::from_str::<XlsxRow>("0").is_err());
    assert!(serde_json::from_str::<XlsxColumn>("16385").is_err());
}

fn join_plan() -> JoinPlan {
    let orders = JoinSource::new(vec!["orders".into()]);
    let products = JoinSource::new(vec!["products".into()]);
    let product_key = JoinKey::new(
        vec!["orders".into()],
        vec!["sku".into()],
        vec!["sku".into()],
    );
    JoinPlan::new(orders, products, JoinConditions::new(product_key)).unwrap()
}

#[test]
fn old_scopes_default_dynamic_target_metadata_off() {
    let scope: Scope =
        serde_json::from_str(r#"{"target_field":"","source":null,"bindings":[],"children":[]}"#)
            .unwrap();
    assert!(scope.dynamic_bindings.is_empty());
    assert!(scope.dynamic_children.is_empty());
    assert!(!scope.merge_dynamic_fields);
    assert_eq!(scope.iteration_output, IterationOutput::Repeated);
    assert_eq!(scope.construction, ScopeConstruction::Constructed);
    assert!(scope.group_adjacent_by.is_none());
    assert!(scope.group_starting_with.is_none());
    assert!(scope.group_ending_with.is_none());
    assert!(!scope.iterates());
}

#[test]
fn copy_current_source_construction_roundtrips_explicitly() {
    let scope = Scope {
        construction: ScopeConstruction::CopyCurrentSource,
        ..Scope::default()
    };

    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""construction":"copy_current_source""#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.construction, ScopeConstruction::CopyCurrentSource);
}

#[test]
fn legacy_source_and_sequence_fields_select_typed_iteration() {
    let source: Scope = serde_json::from_str(
        r#"{"target_field":"","source":["items"],"bindings":[],"children":[]}"#,
    )
    .unwrap();
    assert_eq!(
        source.source().expect("legacy fixture selector"),
        Some(["items".to_string()].as_slice())
    );
    assert!(source.sequence().is_none());

    let sequence: Scope = serde_json::from_str(
            r#"{"target_field":"","source":null,"sequence":{"kind":"generate","to":2,"item":3},"bindings":[],"children":[]}"#,
        )
        .unwrap();
    assert!(matches!(
        sequence.sequence(),
        Some(SequenceExpr::Generate {
            from: None,
            to: 2,
            item: 3
        })
    ));

    let encoded = serde_json::to_string(&source).unwrap();
    assert!(encoded.contains(r#""source":["items"]"#));
    assert!(!encoded.contains(r#""iteration""#));
}

#[test]
fn scope_deserialization_rejects_multiple_iteration_forms() {
    let source_and_sequence = serde_json::from_str::<Scope>(
        r#"{"source":["items"],"sequence":{"kind":"generate","to":2,"item":3}}"#,
    );
    assert!(
        source_and_sequence
            .unwrap_err()
            .to_string()
            .contains("mutually exclusive")
    );

    let join = serde_json::to_value(Scope {
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(9),
            plan: join_plan(),
        },
        ..Scope::default()
    })
    .unwrap();
    let mut conflicting = join.as_object().cloned().unwrap();
    conflicting.insert("source".into(), serde_json::json!(["items"]));
    assert!(
        serde_json::from_value::<Scope>(serde_json::Value::Object(conflicting))
            .unwrap_err()
            .to_string()
            .contains("mutually exclusive")
    );
}

#[test]
fn join_plan_enforces_ordered_distinct_sources() {
    let plan = join_plan()
        .then(
            JoinSource::new(vec!["inventory".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["products".into()],
                vec!["id".into()],
                vec!["product_id".into()],
            ))
            .and(JoinKey::new(
                vec!["orders".into()],
                vec!["region".into()],
                vec!["region".into()],
            )),
        )
        .unwrap();
    let sources: Vec<_> = plan
        .sources()
        .map(|source| source.as_collection().unwrap().collection().join("/"))
        .collect();
    assert_eq!(sources, ["orders", "products", "inventory"]);
    assert_eq!(plan.stages().count(), 2);

    let duplicate = join_plan().then(
        JoinSource::new(vec!["orders".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["products".into()],
            vec!["sku".into()],
            vec!["sku".into()],
        )),
    );
    assert!(matches!(
        duplicate,
        Err(JoinPlanError::DuplicateCollection(_))
    ));

    let unknown = JoinPlan::new(
        JoinSource::new(vec!["orders".into()]),
        JoinSource::new(vec!["products".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["missing".into()],
            vec!["sku".into()],
            vec!["sku".into()],
        )),
    );
    assert!(matches!(
        unknown,
        Err(JoinPlanError::UnknownLeftCollection(_))
    ));
}

#[test]
fn join_plan_deserialization_reapplies_constructor_invariants() {
    let join_scope = |second_collection: &str, left_collection: &str| {
        serde_json::json!({
            "join": {
                "id": 1,
                "plan": {
                    "first": { "collection": ["orders"] },
                    "second": {
                        "source": { "collection": [second_collection] },
                        "conditions": {
                            "first": {
                                "left_collection": [left_collection],
                                "left_path": ["sku"],
                                "right_path": ["sku"]
                            }
                        }
                    }
                }
            }
        })
    };

    let duplicate = serde_json::from_value::<Scope>(join_scope("orders", "orders"));
    assert!(
        duplicate
            .unwrap_err()
            .to_string()
            .contains("used more than once")
    );

    let unknown = serde_json::from_value::<Scope>(join_scope("products", "missing"));
    assert!(
        unknown
            .unwrap_err()
            .to_string()
            .contains("before it is joined")
    );
}

#[test]
fn join_scope_and_owned_nodes_roundtrip() {
    let scope = Scope {
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(44),
            plan: join_plan(),
        },
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""join":{"id":44"#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    let Some((id, plan)) = decoded.join() else {
        panic!("expected inner join");
    };
    assert_eq!(id.get(), 44);
    assert_eq!(plan.sources().count(), 2);

    for node in [
        Node::JoinField {
            join: id,
            collection: vec!["products".into()],
            path: vec!["name".into()],
        },
        Node::JoinPosition { join: id },
    ] {
        let encoded = serde_json::to_string(&node).unwrap();
        let decoded: Node = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(
            decoded,
            Node::JoinField { join, .. } | Node::JoinPosition { join }
                if join == JoinId::new(44)
        ));
    }

    let aggregate = Node::JoinAggregate {
        function: AggregateOp::Sum,
        join: id,
        plan: join_plan(),
        expression: Some(7),
        arg: None,
    };
    let encoded = serde_json::to_string(&aggregate).unwrap();
    assert!(encoded.contains(r#""kind":"join_aggregate""#));
    let decoded: Node = serde_json::from_str(&encoded).unwrap();
    assert!(matches!(
        decoded,
        Node::JoinAggregate {
            function: AggregateOp::Sum,
            join,
            expression: Some(7),
            arg: None,
            ..
        } if join == JoinId::new(44)
    ));
}

#[test]
fn scope_iteration_helpers_replace_and_clear_only_their_form() {
    let mut scope = Scope::default();
    scope.set_source(Some(vec!["rows".into()]));
    scope
        .source_mut()
        .expect("legacy fixture selector")
        .unwrap()
        .push("items".into());
    assert_eq!(
        scope.source().expect("legacy fixture selector"),
        Some(["rows".into(), "items".into()].as_slice())
    );

    let sequence = SequenceExpr::Generate {
        from: None,
        to: 7,
        item: 8,
    };
    scope.set_sequence(Some(sequence));
    scope.set_source(None);
    assert!(scope.sequence().is_some());
    scope.set_sequence(None);
    assert!(!scope.iterates());
}

#[test]
fn dynamic_document_iteration_roundtrips_with_its_source() {
    let scope = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["documents".into()],
            output_path: 42,
        },
        ..Scope::default()
    };

    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""source":["documents"]"#));
    assert!(encoded.contains(r#""output_path":42"#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.source().expect("legacy fixture selector"),
        Some(["documents".into()].as_slice())
    );
    assert_eq!(decoded.output_path(), Some(42));
}

#[test]
fn secondary_sort_keys_roundtrip_without_changing_legacy_primary_fields() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        sort_by: Some(4),
        sort_descending: true,
        sort_then_by: vec![SortKey {
            node: 8,
            descending: false,
        }],
        ..Scope::default()
    };

    let encoded = serde_json::to_string(&scope).unwrap();
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.sort_by, Some(4));
    assert!(decoded.sort_descending);
    assert_eq!(
        decoded.sort_then_by,
        [SortKey {
            node: 8,
            descending: false
        }]
    );
}

#[test]
fn ordered_sequence_windows_roundtrip() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        windows: vec![
            SequenceWindow::SkipFirst { count: 4 },
            SequenceWindow::FromTo { first: 5, last: 6 },
            SequenceWindow::Last { count: 7 },
        ],
        ..Scope::default()
    };

    let encoded = serde_json::to_string(&scope).unwrap();
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.windows, scope.windows);
}

#[test]
fn dynamic_document_output_path_requires_source_iteration() {
    let without_source = serde_json::from_str::<Scope>(r#"{"output_path":42}"#);
    assert!(
        without_source
            .unwrap_err()
            .to_string()
            .contains("requires source iteration")
    );

    let mut scope = Scope::default();
    assert!(!scope.set_output_path(Some(42)));
    scope.set_source(Some(Vec::new()));
    assert!(scope.set_output_path(Some(42)));
    assert_eq!(scope.output_path(), Some(42));
    assert!(scope.set_output_path(None));
    assert_eq!(
        scope.source().expect("legacy fixture selector"),
        Some([].as_slice())
    );
}

#[test]
fn group_starting_predicate_roundtrips() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        group_starting_with: Some(7),
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""group_starting_with":7"#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.group_starting_with, Some(7));
}

#[test]
fn post_group_filter_roundtrips_and_defaults_to_absent() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        post_group_filter: Some(10),
        group_by: Some(11),
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""post_group_filter":10"#));
    assert_eq!(
        serde_json::from_str::<Scope>(&encoded)
            .unwrap()
            .post_group_filter,
        Some(10)
    );
    assert_eq!(
        serde_json::from_str::<Scope>(r#"{"group_by":11}"#)
            .unwrap()
            .post_group_filter,
        None
    );
}

#[test]
fn adjacent_and_ending_group_controls_roundtrip() {
    let adjacent = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        group_adjacent_by: Some(8),
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&adjacent).unwrap();
    assert!(encoded.contains(r#""group_adjacent_by":8"#));
    assert_eq!(
        serde_json::from_str::<Scope>(&encoded)
            .unwrap()
            .group_adjacent_by,
        Some(8)
    );

    let ending = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        group_ending_with: Some(9),
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&ending).unwrap();
    assert!(encoded.contains(r#""group_ending_with":9"#));
    assert_eq!(
        serde_json::from_str::<Scope>(&encoded)
            .unwrap()
            .group_ending_with,
        Some(9)
    );
}

#[test]
fn dynamic_target_metadata_roundtrips() {
    let scope = Scope {
        dynamic_bindings: vec![DynamicBinding { key: 1, value: 2 }],
        dynamic_children: vec![DynamicChild {
            key: 3,
            scope: Scope {
                iteration: ScopeIteration::Source(vec!["items".into()]),
                ..Scope::default()
            },
        }],
        merge_dynamic_fields: true,
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.dynamic_bindings.len(), 1);
    assert_eq!(decoded.dynamic_children.len(), 1);
    assert!(decoded.merge_dynamic_fields);
}

#[test]
fn first_item_iteration_output_roundtrips() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        iteration_output: IterationOutput::First,
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""iteration_output":"first""#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.iteration_output, IterationOutput::First);
}

#[test]
fn mapped_sequence_iteration_output_roundtrips() {
    let scope = Scope {
        iteration: ScopeIteration::Source(vec!["items".into()]),
        iteration_output: IterationOutput::MappedSequence,
        ..Scope::default()
    };
    let encoded = serde_json::to_string(&scope).unwrap();
    assert!(encoded.contains(r#""iteration_output":"mapped_sequence""#));
    let decoded: Scope = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.iteration_output, IterationOutput::MappedSequence);
}

#[test]
fn sequence_exists_roundtrips() {
    let node = Node::SequenceExists {
        sequence: SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3,
        },
        predicate: 4,
    };
    let encoded = serde_json::to_string(&node).unwrap();
    let decoded: Node = serde_json::from_str(&encoded).unwrap();
    let Node::SequenceExists {
        sequence,
        predicate,
    } = decoded
    else {
        panic!("expected sequence-exists node");
    };
    assert!(matches!(
        sequence,
        SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3
        }
    ));
    assert_eq!(predicate, 4);
}

#[test]
fn sequence_item_at_roundtrips() {
    let node = Node::SequenceItemAt {
        sequence: SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        },
        index: 4,
    };
    let encoded = serde_json::to_string(&node).unwrap();
    assert!(encoded.contains(r#""kind":"sequence_item_at""#));
    let decoded: Node = serde_json::from_str(&encoded).unwrap();
    assert!(matches!(
        decoded,
        Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: Some(1),
                to: 2,
                item: 3
            },
            index: 4
        }
    ));
}

#[test]
fn sequence_aggregate_roundtrips() {
    let node = Node::SequenceAggregate {
        function: AggregateOp::Join,
        sequence: SequenceExpr::TokenizeRegex {
            input: 1,
            pattern: 2,
            flags: Some(3),
            item: 4,
        },
        predicate: Some(5),
        expression: Some(6),
        arg: Some(7),
    };
    let encoded = serde_json::to_string(&node).unwrap();
    assert!(encoded.contains(r#""kind":"sequence_aggregate""#));
    let decoded: Node = serde_json::from_str(&encoded).unwrap();
    assert!(matches!(
        decoded,
        Node::SequenceAggregate {
            function: AggregateOp::Join,
            sequence: SequenceExpr::TokenizeRegex {
                input: 1,
                pattern: 2,
                flags: Some(3),
                item: 4
            },
            predicate: Some(5),
            expression: Some(6),
            arg: Some(7)
        }
    ));
}

#[test]
fn regex_sequence_preserves_disconnected_optional_flags() {
    let sequence = SequenceExpr::TokenizeRegex {
        input: 1,
        pattern: 2,
        flags: None,
        item: 3,
    };
    let encoded = serde_json::to_string(&sequence).unwrap();
    assert!(!encoded.contains("flags"));
    let decoded: SequenceExpr = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, sequence);
    assert_eq!(decoded.inputs(), vec![1, 2]);
}

#[test]
fn xml_serializer_indent_defaults_on_and_roundtrips_when_disabled() {
    let node = Node::XmlSerialize {
        path: vec!["Item".into()],
        frame: None,
        schema: Box::new(ir::SchemaNode::group("Item", Vec::new())),
        declaration: false,
        indent: false,
        namespace: None,
    };
    let encoded = serde_json::to_value(&node).unwrap();
    assert_eq!(
        encoded
            .pointer("/schema/name")
            .and_then(|name| name.as_str()),
        Some("Item")
    );
    let decoded: Node = serde_json::from_value(encoded.clone()).unwrap();
    assert!(matches!(decoded, Node::XmlSerialize { indent: false, .. }));

    let mut legacy = encoded.as_object().unwrap().clone();
    legacy.remove("indent");
    let decoded: Node = serde_json::from_value(serde_json::Value::Object(legacy)).unwrap();
    assert!(matches!(decoded, Node::XmlSerialize { indent: true, .. }));
}

#[test]
fn csv_utf8_bom_defaults_off_and_survives_format_options_serde() {
    let legacy: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(!legacy.csv_utf8_bom);
    let marked = FormatOptions {
        csv_utf8_bom: true,
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&marked).unwrap();
    assert!(encoded.contains("\"csv_utf8_bom\":true"));
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.csv_utf8_bom);
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("csv_utf8_bom")
    );
}

#[test]
fn source_owner_tagged_forms_preserve_exact_names() {
    assert_eq!(
        serde_json::to_string(&SourceOwner::Primary).unwrap(),
        r#"{"kind":"primary"}"#
    );
    assert_eq!(
        serde_json::from_str::<SourceOwner>(r#"{"kind":"primary"}"#).unwrap(),
        SourceOwner::Primary
    );
    for (name, encoded) in [
        ("Catalog", r#"{"kind":"named","name":"Catalog"}"#),
        ("catalog", r#"{"kind":"named","name":"catalog"}"#),
        (" catalog ", r#"{"kind":"named","name":" catalog "}"#),
        ("参照 é", r#"{"kind":"named","name":"参照 é"}"#),
        (" ", r#"{"kind":"named","name":" "}"#),
    ] {
        let expected = SourceOwner::Named { name: name.into() };
        let actual: SourceOwner = serde_json::from_str(encoded).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }
    assert_ne!(
        SourceOwner::Named {
            name: "Catalog".into()
        },
        SourceOwner::Named {
            name: "catalog".into()
        }
    );
    assert_ne!(
        SourceOwner::Named {
            name: "catalog".into()
        },
        SourceOwner::Named {
            name: " catalog ".into()
        }
    );
}

#[test]
fn source_owner_rejects_unknown_duplicate_missing_and_malformed_fields() {
    for encoded in [
        r#"{}"#,
        r#"{"name":"catalog"}"#,
        r#"{"kind":"named"}"#,
        r#"{"kind":"other"}"#,
        r#"{"kind":"Primary"}"#,
        r#"{"kind":"primary","name":"catalog"}"#,
        r#"{"kind":"primary","extra":0}"#,
        r#"{"kind":"named","name":"catalog","extra":0}"#,
        r#"{"kind":"primary","kind":"primary"}"#,
        r#"{"kind":"named","name":"catalog","name":"catalog"}"#,
        r#"{"kind":"named","name":""}"#,
        r#"{"kind":null}"#,
        r#"{"kind":1}"#,
        r#"{"kind":"named","name":null}"#,
        r#"{"kind":"named","name":1}"#,
        r#"{"kind":"named","name":false}"#,
        r#"{"kind":"named","name":[]}"#,
        r#"{"kind":"named","name":{}}"#,
        r#"null"#,
        r#""primary""#,
        r#"[]"#,
        r#"["primary"]"#,
        r#"1"#,
        r#"false"#,
    ] {
        assert!(
            serde_json::from_str::<SourceOwner>(encoded).is_err(),
            "{encoded}"
        );
    }
}

#[test]
fn source_owner_refuses_empty_typed_name_when_serializing() {
    let invalid = SourceOwner::Named {
        name: String::new(),
    };
    assert!(
        serde_json::to_string(&invalid)
            .unwrap_err()
            .to_string()
            .contains("name cannot be empty")
    );
    let invalid_ref = OwnedSourceRef {
        owner: invalid,
        path: Vec::new(),
    };
    assert!(serde_json::to_string(&invalid_ref).is_err());
    assert!(serde_json::to_string(&SourceSelector::Owned(invalid_ref)).is_err());
}

#[test]
fn owned_source_refs_preserve_explicit_root_nested_and_unrestricted_segments() {
    for (encoded, expected) in [
        (
            r#"{"owner":{"kind":"primary"},"path":[]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
        ),
        (
            r#"{"owner":{"kind":"primary"},"path":["Rows","Value"]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Rows".into(), "Value".into()],
            },
        ),
        (
            r#"{"owner":{"kind":"named","name":" 参照 "},"path":[]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: " 参照 ".into(),
                },
                path: Vec::new(),
            },
        ),
        (
            r#"{"owner":{"kind":"named","name":"catalog"},"path":["","..","a/b","/","\\"," 名 ","\u0000"]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "catalog".into(),
                },
                path: vec![
                    "".into(),
                    "..".into(),
                    "a/b".into(),
                    "/".into(),
                    "\\".into(),
                    " 名 ".into(),
                    "\0".into(),
                ],
            },
        ),
    ] {
        let actual: OwnedSourceRef = serde_json::from_str(encoded).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }
    let long = "名".repeat(1024);
    let expected = OwnedSourceRef {
        owner: SourceOwner::Named { name: long.clone() },
        path: vec![long],
    };
    let encoded = serde_json::to_string(&expected).unwrap();
    assert_eq!(
        serde_json::from_str::<OwnedSourceRef>(&encoded).unwrap(),
        expected
    );
}

#[test]
fn owned_source_refs_reject_unknown_duplicate_missing_and_malformed_fields() {
    for encoded in [
        r#"{}"#,
        r#"{"owner":{"kind":"primary"}}"#,
        r#"{"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":[],"extra":0}"#,
        r#"{"owner":{"kind":"primary"},"owner":{"kind":"primary"},"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":[],"path":[]}"#,
        r#"{"owner":{"kind":"primary","kind":"primary"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":"a","name":"a"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":""},"path":[]}"#,
        r#"{"owner":null,"path":[]}"#,
        r#"{"owner":"primary","path":[]}"#,
        r#"{"owner":[],"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":null}"#,
        r#"{"owner":{"kind":"primary"},"path":"Rows"}"#,
        r#"{"owner":{"kind":"primary"},"path":{}}"#,
        r#"{"owner":{"kind":"primary"},"path":[1]}"#,
        r#"{"owner":{"kind":"primary"},"path":[null]}"#,
        r#"{"owner":{"kind":"primary"},"path":[[]]}"#,
        r#"[{"kind":"primary"},[]]"#,
        r#"[]"#,
        r#"null"#,
    ] {
        assert!(
            serde_json::from_str::<OwnedSourceRef>(encoded).is_err(),
            "{encoded}"
        );
    }
}

#[test]
fn source_selector_preserves_legacy_arrays_without_migration() {
    for (encoded, path) in [
        (r#"[]"#, Vec::new()),
        (r#"["Rows","Value"]"#, vec!["Rows".into(), "Value".into()]),
        (
            r#"["","..","a/b"," 名 "]"#,
            vec!["".into(), "..".into(), "a/b".into(), " 名 ".into()],
        ),
    ] {
        let actual: SourceSelector = serde_json::from_str(encoded).unwrap();
        assert_eq!(actual, SourceSelector::Legacy(path));
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }
}

#[test]
fn source_selector_preserves_owned_objects_without_prefix_rewriting() {
    for (encoded, expected) in [
        (
            r#"{"owner":{"kind":"primary"},"path":[]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
        ),
        (
            r#"{"owner":{"kind":"named","name":"reference"},"path":["Rows","Value"]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "reference".into(),
                },
                path: vec!["Rows".into(), "Value".into()],
            },
        ),
        (
            r#"{"owner":{"kind":"named","name":"reference"},"path":["reference","Value"]}"#,
            OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "reference".into(),
                },
                path: vec!["reference".into(), "Value".into()],
            },
        ),
    ] {
        let actual: SourceSelector = serde_json::from_str(encoded).unwrap();
        assert_eq!(actual, SourceSelector::Owned(expected));
        assert_eq!(serde_json::to_string(&actual).unwrap(), encoded);
    }
}

#[test]
fn source_selector_rejects_every_nonarray_nonobject_or_malformed_form() {
    for encoded in [
        r#"null"#,
        r#""Rows""#,
        r#"1"#,
        r#"false"#,
        r#"[1]"#,
        r#"[null]"#,
        r#"[[]]"#,
        r#"[{}]"#,
        r#"[{"kind":"primary"},[]]"#,
        r#"{}"#,
        r#"{"owner":{"kind":"primary"}}"#,
        r#"{"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":[],"extra":0}"#,
        r#"{"owner":{"kind":"primary"},"owner":{"kind":"primary"},"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":[],"path":[]}"#,
        r#"{"owner":{"kind":"primary","kind":"primary"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":"a","name":"a"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":""},"path":[]}"#,
        r#"{"owner":null,"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":null}"#,
    ] {
        assert!(
            serde_json::from_str::<SourceSelector>(encoded).is_err(),
            "{encoded}"
        );
    }
}

#[test]
fn source_selector_null_and_absence_belong_to_outer_option() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Outer {
        #[serde(default)]
        source: Option<SourceSelector>,
    }
    for encoded in [r#"{}"#, r#"{"source":null}"#] {
        let actual: Outer = serde_json::from_str(encoded).unwrap();
        assert_eq!(actual.source, None);
        assert_eq!(
            serde_json::to_string(&actual).unwrap(),
            r#"{"source":null}"#
        );
    }
    assert_eq!(
        serde_json::from_str::<Outer>(r#"{"source":[]}"#)
            .unwrap()
            .source,
        Some(SourceSelector::Legacy(Vec::new()))
    );
    assert_eq!(
        serde_json::from_str::<Outer>(r#"{"source":{"owner":{"kind":"primary"},"path":[]}}"#)
            .unwrap()
            .source,
        Some(SourceSelector::Owned(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: Vec::new()
        }))
    );
    assert_eq!(
        serde_json::from_str::<Option<OwnedSourceRef>>("null").unwrap(),
        None
    );
    assert_eq!(
        serde_json::from_str::<Option<SourceOwner>>("null").unwrap(),
        None
    );
}

#[test]
fn old_path_array_readers_refuse_owned_objects() {
    for encoded in [
        r#"{"owner":{"kind":"primary"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":"catalog"},"path":["Rows","Value"]}"#,
    ] {
        assert!(serde_json::from_str::<Vec<String>>(encoded).is_err());
        assert!(serde_json::from_str::<Option<Vec<String>>>(encoded).is_err());
    }
    assert_eq!(
        serde_json::from_str::<Vec<String>>(r#"["Rows","Value"]"#).unwrap(),
        ["Rows", "Value"]
    );
    assert_eq!(
        serde_json::from_str::<Option<Vec<String>>>("null").unwrap(),
        None
    );
}
