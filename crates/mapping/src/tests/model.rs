use super::*;
use ir::ScalarType;

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
        graph: Graph::default(),
        root: Scope::default(),
    };

    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();

    assert_eq!(encoded.matches("\"message\"").count(), 1);
    assert_eq!(decoded.failure_rules, project.failure_rules);
}

#[test]
fn json_lines_format_option_defaults_off_and_roundtrips_when_enabled() {
    let defaults: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(!defaults.json_lines);
    assert!(defaults.edi_kind.is_none());
    assert!(defaults.edi_implied_decimals.is_empty());
    assert!(defaults.edi_lexical_formats.is_empty());
    assert!(defaults.x12_separators.is_none());
    assert!(defaults.x12_interchange_version.is_none());
    assert!(!defaults.xml_document);
    assert!(!defaults.local_xml_file_set);
    assert!(!defaults.json_document);
    assert!(defaults.tabular_kind.is_none());
    assert!(defaults.fixed_width.is_none());
    assert!(defaults.flextext.is_none());
    assert!(defaults.idoc.is_none());
    assert!(defaults.swift_mt.is_none());
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
        edi_implied_decimals: vec![
            EdiImpliedDecimal::new(vec!["Interchange".into(), "Amount".into()], 3).unwrap(),
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
fn protobuf_format_option_roundtrips_embedded_schema() {
    let options = FormatOptions {
        protobuf: Some(ProtobufOptions {
            schema: "message Result { required string value = 1; }".into(),
            root_message: "Result".into(),
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
        table: XlsxTableRegion {
            path: vec!["Staff".into()],
            sheet: Some("Roster".into()),
            start_row: XlsxRow::new(2).unwrap(),
            columns: vec![XlsxColumn::new(1).unwrap(), XlsxColumn::new(3).unwrap()],
            has_header: true,
            row_number_field: None,
        },
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
    };
    let options = FormatOptions {
        xlsx_composite: Some(composite.clone()),
        ..FormatOptions::default()
    };
    let encoded = serde_json::to_string(&options).unwrap();
    let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.xlsx_composite, Some(composite));
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
    assert_eq!(source.source(), Some(["items".to_string()].as_slice()));
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
        .map(|source| source.collection().join("/"))
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
    scope.source_mut().unwrap().push("items".into());
    assert_eq!(
        scope.source(),
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
    assert_eq!(decoded.source(), Some(["documents".into()].as_slice()));
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
    assert_eq!(scope.source(), Some([].as_slice()));
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
