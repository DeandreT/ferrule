//! Schema component and port-tree rendering for MFD export.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use crate::MfdError;

const XLSX_MAX_ROW: u32 = 1_048_576;
const XLSX_MAX_COLUMN: u32 = 16_384;

/// Which MapForce component family a mapping side exports as.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SideFormat {
    Xml,
    Json,
    Csv,
    FixedWidth,
    Xlsx,
    Db,
}

pub(super) fn side_format(instance_path: &Option<String>, options: &FormatOptions) -> SideFormat {
    if options.fixed_width.is_some() {
        return SideFormat::FixedWidth;
    }
    let ext = instance_path
        .as_deref()
        .and_then(|p| Path::new(p).extension())
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    match ext.as_deref() {
        Some("json") | Some("jsonl") | Some("ndjson") => SideFormat::Json,
        Some("csv") | Some("txt") => SideFormat::Csv,
        Some("xlsx") => SideFormat::Xlsx,
        Some("db") | Some("sqlite") | Some("sqlite3") => SideFormat::Db,
        _ => SideFormat::Xml,
    }
}

/// The datasource name a connection path registers under (its file stem).
pub(super) fn db_datasource_name(instance_path: Option<&str>) -> String {
    instance_path
        .and_then(|p| Path::new(p).file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_string()
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Side {
    Source,
    Target,
}

impl Side {
    fn port_attr(self) -> &'static str {
        match self {
            Side::Source => "outkey",
            Side::Target => "inpkey",
        }
    }

    fn instance_attr(self) -> &'static str {
        match self {
            Side::Source => "inputinstance",
            Side::Target => "outputinstance",
        }
    }
}

pub(super) struct GeneratedSibling {
    pub(super) path: PathBuf,
    pub(super) contents: String,
}

pub(super) struct RenderedSchemaComponent {
    pub(super) xml: String,
    pub(super) sibling: Option<GeneratedSibling>,
}

/// Renders one schema component and its optional generated schema sibling.
/// The caller writes artifacts only after both mapping sides validate.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_schema_component(
    schema: &SchemaNode,
    format: SideFormat,
    ports: &PortTree,
    side: Side,
    instance_path: Option<&str>,
    options: &FormatOptions,
    mfd_path: &Path,
    force_root_port: bool,
) -> Result<RenderedSchemaComponent, MfdError> {
    let stem = mfd_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mapping");
    let dir = mfd_path.parent().unwrap_or(Path::new("."));
    let (uid, side_name, header, view) = match side {
        Side::Source => (2, "source", "", "<view rbx=\"300\" rby=\"400\"/>"),
        Side::Target => (
            3,
            "target",
            "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t",
            "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
        ),
    };
    let attr = side.port_attr();
    let instance = instance_path
        .map(|p| format!(" {}=\"{}\"", side.instance_attr(), xml_escape(p)))
        .unwrap_or_default();

    let mut out = String::new();
    let mut sibling = None;
    match format {
        SideFormat::Xml => {
            let schema_file = format!("{stem}-{side_name}.xsd");
            sibling = Some(GeneratedSibling {
                path: dir.join(&schema_file),
                contents: format_xml::xsd::export(schema)?,
            });
            if let Some(http) = options.http_get {
                if side != Side::Source {
                    return Err(MfdError::Unsupported(
                        "HTTP GET transport is valid only for mapping sources".to_string(),
                    ));
                }
                let url = instance_path.ok_or_else(|| {
                    MfdError::Unsupported(
                        "an HTTP GET source requires its URL in source_path".to_string(),
                    )
                })?;
                if !valid_http_url(url) {
                    return Err(MfdError::Unsupported(
                        "an HTTP GET source requires an HTTP(S) URL without credentials or a fragment"
                            .to_string(),
                    ));
                }
                let _ = write!(
                    out,
                    "\t\t\t\t<component name=\"GET {}\" library=\"webservice\" uid=\"{uid}\" kind=\"20\">\n\
                     \t\t\t\t\t<properties/>\n\
                     \t\t\t\t\t{view}\n\
                     \t\t\t\t\t<data>\n\
                     \t\t\t\t\t\t<root><entry name=\"HTTPMessage\"><entry name=\"HTTPBody\"/></entry></root>\n\
                     \t\t\t\t\t\t<root rootindex=\"1\">\n\
                     \t\t\t\t\t\t\t<entry name=\"HTTPMessage\" expanded=\"1\">\n\
                     \t\t\t\t\t\t\t\t<entry name=\"HTTPBody\" expanded=\"1\">\n\
                     \t\t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-xml\" expanded=\"1\">\n\
                     \t\t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" root=\"{}\" encoding=\"UTF-8\"/>\n\
                     {}\
                     \t\t\t\t\t\t\t\t\t</entry>\n\
                     \t\t\t\t\t\t\t\t</entry>\n\
                     \t\t\t\t\t\t\t</entry>\n\
                     \t\t\t\t\t\t</root>\n\
                     \t\t\t\t\t\t<wsdl kind=\"call\" sourceMode=\"manual\" url=\"{}\" timeout=\"{}\" httpmethod=\"GET\"/>\n\
                     \t\t\t\t\t</data>\n\
                     \t\t\t\t</component>\n",
                    xml_escape(&schema.name),
                    xml_escape(&schema_file),
                    xml_escape(&schema.name),
                    ports.entries_xml(schema, attr, 10, true),
                    xml_escape(url),
                    http.timeout_seconds().get(),
                );
                return Ok(RenderedSchemaComponent { xml: out, sibling });
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"{uid}\" kind=\"14\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 {}\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{{}}{}\"{instance}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                ports.entries_xml(schema, attr, 9, force_root_port),
                xml_escape(&schema_file),
                xml_escape(&schema.name),
            );
        }
        SideFormat::Json => {
            let schema_file = format!("{stem}-{side_name}.schema.json");
            let json_lines = options.json_lines
                || instance_path
                    .and_then(|path| Path::new(path).extension())
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extension.eq_ignore_ascii_case("jsonl")
                            || extension.eq_ignore_ascii_case("ndjson")
                    });
            sibling = Some(GeneratedSibling {
                path: dir.join(&schema_file),
                contents: format_json::json_schema::export(schema),
            });
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{uid}\" kind=\"31\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"root\" expanded=\"1\">\n\
                 {}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<json schema=\"{}\"{instance}{json_lines}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                ports.json_entries_xml(schema, attr, 10),
                xml_escape(&schema_file),
                json_lines = if json_lines { " jsonlines=\"1\"" } else { "" },
            );
        }
        SideFormat::Db => {
            // Unlike a csv row schema, a table root is repeating by
            // format-db convention; only the children's shape matters.
            let fields = flat_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to a database table but its schema \
                     is not a flat group of scalar fields"
                ))
            })?;
            let datasource = db_datasource_name(instance_path);
            let table_key = ports.required_key_for_abs(&[], "database table")?;
            let mut column_entries = String::new();
            for (column, _) in &fields {
                let key =
                    ports.required_key_for_abs(&[(*column).to_string()], "database column")?;
                let _ = writeln!(
                    column_entries,
                    "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                    xml_escape(column)
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{0}\" library=\"db\" uid=\"{uid}\" kind=\"15\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"{0}\" type=\"table\" {attr}=\"{table_key}\" expanded=\"1\">\n\
                 {column_entries}\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<database ref=\"{1}\">\n\
                 \t\t\t\t\t\t\t<data><selections><selection><PathElement Name=\"main\" Kind=\"Database\"/><PathElement Name=\"{0}\" Kind=\"Table\"/></selection></selections></data>\n\
                 \t\t\t\t\t\t</database>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                xml_escape(&datasource),
            );
        }
        SideFormat::Csv => {
            let fields = csv_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to a csv file but its schema is \
                     not a flat group of scalar fields"
                ))
            })?;
            let block_key = ports.required_key_for_abs(&[], "CSV row block")?;
            let mut field_entries = String::new();
            let mut field_decls = String::new();
            for (i, (name, ty)) in fields.iter().enumerate() {
                let key = ports.required_key_for_abs(&[(*name).to_string()], "CSV field")?;
                let _ = writeln!(
                    field_entries,
                    "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                    xml_escape(name)
                );
                let _ = writeln!(
                    field_decls,
                    "\t\t\t\t\t\t\t\t<field{i} name=\"{}\" type=\"{}\"/>",
                    xml_escape(name),
                    csv_type_name(*ty)
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"Rows\" {attr}=\"{block_key}\" expanded=\"1\">\n\
                 {field_entries}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<text type=\"csv\"{instance}>\n\
                 \t\t\t\t\t\t\t<settings separator=\"{}\" quote=\"&quot;\" firstrownames=\"{}\">\n\
                 \t\t\t\t\t\t\t\t<names root=\"{}\" block=\"Rows\">\n\
                 {field_decls}\
                 \t\t\t\t\t\t\t\t</names>\n\
                 \t\t\t\t\t\t\t</settings>\n\
                 \t\t\t\t\t\t</text>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                xml_escape(&options.delimiter.unwrap_or(',').to_string()),
                options.has_header_row.unwrap_or(true),
                xml_escape(&schema.name),
            );
        }
        SideFormat::FixedWidth => {
            if options.delimiter.is_some() || options.has_header_row.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} fixed-width layout conflicts with CSV delimiter/header options"
                )));
            }
            let fields = csv_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side has a fixed-width layout but its schema is not a flat group of scalar fields"
                ))
            })?;
            let layout = options.fixed_width.as_ref().ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} fixed-width component has no layout"
                ))
            })?;
            if layout.field_widths().len() != fields.len() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} fixed-width layout has {} width(s) for {} schema field(s)",
                    layout.field_widths().len(),
                    fields.len()
                )));
            }
            let block_key = ports.required_key_for_abs(&[], "fixed-width row block")?;
            let mut field_entries = String::new();
            let mut field_decls = String::new();
            for (index, ((name, ty), width)) in fields.iter().zip(layout.field_widths()).enumerate()
            {
                let key =
                    ports.required_key_for_abs(&[(*name).to_string()], "fixed-width field")?;
                let _ = writeln!(
                    field_entries,
                    "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                    xml_escape(name)
                );
                let _ = writeln!(
                    field_decls,
                    "\t\t\t\t\t\t\t\t<field{index} name=\"{}\" type=\"{}\" length=\"{}\"/>",
                    xml_escape(name),
                    csv_type_name(*ty),
                    width.get()
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"Rows\" {attr}=\"{block_key}\" expanded=\"1\">\n\
                 {field_entries}\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<text type=\"flf\"{instance} encoding=\"1000\" byteorder=\"1\" byteordermark=\"0\">\n\
                 \t\t\t\t\t\t\t<settings delimiter=\"{}\" fillchar=\"{}\" removeempty=\"{}\">\n\
                 \t\t\t\t\t\t\t\t<names root=\"{}\" block=\"Rows\">\n\
                 {field_decls}\
                 \t\t\t\t\t\t\t\t</names>\n\
                 \t\t\t\t\t\t\t</settings>\n\
                 \t\t\t\t\t\t</text>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                layout.record_delimiters(),
                xml_escape(&layout.fill_char().to_string()),
                layout.treat_empty_as_absent(),
                xml_escape(&schema.name),
            );
        }
        SideFormat::Xlsx => {
            if options.xlsx_grid.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} XLSX layout is a grid; grid XLSX export is not supported"
                )));
            }
            if options.xlsx_composite.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} XLSX layout is composite; composite XLSX export is not supported"
                )));
            }
            if !options.xlsx_rows.is_empty() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} XLSX layout is transposed; transposed XLSX export is not supported"
                )));
            }
            let fields = csv_fields(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to an XLSX worksheet but its schema is \
                     not a flat group of scalar fields"
                ))
            })?;
            let row_key = ports.required_key_for_abs(&[], "XLSX row")?;
            let start_row = options.xlsx_start_row.unwrap_or(1);
            if !(1..=XLSX_MAX_ROW).contains(&start_row) {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} XLSX start row must be between 1 and {XLSX_MAX_ROW}"
                )));
            }
            let columns = xlsx_columns(fields.len(), &options.xlsx_columns, side_name)?;
            let sheet = options
                .xlsx_sheet
                .as_deref()
                .filter(|sheet| !sheet.is_empty())
                .unwrap_or("Sheet1");
            let row_header_attr = if options.has_header_row.unwrap_or(true) {
                " enabletitlerow=\"1\""
            } else {
                ""
            };
            let mut cells = String::new();
            for ((name, ty), column) in fields.iter().zip(columns) {
                let key = ports.required_key_for_abs(&[(*name).to_string()], "XLSX cell")?;
                let (datatype, storage_type) = xlsx_type_name(*ty);
                let _ = write!(
                    cells,
                    "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" {attr}=\"{key}\" annotation=\"{}\" datatype=\"{datatype}\">\n\
                     \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"logical-and\" library=\"core\">\n\
                     \t\t\t\t\t\t\t\t\t\t\t\t\t<expression><function name=\"equal\" library=\"core\"><expression><attribute name=\"n\"/></expression><expression><constant value=\"{column}\" datatype=\"long\"/></expression></function></expression>\n\
                     \t\t\t\t\t\t\t\t\t\t\t\t\t<expression><function name=\"equal\" library=\"core\"><expression><attribute name=\"t\"/></expression><expression><constant value=\"{storage_type}\"/></expression></function></expression>\n\
                     \t\t\t\t\t\t\t\t\t\t\t\t</function></expression></condition>\n\
                     \t\t\t\t\t\t\t\t\t\t\t</entry>\n",
                    xml_escape(name),
                );
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"xlsx\" uid=\"{uid}\" kind=\"26\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/><namespace uid=\"http://www.altova.com/mapforce\"/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\" ns=\"1\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" ns=\"1\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t<entry name=\"Workbook\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t\t<entry name=\"Worksheet\" expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal-ignorecase\" library=\"xlsx\"><expression><attribute name=\"Name\"/></expression><expression><constant value=\"{}\"/></expression></function></expression></condition>\n\
                 \t\t\t\t\t\t\t\t\t\t\t<ranges><range id=\"1\" start=\"{start_row}\"/></ranges>\n\
                 \t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\" displayselectionmode=\"selection\"><entry name=\"Cell\" datatype=\"string\"/></entry>\n\
                 \t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" {attr}=\"{row_key}\" expanded=\"1\"{row_header_attr}>\n\
                 \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"1\" datatype=\"long\"/></expression></function></expression></condition>\n\
                 \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" displayselectionmode=\"selection\" datatype=\"string\"/>\n\
                 {cells}\
                 \t\t\t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<excel{instance}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(&schema.name),
                xml_escape(sheet),
            );
        }
    }
    Ok(RenderedSchemaComponent { xml: out, sibling })
}

/// The flat scalar fields a csv component needs, or `None` when the schema
/// has any other shape.
fn csv_fields(schema: &SchemaNode) -> Option<Vec<(&str, ScalarType)>> {
    if schema.repeating {
        return None;
    }
    flat_fields(schema)
}

/// The scalar children of a flat group, ignoring the root's own
/// repetition (db tables repeat by convention).
fn flat_fields(schema: &SchemaNode) -> Option<Vec<(&str, ScalarType)>> {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .map(|c| match &c.kind {
                SchemaKind::Scalar { ty } if !c.repeating && !c.attribute => {
                    Some((c.name.as_str(), *ty))
                }
                _ => None,
            })
            .collect(),
        SchemaKind::Scalar { .. } => None,
    }
}

fn csv_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

fn xlsx_columns(
    field_count: usize,
    configured: &[u32],
    side_name: &str,
) -> Result<Vec<u32>, MfdError> {
    let columns = if configured.is_empty() {
        let count = u32::try_from(field_count).map_err(|_| {
            MfdError::Unsupported(format!(
                "the {side_name} XLSX schema declares too many columns"
            ))
        })?;
        (1..=count).collect::<Vec<_>>()
    } else {
        if configured.len() != field_count {
            return Err(MfdError::Unsupported(format!(
                "the {side_name} XLSX layout has {} column selector(s) for {field_count} field(s)",
                configured.len()
            )));
        }
        configured.to_vec()
    };
    let mut unique = std::collections::BTreeSet::new();
    if columns
        .iter()
        .any(|column| !(1..=XLSX_MAX_COLUMN).contains(column) || !unique.insert(*column))
    {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} XLSX column selectors must be unique numbers between 1 and \
             {XLSX_MAX_COLUMN}"
        )));
    }
    Ok(columns)
}

fn xlsx_type_name(ty: ScalarType) -> (&'static str, &'static str) {
    match ty {
        ScalarType::String => ("string", "s"),
        ScalarType::Int => ("long", "n"),
        ScalarType::Float => ("double", "n"),
        ScalarType::Bool => ("boolean", "b"),
    }
}

pub(super) struct KeyAlloc {
    pub(super) next: u32,
}

impl KeyAlloc {
    pub(super) fn next(&mut self) -> u32 {
        let key = self.next;
        self.next += 1;
        key
    }
}

/// Port keys assigned to every node of a schema, addressable by absolute
/// path.
pub(super) struct PortTree {
    by_abs: BTreeMap<Vec<String>, u32>,
}

pub(super) enum PortMatch {
    Missing,
    Unique(u32),
    Ambiguous,
}

impl PortTree {
    pub(super) fn build(schema: &SchemaNode, keys: &mut KeyAlloc) -> Self {
        let mut by_abs = BTreeMap::new();
        // The document root itself: rendered as a port only by row/array
        // shaped components (a csv block, a json root object).
        by_abs.insert(Vec::new(), keys.next());
        fn walk(
            node: &SchemaNode,
            path: &mut Vec<String>,
            keys: &mut KeyAlloc,
            by_abs: &mut BTreeMap<Vec<String>, u32>,
        ) {
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    if child.text {
                        let parent = &path[..path.len() - 1];
                        let key = by_abs[parent];
                        by_abs.insert(path.clone(), key);
                        path.pop();
                        continue;
                    }
                    by_abs.insert(path.clone(), keys.next());
                    walk(child, path, keys, by_abs);
                    path.pop();
                }
            }
        }
        walk(schema, &mut Vec::new(), keys, &mut by_abs);
        Self { by_abs }
    }

    pub(super) fn key_for_abs(&self, abs: &[String]) -> Option<u32> {
        self.by_abs.get(abs).copied()
    }

    fn required_key_for_abs(&self, abs: &[String], kind: &str) -> Result<u32, MfdError> {
        self.key_for_abs(abs).ok_or_else(|| {
            let path = if abs.is_empty() {
                "<root>".to_string()
            } else {
                abs.join("/")
            };
            MfdError::Unsupported(format!("internal {kind} port `{path}` was not allocated"))
        })
    }

    /// Finds a unique absolute path ending in `suffix`. SourceField paths
    /// can be cut at an enclosing iteration frame, but choosing one of
    /// several equal tails would silently miswire the exported mapping.
    pub(super) fn match_suffix(&self, suffix: &[String]) -> PortMatch {
        if suffix.is_empty() {
            return self
                .key_for_abs(suffix)
                .map_or(PortMatch::Missing, PortMatch::Unique);
        }
        let mut matches = self
            .by_abs
            .iter()
            .filter(|(abs, _)| abs.ends_with(suffix))
            .map(|(_, &key)| key);
        let Some(first) = matches.next() else {
            return PortMatch::Missing;
        };
        if matches.next().is_some() {
            PortMatch::Ambiguous
        } else {
            PortMatch::Unique(first)
        }
    }

    /// Entry-tree XML for a schema with `attr` (outkey/inpkey) on every
    /// entry.
    fn entries_xml(
        &self,
        schema: &SchemaNode,
        attr: &str,
        indent: usize,
        force_root_port: bool,
    ) -> String {
        let mut out = String::new();
        fn walk(
            node: &SchemaNode,
            path: &mut Vec<String>,
            attr: &str,
            indent: usize,
            by_abs: &BTreeMap<Vec<String>, u32>,
            out: &mut String,
        ) {
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children.iter().filter(|child| !child.text) {
                    path.push(child.name.clone());
                    let pad = "\t".repeat(indent);
                    let key = by_abs[&*path];
                    let type_attr = if child.attribute {
                        " type=\"attribute\""
                    } else {
                        ""
                    };
                    let _ = write!(
                        out,
                        "{pad}<entry name=\"{}\"{type_attr} {attr}=\"{key}\" expanded=\"1\"",
                        xml_escape(&child.name)
                    );
                    if matches!(child.kind, SchemaKind::Scalar { .. }) {
                        out.push_str("/>\n");
                    } else {
                        out.push_str(">\n");
                        walk(child, path, attr, indent + 1, by_abs, out);
                        let _ = writeln!(out, "{pad}</entry>");
                    }
                    path.pop();
                }
            }
        }
        // The document root itself is one entry level wrapping the children.
        let pad = "\t".repeat(indent);
        let root_port = if force_root_port || schema.text_child().is_some() {
            let key = self.by_abs[&Vec::<String>::new()];
            format!(" {attr}=\"{key}\"")
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "{pad}<entry name=\"{}\"{root_port} expanded=\"1\">",
            xml_escape(&schema.name)
        );
        walk(
            schema,
            &mut Vec::new(),
            attr,
            indent + 1,
            &self.by_abs,
            &mut out,
        );
        let _ = writeln!(out, "{pad}</entry>");
        out
    }

    /// Entry-tree XML for a json component, mirroring MapForce's
    /// normalized shape (and the importer's inverse): property entries
    /// carry `type="json-property"`, structural `object`/`array`/`item`
    /// entries carry the keys -- object/iteration keys on `object`, scalar
    /// keys on the type leaf.
    fn json_entries_xml(&self, schema: &SchemaNode, attr: &str, indent: usize) -> String {
        let mut out = String::new();
        if schema.repeating {
            let pad = "\t".repeat(indent);
            let _ = writeln!(out, "{pad}<entry name=\"array\" expanded=\"1\">");
            let _ = writeln!(
                out,
                "{pad}\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
            );
            self.json_value_xml(schema, &mut Vec::new(), attr, indent + 2, &mut out);
            let _ = writeln!(out, "{pad}\t</entry>");
            let _ = writeln!(out, "{pad}</entry>");
        } else {
            self.json_value_xml(schema, &mut Vec::new(), attr, indent, &mut out);
        }
        out
    }

    /// Renders the value shape of `node` (its own repetition is the
    /// caller's concern).
    fn json_value_xml(
        &self,
        node: &SchemaNode,
        path: &mut Vec<String>,
        attr: &str,
        indent: usize,
        out: &mut String,
    ) {
        let pad = "\t".repeat(indent);
        let key = self.by_abs[&*path];
        match &node.kind {
            SchemaKind::Scalar { ty } => {
                let _ = writeln!(
                    out,
                    "{pad}<entry name=\"{}\" {attr}=\"{key}\"/>",
                    json_type_name(*ty)
                );
            }
            SchemaKind::Group { children, .. } => {
                let _ = writeln!(
                    out,
                    "{pad}<entry name=\"object\" {attr}=\"{key}\" expanded=\"1\">"
                );
                for child in children {
                    let _ = writeln!(
                        out,
                        "{pad}\t<entry name=\"{}\" type=\"json-property\" expanded=\"1\">",
                        xml_escape(&child.name)
                    );
                    path.push(child.name.clone());
                    if child.repeating {
                        let _ = writeln!(out, "{pad}\t\t<entry name=\"array\" expanded=\"1\">");
                        let _ = writeln!(
                            out,
                            "{pad}\t\t\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
                        );
                        self.json_value_xml(child, path, attr, indent + 4, out);
                        let _ = writeln!(out, "{pad}\t\t\t</entry>");
                        let _ = writeln!(out, "{pad}\t\t</entry>");
                    } else {
                        self.json_value_xml(child, path, attr, indent + 2, out);
                    }
                    path.pop();
                    let _ = writeln!(out, "{pad}\t</entry>");
                }
                let _ = writeln!(out, "{pad}</entry>");
            }
        }
    }
}

fn valid_http_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !url.contains('#')
        && url.is_ascii()
        && !url
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn json_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
