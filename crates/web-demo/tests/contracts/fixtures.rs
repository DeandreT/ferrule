use std::num::NonZeroU32;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::*;
use web_demo::runtime::{DataFormat, UnsupportedContractFamily as Family};

pub const FORMATS: [DataFormat; 4] = [
    DataFormat::Xml,
    DataFormat::Json,
    DataFormat::Csv,
    DataFormat::Xbrl,
];
pub const JSON: &str = r#"{"name":"Ada"}"#;
pub const PRETTY_JSON: &str = "{\n  \"name\": \"Ada\"\n}\n";

pub fn project() -> Project {
    let mut project = Project {
        execution_contract: None,
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Default::default(),
    };
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    project.root.bindings.push(Binding {
        target_field: "name".into(),
        node: 0,
    });
    project
}

pub fn name(value: &str) -> Instance {
    Instance::Group(vec![("name".into(), Instance::Scalar(Value::String(value.into())))].into())
}

pub fn flextext(root: &str) -> FlexTextLayout {
    FlexTextLayout::new(
        root,
        FlexCommand::Store {
            name: "name".into(),
            ty: ScalarType::String,
            trim: None,
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap()
}

pub fn pdf() -> PdfLayout {
    PdfLayout::new(
        "Source",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "name".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
}

pub fn idoc() -> IdocLayout {
    IdocLayout::new(vec![
        IdocSegmentLayout::new(
            "RECORD",
            vec![
                IdocFieldLayout::new(
                    "name",
                    NonZeroU32::new(8).unwrap(),
                    NonZeroU32::new(12).unwrap(),
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    ])
    .unwrap()
}

pub fn swift() -> SwiftMtLayout {
    SwiftMtLayout::new(vec![SwiftMessageLayout::new(
        "MT950",
        vec![SwiftFieldLayout::new(
            "20",
            vec!["MT950".into(), "20".into()],
            false,
            SwiftValueExpr::Capture {
                path: vec![],
                min: 1,
                max: 16,
                charset: SwiftCharset::Text,
            },
        )],
    )])
    .unwrap()
}

pub fn swift_schema() -> SchemaNode {
    SchemaNode::group(
        "SWIFT",
        vec![
            SchemaNode::group(
                "Message",
                vec![
                    SchemaNode::group(
                        "Application Header",
                        vec![
                            SchemaNode::group("Input", vec![]),
                            SchemaNode::group("Output", vec![]),
                        ],
                    ),
                    SchemaNode::group("MT950", vec![SchemaNode::scalar("20", ScalarType::String)]),
                ],
            )
            .repeating(),
        ],
    )
}

pub fn swift_output() -> SwiftMtOutputOptions {
    SwiftMtOutputOptions::new(
        SwiftMtDirection::Input,
        vec![SwiftMtMessageHeaders {
            basic_header: "F01BANKBEBBAXXX0000000001".into(),
            application_header: "BANKDEFFXXXXN".into(),
        }],
    )
    .unwrap()
}

pub fn protobuf() -> ProtobufOptions {
    ProtobufOptions {
        schema: "syntax = \"proto3\"; message Source { string name = 1; }".into(),
        root_message: "Source".into(),
        schema_path: None,
        imports: vec![],
    }
}

pub fn grid() -> XlsxGridLayout {
    XlsxGridLayout {
        sheet: None,
        header_row: XlsxRow::new(1).unwrap(),
        data_start_row: XlsxRow::new(2).unwrap(),
        header_value_field: "header".into(),
        header_position_field: "column".into(),
        rows_field: "rows".into(),
        cells_field: "cells".into(),
        cell_value_field: "value".into(),
        cell_position_field: "column".into(),
        fixed_cells: vec![],
    }
}

pub fn composite() -> XlsxCompositeLayout {
    XlsxCompositeLayout {
        table: Some(XlsxTableRegion {
            path: vec![],
            sheet: None,
            start_row: XlsxRow::new(1).unwrap(),
            columns: vec![],
            has_header: true,
            row_number_field: None,
        }),
        additional_tables: vec![],
        records: vec![],
        grids: vec![],
    }
}

pub fn worksheet_set() -> XlsxWorksheetSetLayout {
    XlsxWorksheetSetLayout {
        worksheets_path: vec!["sheets".into()],
        worksheet_name_path: vec!["name".into()],
        rows_path: vec!["rows".into()],
        row_number_path: None,
        start_row: XlsxRow::new(1).unwrap(),
        columns: vec![],
        has_header: true,
    }
}

pub fn hierarchical() -> XlsxHierarchicalLayout {
    XlsxHierarchicalLayout {
        worksheets_path: vec!["sheets".into()],
        worksheet_name_path: vec!["name".into()],
        ranges: vec![XlsxOutputRange {
            path: vec!["rows".into()],
            start: XlsxRangeStart::Absolute {
                row: XlsxRow::new(1).unwrap(),
            },
            count: None,
            has_header: true,
            columns: vec![XlsxOutputColumn {
                path: vec!["name".into()],
                column: XlsxColumn::new(1).unwrap(),
                header: Some("Name".into()),
                kind: XlsxCellKind::String,
            }],
        }],
    }
}

pub fn sqlite_read() -> SqliteReadProfile {
    SqliteReadProfile::SqliteReadV1(SqliteReadV1 {
        table: "people".into(),
        key: "id".into(),
        projection: vec![
            SqliteReadProjection {
                field: "id".into(),
                column: "id".into(),
            },
            SqliteReadProjection {
                field: "name".into(),
                column: "name".into(),
            },
        ],
        parameters: vec![],
        all: vec![],
        order_by: vec![SqliteReadOrder {
            column: "id".into(),
            ty: ScalarType::Int,
            direction: SqliteReadDirection::Asc,
            nulls: SqliteReadNulls::Last,
        }],
    })
}

// Each entry sets exactly one non-default runtime field. In particular, no
// family discriminator may mask an independently ignored policy or layout.
pub fn cases() -> Vec<(&'static str, Family, FormatOptions)> {
    macro_rules! case {
        ($field:ident, $family:ident, $value:expr) => {
            (
                stringify!($field),
                Family::$family,
                FormatOptions {
                    $field: $value,
                    ..Default::default()
                },
            )
        };
    }
    vec![
        case!(flextext, FlexText, Some(flextext("Source"))),
        case!(pdf, Pdf, Some(pdf())),
        case!(protobuf, Protobuf, Some(protobuf())),
        case!(idoc, Idoc, Some(idoc())),
        case!(swift_mt, SwiftMt, Some(swift())),
        case!(swift_mt_output, SwiftMt, Some(swift_output())),
        case!(xlsx_sheet, Xlsx, Some("People".into())),
        case!(xlsx_start_row, Xlsx, Some(1)),
        case!(xlsx_columns, Xlsx, vec![1]),
        case!(xlsx_headers, Xlsx, vec!["Name".into()]),
        case!(xlsx_rows, Xlsx, vec![1]),
        case!(xlsx_composite, Xlsx, Some(composite())),
        case!(xlsx_worksheet_set, Xlsx, Some(worksheet_set())),
        case!(xlsx_grid, Xlsx, Some(grid())),
        case!(xlsx_hierarchical, Xlsx, Some(hierarchical())),
        case!(xlsx_update_existing, Xlsx, true),
        case!(lenient_segments, Edi, true),
        case!(
            edi_implied_decimals,
            Edi,
            vec![EdiImpliedDecimal::new(vec!["name".into()], 2).unwrap()]
        ),
        case!(
            edi_lexical_formats,
            Edi,
            vec![EdiLexicalFormat::new(vec!["name".into()], EdiLexicalKind::CompactDate8).unwrap()]
        ),
        case!(
            edi_value_constraints,
            Edi,
            vec![EdiValueConstraint::new(vec!["name".into()], 1, 3, vec!["Ada".into()]).unwrap()]
        ),
        case!(edi_autocomplete, Edi, Some(EdiAutocomplete::Hl7)),
        case!(
            x12_separators,
            Edi,
            Some(X12Separators {
                element: '*',
                component: ':',
                segment: '~',
                repetition: Some('^'),
                release: None
            })
        ),
        case!(x12_interchange_version, Edi, Some("00501".into())),
        case!(sqlite_read, SqliteRead, Some(sqlite_read())),
        case!(local_xml_file_set, LocalXmlFileSet, true),
        case!(
            edi_config_reference,
            EdiConfigReference,
            Some(EdiConfigDependency::external_reference("missing.config"))
        ),
        (
            "edi_config_reference_missing",
            Family::EdiConfigReference,
            FormatOptions {
                edi_config_reference: Some(EdiConfigDependency::missing_configuration()),
                ..Default::default()
            },
        ),
    ]
}
