use super::validate;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    Binding, DynamicBinding, DynamicSourcePath, Graph, NamedSource, Node, PdfCapture, PdfCommand,
    PdfLayout, PdfPageSelection, PdfRegion, Project, Scope, ScopeConstruction, SequenceExpr,
    SequenceWindow, WsdlMessageOptions, XbrlBoundaryOptions, XlsxRow, XlsxWorksheetSetLayout,
};
use std::num::NonZeroU32;

fn valid_project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["name".into()],
        },
    );
    Project {
        source: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            bindings: vec![Binding {
                target_field: "name".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn target_name(project: &mut Project) -> &mut SchemaNode {
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    let Some(target) = children.iter_mut().find(|child| child.name == "name") else {
        panic!("test target field must exist");
    };
    target
}

#[test]
fn accepts_a_valid_project_and_relative_source_paths() {
    let mut project = valid_project();
    project.extra_sources.push(NamedSource {
        name: "reference".into(),
        path: "reference.json".into(),
        schema: SchemaNode::group(
            "records",
            vec![SchemaNode::scalar("code", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        1,
        Node::SourceField {
            frame: None,
            path: vec!["reference".into(), "code".into()],
        },
    );

    assert!(validate(&project).is_empty());
}

#[test]
fn validates_flat_xlsx_header_and_update_options_before_execution() {
    let mut project = valid_project();
    project.source_options.xlsx_update_existing = true;
    project.target_options.xlsx_headers = vec!["first".into(), "extra".into()];
    project.target_options.xlsx_rows = vec![1];

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("valid only for mapping targets")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("only with a flat XLSX table")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "target format options"
            && issue
                .message
                .contains("2 value(s) for 1 flat schema field(s)")
    }));
}

#[test]
fn validates_worksheet_set_direction_and_layout_exclusivity() {
    let mut project = valid_project();
    project.target_options.xlsx_sheet = Some("Legacy".into());
    project.target_options.xlsx_worksheet_set = Some(XlsxWorksheetSetLayout {
        worksheets_path: vec!["Sheets".into()],
        worksheet_name_path: vec!["Name".into()],
        rows_path: vec!["Rows".into()],
        row_number_path: None,
        start_row: XlsxRow::new(1).unwrap(),
        columns: Vec::new(),
        has_header: false,
    });

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("valid only for mapping sources")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("cannot be combined with flat")
    }));
}

#[test]
fn validates_wsdl_roles_xml_identity_and_format_exclusivity() {
    let mut project = valid_project();
    project.source_options.wsdl = Some(
        WsdlMessageOptions::response("contract.wsdl", "Service", "Port", "Operation").unwrap(),
    );
    project.source_options.json_document = true;
    project.target_options.wsdl =
        Some(WsdlMessageOptions::request("contract.wsdl", "Service", "Port", "Operation").unwrap());

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "source format options" && issue.message.contains("must be a request")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("requires XML document identity")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("another format identity")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "target format options" && issue.message.contains("response or fault")
    }));
}

#[test]
fn accepts_scalar_paths_through_recursive_schema_anchors() {
    let mut project = valid_project();
    let section = SchemaNode::group(
        "MainSection",
        vec![
            SchemaNode::scalar("Trademark", ScalarType::String).repeating(),
            SchemaNode::recursive_group("SubSection", "MainSection").repeating(),
        ],
    );
    project.source = SchemaNode::group(
        "Page",
        vec![SchemaNode::group("Item", vec![section]).repeating()],
    );
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            frame: Some(vec!["Item".into()]),
            path: vec![
                "MainSection".into(),
                "SubSection".into(),
                "Trademark".into(),
            ],
        },
    );
    project.root.set_source(Some(vec!["Item".into()]));

    assert!(validate(&project).is_empty());
}

#[test]
fn validates_dynamic_extra_source_ownership() {
    let mut project = valid_project();
    project.extra_sources.push(NamedSource {
        name: "reference".into(),
        path: String::new(),
        schema: SchemaNode::group("records", Vec::new()),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 99,
            iteration: vec!["missing".into()],
        }),
    });

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "extra source `reference`" && issue.message.contains("missing node 99")
    }));
    assert!(issues.iter().any(|issue| {
        issue.location == "extra source `reference`"
            && issue.message.contains("matches no source path")
    }));
}

#[test]
fn rejects_http_transport_metadata_on_a_target() {
    let mut project = valid_project();
    project.target_options.http_get = Some(mapping::HttpGetOptions::default());

    assert!(validate(&project).iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("only for mapping sources")
    }));
}

#[test]
fn validates_idoc_output_and_structured_edi_format_exclusivity() {
    let field = mapping::IdocFieldLayout::new(
        "value",
        NonZeroU32::new(12).unwrap(),
        NonZeroU32::new(20).unwrap(),
    )
    .unwrap();
    let segment = mapping::IdocSegmentLayout::new("HEADER0001", vec![field]).unwrap();
    let layout = mapping::IdocLayout::new(vec![segment]).unwrap();

    let mut source = valid_project();
    source.source_options.idoc = Some(layout.clone());
    source.source_options.delimiter = Some('|');
    assert!(validate(&source).iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("`idoc` cannot be combined")
    }));

    let mut swift_source = valid_project();
    swift_source.source_options.swift_mt = Some(
        mapping::SwiftMtLayout::new(vec![mapping::SwiftMessageLayout::new("MT950", Vec::new())])
            .unwrap(),
    );
    swift_source.source_options.fixed_width = Some(
        mapping::FixedWidthLayout::new(
            vec![mapping::FixedFieldWidth::new(1).unwrap()],
            ' ',
            true,
            true,
        )
        .unwrap(),
    );
    assert!(validate(&swift_source).iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("`swift_mt` cannot be combined")
    }));

    let mut target = valid_project();
    target.target_options.idoc = Some(layout.clone());
    assert!(validate(&target).is_empty());

    target.target_options.delimiter = Some('|');
    assert!(validate(&target).iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("`idoc` cannot be combined")
    }));

    let mut swift_target = valid_project();
    swift_target.target_options.swift_mt = Some(
        mapping::SwiftMtLayout::new(vec![mapping::SwiftMessageLayout::new("MT950", Vec::new())])
            .unwrap(),
    );
    assert!(validate(&swift_target).iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("only for mapping sources")
    }));
}

#[test]
fn validates_xbrl_boundary_side_and_format_exclusivity() -> Result<(), Box<dyn std::error::Error>> {
    let mut valid_source = valid_project();
    valid_source.source_options.xbrl = Some(XbrlBoundaryOptions::external_source("source.xsd")?);
    assert!(validate(&valid_source).is_empty());

    let mut valid_target = valid_project();
    valid_target.target_options.xbrl = Some(XbrlBoundaryOptions::external_target(
        "target.xsd",
        Some("table.sps"),
    )?);
    assert!(validate(&valid_target).is_empty());

    let mut wrong_sides = valid_project();
    wrong_sides.source_options.xbrl =
        Some(XbrlBoundaryOptions::external_target("target.xsd", None)?);
    wrong_sides.target_options.xbrl = Some(XbrlBoundaryOptions::external_source("source.xsd")?);
    let side_issues = validate(&wrong_sides);
    assert_eq!(
        side_issues
            .iter()
            .filter(|issue| issue.message.contains("boundary mode"))
            .count(),
        2
    );

    let mut conflict = valid_project();
    conflict.source_options.xbrl = Some(XbrlBoundaryOptions::external_source("source.xsd")?);
    conflict.source_options.delimiter = Some('|');
    assert!(validate(&conflict).iter().any(|issue| {
        issue.location == "source format options" && issue.message.contains("cannot be combined")
    }));

    let mut extra = valid_project();
    extra.extra_sources.push(NamedSource {
        name: "taxonomy".to_owned(),
        path: "instance.xbrl".to_owned(),
        schema: SchemaNode::group("instance", Vec::new()),
        options: mapping::FormatOptions {
            xbrl: Some(XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?),
            ..mapping::FormatOptions::default()
        },
        dynamic_path: None,
    });
    assert!(validate(&extra).iter().any(|issue| {
        issue.location == "extra source `taxonomy` format options"
            && issue.message.contains("boundary mode")
    }));
    Ok(())
}

#[test]
fn validates_pdf_direction_and_source_schema() {
    let layout = PdfLayout::new(
        "row",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "name".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap();
    let mut source = valid_project();
    source.source_options.pdf = Some(layout.clone());
    assert!(validate(&source).is_empty());

    source.source = SchemaNode::group("row", vec![SchemaNode::scalar("other", ScalarType::String)]);
    assert!(validate(&source).iter().any(|issue| {
        issue.location == "source format options"
            && issue.message.contains("does not match the source schema")
    }));

    let mut target = valid_project();
    target.target_options.pdf = Some(layout);
    assert!(validate(&target).iter().any(|issue| {
        issue.location == "target format options"
            && issue.message.contains("only for mapping sources")
    }));
}

#[test]
fn validates_copy_current_source_construction_invariants() {
    let mut valid = valid_project();
    valid.root.set_source(None);
    valid.root.bindings.clear();
    valid.root.construction = ScopeConstruction::CopyCurrentSource;
    assert!(validate(&valid).is_empty());

    let mut content = valid.clone();
    content.root.bindings.push(Binding {
        target_field: "name".into(),
        node: 0,
    });
    content
        .root
        .dynamic_bindings
        .push(DynamicBinding { key: 0, value: 0 });
    content.root.children.push(Scope {
        target_field: "child".into(),
        ..Scope::default()
    });
    content.root.group_by = Some(0);
    let content_issues = validate(&content);
    assert!(content_issues.iter().any(|issue| {
        issue
            .message
            .contains("cannot contain bindings, child scopes, or dynamic target content")
    }));
    assert!(
        content_issues
            .iter()
            .any(|issue| { issue.message.contains("cannot use grouping controls") })
    );

    let mut scalar_source = valid.clone();
    scalar_source.root.set_source(Some(vec!["name".into()]));
    assert!(
        validate(&scalar_source)
            .iter()
            .any(|issue| { issue.message.contains("requires a group source item") })
    );

    let mut scalar_target = valid.clone();
    scalar_target.target = SchemaNode::scalar("result", ScalarType::String);
    assert!(
        validate(&scalar_target)
            .iter()
            .any(|issue| { issue.message.contains("requires a group target schema") })
    );

    let mut mismatched_target = valid.clone();
    mismatched_target.target =
        SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::Int)]);
    assert!(validate(&mismatched_target).iter().any(|issue| {
        issue
            .message
            .contains("requires matching source and target group fields")
    }));

    let mut generated = valid;
    generated.graph.nodes.insert(
        1,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    generated.root.set_sequence(Some(SequenceExpr::Generate {
        from: None,
        to: 0,
        item: 1,
    }));
    assert!(validate(&generated).iter().any(|issue| {
        issue
            .message
            .contains("cannot iterate a generated sequence")
    }));
}

#[test]
fn rejects_inconsistent_deserialized_group_alternatives() {
    let mut project = valid_project();
    let SchemaKind::Group { alternatives, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    *alternatives = vec![ir::GroupAlternative {
        name: "broken".into(),
        members: vec!["missing".into()],
        required: vec!["missing".into()],
        constraints: Vec::new(),
    }];

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema" && issue.message.contains("group alternative metadata")
    }));
}

#[test]
fn rejects_inconsistent_required_field_metadata() {
    let mut project = valid_project();
    let SchemaKind::Group { required, .. } = &mut project.target.kind else {
        panic!("test target must be a group");
    };
    required.push("missing".into());

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema" && issue.message.contains("required-field metadata")
    }));
}

#[test]
fn rejects_programmatically_invalid_fixed_union_metadata() {
    let mut project = valid_project();
    let Some(types) = ir::ScalarTypeSet::new([ir::ScalarType::String, ir::ScalarType::Int]) else {
        panic!("test scalar union members must be distinct");
    };
    let target = target_name(&mut project);
    target.kind = SchemaKind::ScalarUnion { types };
    target.fixed = Some("ambiguous".into());

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("fixed-value metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_programmatically_invalid_numeric_range_metadata() {
    let mut project = valid_project();
    let Some(range) = ir::IntegerRange::new(Some(1), Some(9)) else {
        panic!("test integer range is valid");
    };
    target_name(&mut project).numeric_range = Some(ir::NumericRange::Integer(range));

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("numeric-range metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_programmatically_invalid_item_count_metadata() {
    let mut project = valid_project();
    let Some(range) = ir::ItemCountRange::new(1, Some(3)) else {
        panic!("test item-count range is valid");
    };
    target_name(&mut project).item_count_range = Some(range);

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("item-count metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_programmatically_invalid_property_count_metadata() {
    let mut project = valid_project();
    let Some(range) = ir::PropertyCountRange::new(1, Some(3)) else {
        panic!("test property-count range is valid");
    };
    target_name(&mut project).property_count_range = Some(range);

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("property-count metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_programmatically_invalid_string_length_metadata() {
    let mut project = valid_project();
    let Some(range) = ir::StringLengthRange::new(1, Some(3)) else {
        panic!("test string-length range is valid");
    };
    let target = target_name(&mut project);
    target.kind = SchemaKind::Scalar {
        ty: ScalarType::Int,
    };
    target.string_length_range = Some(range);

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("string-length metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_invalid_and_schema_wide_json_pattern_metadata() {
    let Ok(patterns) = ir::JsonPatternConstraints::new([["^A$"]]) else {
        panic!("test pattern is valid");
    };
    let mut wrong_domain = valid_project();
    let target = target_name(&mut wrong_domain);
    target.kind = SchemaKind::Scalar {
        ty: ScalarType::Int,
    };
    target.json_patterns = Some(patterns);
    assert!(validate(&wrong_domain).iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("JSON pattern metadata")
            && issue.message.contains("name")
    }));

    let mut over_budget = valid_project();
    over_budget.source = SchemaNode::group(
        "Source",
        (0..=ir::MAX_DISTINCT_JSON_PATTERNS)
            .map(|index| {
                let Ok(patterns) = ir::JsonPatternConstraints::new([[format!("^value-{index}$")]])
                else {
                    panic!("test pattern is valid");
                };
                let Some(node) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
                    .with_json_patterns(patterns)
                else {
                    panic!("test pattern matches a string field");
                };
                node
            })
            .collect(),
    );
    assert!(validate(&over_budget).iter().any(|issue| {
        issue.location == "source schema"
            && issue.message.contains("schema-wide")
            && issue.message.contains("pattern")
    }));
}

#[test]
fn rejects_programmatically_invalid_json_format_metadata() {
    let mut project = valid_project();
    let Ok(formats) = ir::JsonFormatAnnotations::new(["email".to_string()]) else {
        panic!("test format annotation is bounded");
    };
    let target = target_name(&mut project);
    target.kind = SchemaKind::Scalar {
        ty: ScalarType::Int,
    };
    target.json_formats = formats;

    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue.location == "target schema"
            && issue.message.contains("JSON format metadata")
            && issue.message.contains("name")
    }));
}

#[test]
fn rejects_every_programmatically_invalid_schema_metadata_family() {
    let mut cases = Vec::new();

    let mut recursive = valid_project();
    target_name(&mut recursive).recursive_ref = Some("row".into());
    cases.push(("recursive-reference metadata", recursive));

    let mut generated = valid_project();
    let target = target_name(&mut generated);
    target.repeating = true;
    target.value_generation = Some(ir::ValueGeneration::MaxNumber);
    cases.push(("generated-value metadata", generated));

    let mut defaulted = valid_project();
    let target = target_name(&mut defaulted);
    target.repeating = true;
    target.default = Some("value".into());
    cases.push(("default-value metadata", defaulted));

    let mut mode = valid_project();
    target_name(&mut mode).alternative_mode = ir::GroupAlternativeMode::Inclusive;
    cases.push(("alternative-mode metadata", mode));

    let mut xml_kind = valid_project();
    target_name(&mut xml_kind).xml_alternative_kind = ir::XmlAlternativeKind::SubstitutionGroup;
    cases.push(("XML alternative-kind metadata", xml_kind));

    let mut relation = valid_project();
    target_name(&mut relation).database_relation = Some(ir::DatabaseRelation {
        parent_column: "parent_id".into(),
        child_column: "child_id".into(),
        foreign_key_side: ir::DatabaseForeignKeySide::Child,
    });
    cases.push(("database-relation metadata", relation));

    let mut nullable = valid_project();
    nullable.target.nullable = true;
    cases.push(("scalar nullability metadata", nullable));

    let mut container_nullable = valid_project();
    target_name(&mut container_nullable).container_nullable = true;
    cases.push(("container-nullability metadata", container_nullable));

    let mut arbitrary_json = valid_project();
    let target = target_name(&mut arbitrary_json);
    target.kind = SchemaKind::Scalar {
        ty: ScalarType::Int,
    };
    target.json_any = true;
    cases.push(("arbitrary-JSON metadata", arbitrary_json));

    for (expected, project) in cases {
        let issues = validate(&project);
        assert!(
            issues.iter().any(|issue| {
                issue.location == "target schema" && issue.message.contains(expected)
            }),
            "missing `{expected}` issue in {issues:?}"
        );
    }
}

#[test]
fn reports_dangling_references_paths_unknown_functions_and_cycles() {
    let mut project = valid_project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "mystery".into(),
            args: vec![99],
        },
    );
    project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![2],
        },
    );
    project.graph.nodes.insert(
        3,
        Node::SourceField {
            frame: None,
            path: vec!["missing".into()],
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("unused".into()),
        },
    );
    project.graph.nodes.insert(
        5,
        Node::Call {
            function: "replace".into(),
            args: vec![4, 4],
        },
    );
    project.graph.nodes.insert(
        6,
        Node::Call {
            function: "json_serialize_object".into(),
            args: vec![4, 4, 4, 4],
        },
    );
    project.root.set_source(None);
    project.root.filter = Some(88);
    project.root.group_by = Some(89);
    project.root.group_adjacent_by = Some(94);
    project.root.group_starting_with = Some(92);
    project.root.group_ending_with = Some(95);
    project.root.group_into_blocks = Some(93);
    project.root.sort_by = Some(90);
    project.root.windows = vec![SequenceWindow::First { count: 91 }];
    project.root.bindings.push(Binding {
        target_field: "missing".into(),
        node: 77,
    });
    project.root.children.push(Scope {
        target_field: "absent".into(),
        ..Scope::default()
    });

    let rendered: Vec<String> = validate(&project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect();
    for expected in [
        "unknown function `mystery`",
        "function `replace` expects 3 to 4 argument(s), got 2",
        "function `json_serialize_object` expects 3 argument(s), then complete groups of 3, got 4",
        "argument 0 references missing node 99",
        "cycle reaches node 2",
        "source field `missing` matches no scalar",
        "filter references missing node 88",
        "group-by key references missing node 89",
        "group-adjacent-by key references missing node 94",
        "group-starting-with predicate references missing node 92",
        "group-ending-with predicate references missing node 95",
        "group block size references missing node 93",
        "group-adjacent-by key has no iterated source",
        "group-starting-with predicate has no iterated source",
        "group-ending-with predicate has no iterated source",
        "group block size has no iterated source",
        "scope grouping modes are mutually exclusive",
        "sort key references missing node 90",
        "sequence window 1 references missing bound node 91",
        "filter has no iterated source",
        "sort key has no iterated source",
        "sequence window has no iterated source",
        "binding target `missing` does not exist",
        "binding for `missing` references missing node 77",
        "target scope does not exist",
    ] {
        assert!(
            rendered.iter().any(|issue| issue.contains(expected)),
            "missing `{expected}` in {rendered:#?}"
        );
    }
}
