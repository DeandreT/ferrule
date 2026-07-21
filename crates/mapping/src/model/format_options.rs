use serde::{Deserialize, Serialize};

use crate::{
    EdiAutocomplete, EdiBoundaryKind, EdiImpliedDecimal, EdiLexicalFormat, ExternalSourceOptions,
    FixedWidthLayout, FlexTextLayout, HttpGetOptions, IdocLayout, PdfLayout, ProtobufOptions,
    SwiftMtLayout, TabularBoundaryKind, WsdlMessageOptions, X12Separators, XbrlBoundaryOptions,
    XlsxHierarchicalLayout, is_false,
};

macro_rules! xlsx_coordinate {
    ($name:ident, $max:expr, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            pub const MAX: u32 = $max;

            pub const fn new(value: u32) -> Option<Self> {
                if value >= 1 && value <= Self::MAX {
                    Some(Self(value))
                } else {
                    None
                }
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = u32::deserialize(deserializer)?;
                Self::new(value).ok_or_else(|| {
                    serde::de::Error::custom(format_args!(
                        "XLSX {} must be between 1 and {}",
                        $label,
                        Self::MAX
                    ))
                })
            }
        }
    };
}

xlsx_coordinate!(XlsxRow, 1_048_576, "row");
xlsx_coordinate!(XlsxColumn, 16_384, "column");

/// One repeated row table inside a composite XLSX workbook source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxTableRegion {
    /// Absolute path to a repeating flat group in the source schema.
    pub path: Vec<String>,
    /// Named worksheet; the first worksheet is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub start_row: XlsxRow,
    /// Columns aligned with the table group's scalar children. Empty means
    /// consecutive columns beginning at A.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<XlsxColumn>,
    pub has_header: bool,
}

/// One scalar field read from a fixed worksheet coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxFixedCell {
    /// Path relative to the owning fixed record group.
    pub path: Vec<String>,
    pub row: XlsxRow,
    pub column: XlsxColumn,
}

/// One schema-shaped singleton record assembled from fixed worksheet cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxFixedRecord {
    /// Absolute path to a group in the source schema; empty means the root.
    pub path: Vec<String>,
    /// Named worksheet; the first worksheet is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub cells: Vec<XlsxFixedCell>,
}

/// Composite XLSX source layout with one repeated table and fixed records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxCompositeLayout {
    pub table: XlsxTableRegion,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<XlsxFixedRecord>,
}

/// One two-dimensional worksheet grid exposed as header records containing
/// the complete nested row/cell matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxGridLayout {
    /// Named worksheet; the first worksheet is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// One-based row whose non-empty cells drive the outer records.
    pub header_row: XlsxRow,
    /// One-based first physical row in the nested data matrix.
    pub data_start_row: XlsxRow,
    /// Direct root scalar containing the current header cell value.
    pub header_value_field: String,
    /// Direct root integer scalar containing the header's physical column.
    pub header_position_field: String,
    /// Direct root repeating group containing the data rows.
    pub rows_field: String,
    /// Direct repeating group below each row containing its physical cells.
    pub cells_field: String,
    /// Direct scalar below each cell containing its value.
    pub cell_value_field: String,
    /// Direct integer scalar below each cell containing its physical column.
    pub cell_position_field: String,
    /// Root-relative scalar fields read from fixed worksheet coordinates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_cells: Vec<XlsxFixedCell>,
}

/// Per-side format knobs. This is deliberately one flat bag of optional
/// settings rather than per-format sub-structs: each format adapter reads
/// only the fields that concern it, `mapping` stays free of format-crate
/// dependencies, and old project files load unchanged (everything
/// defaults).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FormatOptions {
    /// EDI: skip segments the schema doesn't mention instead of erroring
    /// on them. Skipping is bounded by the schema's own expectations, so
    /// declared segments are never swallowed.
    #[serde(default)]
    pub lenient_segments: bool,
    /// EDI document family retained independently of the instance extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edi_kind: Option<EdiBoundaryKind>,
    /// EDI decimal leaves whose wire values have fixed implied fractional
    /// places. Paths are compiled from the owning external configuration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edi_implied_decimals: Vec<EdiImpliedDecimal>,
    /// EDI leaves whose declared configuration compacts XML date/time lexical
    /// forms for the wire representation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edi_lexical_formats: Vec<EdiLexicalFormat>,
    /// Dialect-specific trailer and control-count completion retained from
    /// an EDI target boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edi_autocomplete: Option<EdiAutocomplete>,
    /// ANSI X12 syntax retained from the mapping boundary. These separators
    /// override the writer defaults and provide the optional release character
    /// that cannot be discovered from an ISA envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x12_separators: Option<X12Separators>,
    /// ANSI X12 interchange version used to complete an unbound ISA12 field.
    /// When present it is exactly five ASCII digits retained from the mapping
    /// boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x12_interchange_version: Option<String>,
    /// SAP IDoc: embedded fixed-record layout compiled from the external
    /// parser configuration. This mode is input-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idoc: Option<IdocLayout>,
    /// SWIFT MT: embedded selected-message field grammar. This mode is
    /// input-only and takes precedence over the file extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swift_mt: Option<SwiftMtLayout>,
    /// XML document identity retained when no instance filename is available
    /// to carry an `.xml` extension.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub xml_document: bool,
    /// WSDL operation and message identity retained for canonical kind-17
    /// request, response, or fault component export. Runtime I/O remains XML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wsdl: Option<WsdlMessageOptions>,
    /// XML: the source path names a bounded local wildcard file set rather
    /// than one document. Hosts expand it beneath their declared input base
    /// and present the documents as one ordered source sequence.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub local_xml_file_set: bool,
    /// JSON component identity retained when no instance filename is available
    /// to carry a `.json`, `.jsonl`, or `.ndjson` extension.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_document: bool,
    /// Flat tabular component family retained when an instance filename has
    /// no recognized format extension. Explicit extensions and embedded
    /// format adapters take precedence over this fallback identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabular_kind: Option<TabularBoundaryKind>,
    /// CSV: the field delimiter (default `,`).
    #[serde(default)]
    pub delimiter: Option<char>,
    /// CSV: whether the file's first row is a header (default true).
    #[serde(default)]
    pub has_header_row: Option<bool>,
    /// Fixed-width text layout. When set, CSV delimiter/header options do
    /// not apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_width: Option<FixedWidthLayout>,
    /// FlexText-style recursive structured text layout. This mode takes
    /// precedence over the file extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flextext: Option<FlexTextLayout>,
    /// PDF visual extraction layout. This mode is input-only and takes
    /// precedence over the file extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<PdfLayout>,
    /// Static HTTP GET transport policy. The request URL remains the owning
    /// source path so callers can still override it with a local file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_get: Option<HttpGetOptions>,
    /// Typed value captured outside ferrule from an opaque UDF or HTTP POST.
    /// A local response file is executable; ferrule never invokes the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_source: Option<ExternalSourceOptions>,
    /// JSON: read and write one root value per line instead of one enclosing
    /// JSON document.
    #[serde(default, skip_serializing_if = "is_false")]
    pub json_lines: bool,
    /// Protocol Buffers: embedded proto2/proto3 schema and selected message.
    /// This mode takes precedence over the file extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protobuf: Option<ProtobufOptions>,
    /// XBRL taxonomy and table contract metadata used by the runtime adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xbrl: Option<XbrlBoundaryOptions>,
    /// XLSX: worksheet name. The first sheet is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xlsx_sheet: Option<String>,
    /// XLSX: one-based row where the table starts (default 1). When a
    /// header is enabled, this is the header row and data begins below it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xlsx_start_row: Option<u32>,
    /// XLSX: one-based worksheet columns aligned with the schema fields.
    /// Empty means consecutive columns starting at A.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xlsx_columns: Vec<u32>,
    /// XLSX: optional physical header text aligned with the schema fields.
    /// This permits distinct field identifiers to address duplicate headers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xlsx_headers: Vec<String>,
    /// XLSX: replace the selected table in an existing workbook while
    /// preserving all cells and worksheets outside that table.
    #[serde(default, skip_serializing_if = "is_false")]
    pub xlsx_update_existing: bool,
    /// XLSX: one-based worksheet rows to transpose into schema fields.
    /// Empty selects the ordinary row-oriented table layout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xlsx_rows: Vec<u32>,
    /// XLSX: one repeated table plus schema-shaped records read from fixed
    /// worksheet cells. This mode is mutually exclusive with the legacy
    /// flat/transposed XLSX fields above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xlsx_composite: Option<XlsxCompositeLayout>,
    /// XLSX: a two-dimensional matrix repeated once per non-empty header
    /// cell. This mode is input-only and mutually exclusive with every
    /// other XLSX layout option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xlsx_grid: Option<XlsxGridLayout>,
    /// XLSX: repeated runtime-named worksheets containing ordered output row
    /// ranges. This mode is output-only and mutually exclusive with every
    /// other XLSX layout option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xlsx_hierarchical: Option<XlsxHierarchicalLayout>,
}
