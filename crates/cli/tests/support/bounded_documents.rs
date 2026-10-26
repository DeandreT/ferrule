#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

use cli::{NamedPayloadInput, PayloadDocument, PayloadRunOptions, TargetSelection};
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FixedFieldWidth, FixedWidthLayout, FormatOptions, Graph, Node, Project, Scope,
    ScopeConstruction, ScopeIteration, TabularBoundaryKind,
};

pub const PROTO: &str = "syntax = \"proto2\"; message Row { optional string Value = 1; }";

pub fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}

pub fn json() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..Default::default()
    }
}

pub fn xml() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}

pub fn csv(delimiter: char) -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        delimiter: Some(delimiter),
        has_header_row: Some(false),
        csv_preserve_empty_strings: true,
        ..Default::default()
    }
}

pub fn xlsx() -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Xlsx),
        xlsx_sheet: Some("Data".into()),
        xlsx_start_row: Some(2),
        xlsx_columns: vec![3],
        xlsx_headers: vec!["Physical label".into()],
        has_header_row: Some(true),
        ..Default::default()
    }
}

pub fn fixed(width: u32) -> FormatOptions {
    FormatOptions {
        fixed_width: Some(
            FixedWidthLayout::new(vec![FixedFieldWidth::new(width).unwrap()], ' ', true, false)
                .unwrap(),
        ),
        ..Default::default()
    }
}

pub fn protobuf() -> FormatOptions {
    FormatOptions {
        protobuf: Some(mapping::ProtobufOptions {
            schema: PROTO.into(),
            root_message: "Row".into(),
            schema_path: None,
            imports: vec![],
        }),
        ..Default::default()
    }
}

pub fn scope(source: Option<Vec<String>>, node: u32) -> Scope {
    Scope {
        iteration: source.map_or(ScopeIteration::None, ScopeIteration::Source),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Default::default()
    }
}

pub fn project(
    source_options: FormatOptions,
    target_options: FormatOptions,
    rows: bool,
) -> Project {
    Project {
        source: schema(),
        target: schema(),
        // Both saved and host paths intentionally name different codecs.
        source_path: Some("design-time.sqlite".into()),
        target_path: Some("design-time.json5".into()),
        source_options,
        target_options,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    frame: rows.then(Vec::new),
                    path: vec!["Value".into()],
                },
            )]),
        },
        root: scope(rows.then(Vec::new), 0),
    }
}

pub fn copy_project(schema: SchemaNode, options: FormatOptions) -> Project {
    let mut project = project(options.clone(), options, false);
    if let Some(boundary) = &project.source_options.xbrl {
        project.source_options.xbrl = Some(
            mapping::XbrlBoundaryOptions::external_source(boundary.taxonomy())
                .unwrap()
                .with_namespace_bindings(boundary.namespace_bindings().to_vec())
                .unwrap(),
        );
    }
    project.source = schema.clone();
    project.target = schema;
    project.graph.nodes.clear();
    project.root = Scope {
        construction: ScopeConstruction::CopyCurrentSource,
        ..Default::default()
    };
    project
}

pub fn run(
    project: &Project,
    input: &[u8],
    named: &[(&str, &[u8])],
    selection: Option<TargetSelection<'_>>,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    let primary = PayloadDocument::new(Path::new("uploaded.json5"), input)?;
    let named = named
        .iter()
        .map(|(name, bytes)| {
            NamedPayloadInput::new(
                name,
                PayloadDocument::new(Path::new("uploaded.sqlite"), bytes)?,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut options = PayloadRunOptions::new(primary)
        .with_extra_sources(&named)
        .with_output_path(Path::new("host-result.sqlite"))
        .with_bounded_documents();
    if let Some(selection) = selection {
        options = options.with_target(selection);
    }
    cli::run_project_value_payloads(project, Path::new("/virtual/project.json"), &options)
}

pub fn xbrl() -> (SchemaNode, FormatOptions, String) {
    let ns = "http://www.xbrl.org/2003/instance";
    let schema = SchemaNode::group(
        "xbrl",
        vec![
            SchemaNode::group(
                "rows",
                vec![
                    SchemaNode::group(
                        "identifier",
                        vec![
                            SchemaNode::scalar("scheme", ScalarType::String).attribute(),
                            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                        ],
                    ),
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
    let boundary =
        mapping::XbrlBoundaryOptions::external_target("urn:self-authored:taxonomy", None)
            .unwrap()
            .with_namespace_bindings(vec![
                mapping::XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], ns)
                    .unwrap(),
                mapping::XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], ns)
                    .unwrap(),
                mapping::XbrlNamespaceBinding::new(
                    vec!["rows".into(), "period".into(), "instant".into()],
                    ns,
                )
                .unwrap(),
                mapping::XbrlNamespaceBinding::new(
                    vec!["rows".into(), "Label".into()],
                    "urn:self-authored:fact",
                )
                .unwrap(),
            ])
            .unwrap();
    let input = format!(
        r#"<x:xbrl xmlns:x="{ns}" xmlns:f="urn:self-authored:fact"><x:context id="c"><x:entity><x:identifier scheme="urn:id">Entity</x:identifier></x:entity><x:period><x:instant>2026-10-26</x:instant></x:period></x:context><f:Label contextRef="c">zero</f:Label></x:xbrl>"#
    );
    (
        schema,
        FormatOptions {
            xbrl: Some(boundary),
            ..Default::default()
        },
        input,
    )
}
