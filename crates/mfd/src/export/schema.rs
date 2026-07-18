//! Schema component and port-tree rendering for MFD export.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, TabularBoundaryKind};

use crate::MfdError;

use super::concatenation::TargetBranches;
use super::flextext;

const XLSX_MAX_ROW: u32 = 1_048_576;
const XLSX_MAX_COLUMN: u32 = 16_384;

/// Which MapForce component family a mapping side exports as.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum SideFormat {
    Xbrl,
    Edi,
    Pdf,
    Xml,
    Json,
    Csv,
    FixedWidth,
    FlexText,
    Xlsx,
    Db,
}

pub(super) fn side_format(instance_path: &Option<String>, options: &FormatOptions) -> SideFormat {
    if options.xbrl.is_some() {
        return SideFormat::Xbrl;
    }
    if options.edi_kind.is_some() {
        return SideFormat::Edi;
    }
    if options.pdf.is_some() {
        return SideFormat::Pdf;
    }
    if options.flextext.is_some() {
        return SideFormat::FlexText;
    }
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
        _ if options.json_document || options.json_lines => SideFormat::Json,
        _ if options.xml_document => SideFormat::Xml,
        _ if options.tabular_kind == Some(TabularBoundaryKind::Csv) => SideFormat::Csv,
        _ if options.tabular_kind == Some(TabularBoundaryKind::Xlsx) => SideFormat::Xlsx,
        _ if options.delimiter.is_some() || options.has_header_row.is_some() => SideFormat::Csv,
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
    pub(super) siblings: Vec<GeneratedSibling>,
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
    source_root_input: bool,
    target_branches: Option<&TargetBranches>,
    component_name: &str,
    component_uid: u32,
    sibling_suffix: &str,
    default_output: bool,
    used_ports: &BTreeSet<u32>,
    file_instance_port: Option<u32>,
) -> Result<RenderedSchemaComponent, MfdError> {
    if options.protobuf.is_some() {
        return super::protobuf::render(super::protobuf::RenderArgs {
            schema,
            ports,
            side,
            instance_path,
            options,
            mfd_path,
            target_branches,
            component_name,
            component_uid,
            sibling_suffix,
            default_output,
        });
    }
    let stem = mfd_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mapping");
    let dir = mfd_path.parent().unwrap_or(Path::new("."));
    let file_instance_output = file_instance_port
        .map(|key| format!(" {}=\"{key}\"", side.port_attr()))
        .unwrap_or_default();
    let (side_name, header, view) = match side {
        Side::Source => ("source", "", "<view rbx=\"300\" rby=\"400\"/>"),
        Side::Target => (
            "target",
            if default_output {
                "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
            } else {
                ""
            },
            "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
        ),
    };
    let uid = component_uid;
    let attr = side.port_attr();
    let instance = instance_path
        .map(|p| format!(" {}=\"{}\"", side.instance_attr(), xml_escape(p)))
        .unwrap_or_default();

    let mut out = String::new();
    let mut sibling = None;
    match format {
        SideFormat::Pdf => {
            return super::pdf::render(super::pdf::RenderArgs {
                schema,
                ports,
                side,
                instance_path,
                options,
                mfd_path,
                component_name,
                component_uid,
                sibling_suffix,
                force_root_port,
            });
        }
        SideFormat::Edi => {
            return super::edi::render(super::edi::RenderArgs {
                schema,
                ports,
                side,
                instance_path,
                options,
                component_name,
                component_uid,
                force_root_port,
                default_output,
            });
        }
        SideFormat::Xbrl => {
            return super::xbrl::render(super::xbrl::RenderArgs {
                schema,
                ports,
                side,
                instance_path,
                options,
                mfd_path,
                target_branches,
                component_name,
                component_uid,
                default_output,
                used_ports,
            });
        }
        SideFormat::Xml => {
            let schema_file = format!("{stem}-{sibling_suffix}.xsd");
            let namespace = format_xml::xsd::export_namespace(schema)?;
            let instance_root = format!(
                "{{{}}}{}",
                namespace.as_deref().unwrap_or_default(),
                schema.name
            );
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
                    xml_escape(&instance_root),
                    ports.entries_xml(schema, attr, 10, true, None, target_branches),
                    xml_escape(url),
                    http.timeout_seconds().get(),
                );
                return Ok(RenderedSchemaComponent {
                    xml: out,
                    siblings: sibling.into_iter().collect(),
                });
            }
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"xml\" uid=\"{uid}\" kind=\"14\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"FileInstance\"{file_instance_output} expanded=\"1\">\n\
                 \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 {}\
                 \t\t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<document schema=\"{}\" instanceroot=\"{}\"{instance}/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(component_name),
                ports.entries_xml(
                    schema,
                    attr,
                    9,
                    force_root_port,
                    source_root_input.then_some("inpkey"),
                    target_branches,
                ),
                xml_escape(&schema_file),
                xml_escape(&instance_root),
            );
        }
        SideFormat::Json => {
            let schema_file = format!("{stem}-{sibling_suffix}.schema.json");
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
                xml_escape(component_name),
                ports.json_entries_xml(schema, attr, 10),
                xml_escape(&schema_file),
                json_lines = if json_lines { " jsonlines=\"1\"" } else { "" },
            );
        }
        SideFormat::Db => {
            let layout = db_layout(schema).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} side maps to a database table but its schema \
                     is not a canonical relational table tree"
                ))
            })?;
            let datasource = db_datasource_name(instance_path);
            let table_entries = db_entries_xml(&layout, ports, attr, target_branches)?;
            let selections = db_selections_xml(&layout);
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{0}\" library=\"db\" uid=\"{uid}\" kind=\"15\">\n\
                 \t\t\t\t\t{header}{view}\n\
                 \t\t\t\t\t<data>\n\
                 \t\t\t\t\t\t<root>\n\
                 \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
                 \t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
                 {table_entries}\
                 \t\t\t\t\t\t\t</entry>\n\
                 \t\t\t\t\t\t</root>\n\
                 \t\t\t\t\t\t<database ref=\"{1}\">\n\
                 \t\t\t\t\t\t\t<data><selections>\n\
                 {selections}\
                 \t\t\t\t\t\t\t</selections></data>\n\
                 \t\t\t\t\t\t</database>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(component_name),
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
            let mut row_entries = String::new();
            let mut field_decls = String::new();
            for (i, (name, ty)) in fields.iter().enumerate() {
                let _ = writeln!(
                    field_decls,
                    "\t\t\t\t\t\t\t\t<field{i} name=\"{}\" type=\"{}\"/>",
                    xml_escape(name),
                    csv_type_name(*ty)
                );
            }
            let row_count = target_branches
                .and_then(|branches| branches.count(&[]))
                .unwrap_or(1);
            for index in 0..row_count {
                let iterating = target_branches
                    .and_then(|branches| branches.count(&[]).map(|_| branches.iterates(&[], index)))
                    .unwrap_or(true);
                let block_port = if iterating {
                    let key = branch_key(
                        ports,
                        target_branches,
                        Some((&[], index)),
                        &[],
                        "CSV row block",
                    )?;
                    format!(" {attr}=\"{key}\"")
                } else {
                    String::new()
                };
                let clone = if target_branches.is_some() && (index > 0 || iterating) {
                    " clone=\"1\""
                } else {
                    ""
                };
                let _ = writeln!(
                    row_entries,
                    "\t\t\t\t\t\t\t\t\t<entry name=\"Rows\"{block_port} expanded=\"1\"{clone}>"
                );
                for (name, _) in &fields {
                    let path = [(*name).to_string()];
                    let key = branch_key(
                        ports,
                        target_branches,
                        Some((&[], index)),
                        &path,
                        "CSV field",
                    )?;
                    let _ = writeln!(
                        row_entries,
                        "\t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" {attr}=\"{key}\"/>",
                        xml_escape(name)
                    );
                }
                row_entries.push_str("\t\t\t\t\t\t\t\t\t</entry>\n");
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
                 {row_entries}\
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
                xml_escape(component_name),
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
                xml_escape(component_name),
                layout.record_delimiters(),
                xml_escape(&layout.fill_char().to_string()),
                layout.treat_empty_as_absent(),
                xml_escape(&schema.name),
            );
        }
        SideFormat::FlexText => {
            let layout = options.flextext.as_ref().ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "the {side_name} FlexText component has no embedded layout"
                ))
            })?;
            let config_file = format!("{stem}-{sibling_suffix}.mft");
            sibling = Some(GeneratedSibling {
                path: dir.join(&config_file),
                contents: flextext::render_config(layout, instance_path, side_name)?,
            });
            let flex_instance = instance_path
                .map(|path| {
                    format!(
                        " {}=\"{}\"",
                        side.instance_attr(),
                        flextext_instance_escape(path)
                    )
                })
                .unwrap_or_default();
            let _ = write!(
                out,
                "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{uid}\" kind=\"16\">\n\
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
                 \t\t\t\t\t\t<text type=\"txt\" config=\"{}\"{flex_instance} encoding=\"52\" byteorder=\"1\" byteordermark=\"{}\"/>\n\
                 \t\t\t\t\t</data>\n\
                 \t\t\t\t</component>\n",
                xml_escape(component_name),
                ports.entries_xml(schema, attr, 9, force_root_port, None, target_branches),
                xml_escape(&config_file),
                u8::from(layout.write_bom()),
            );
        }
        SideFormat::Xlsx => {
            if let Some(layout) = &options.xlsx_hierarchical {
                if side != Side::Target {
                    return Err(MfdError::Unsupported(format!(
                        "the {side_name} XLSX layout is hierarchical and target-only"
                    )));
                }
                let xml = super::xlsx::render_hierarchical(
                    super::xlsx::RenderArgs {
                        schema,
                        ports,
                        instance_path,
                        options,
                        component_name,
                        component_uid,
                    },
                    layout,
                    default_output,
                )?;
                return Ok(RenderedSchemaComponent {
                    xml,
                    siblings: Vec::new(),
                });
            }
            let retained_source_layout = options.xlsx_grid.is_some()
                || options.xlsx_composite.is_some()
                || !options.xlsx_rows.is_empty();
            if retained_source_layout && side != Side::Source {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} XLSX layout is source-only and cannot be exported as a target"
                )));
            }
            if let Some(xml) = super::xlsx::render(super::xlsx::RenderArgs {
                schema,
                ports,
                instance_path,
                options,
                component_name,
                component_uid,
            })? {
                return Ok(RenderedSchemaComponent {
                    xml,
                    siblings: Vec::new(),
                });
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
                xml_escape(component_name),
                xml_escape(sheet),
            );
        }
    }
    Ok(RenderedSchemaComponent {
        xml: out,
        siblings: sibling.into_iter().collect(),
    })
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

pub(super) enum DbLayout<'a> {
    Table(&'a SchemaNode),
    Database(&'a [SchemaNode]),
}

pub(super) fn db_layout(schema: &SchemaNode) -> Option<DbLayout<'_>> {
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
    } = &schema.kind
    else {
        return None;
    };
    if schema.recursive_ref.is_some() || !alternatives.is_empty() || dynamic.is_some() {
        return None;
    }
    let has_group = children
        .iter()
        .any(|child| matches!(child.kind, SchemaKind::Group { .. }));
    if schema.repeating || !has_group {
        return db_table_is_valid(schema, false).then_some(DbLayout::Table(schema));
    }
    if schema.name != "database" || children.is_empty() {
        return None;
    }
    children
        .iter()
        .all(|table| table.repeating && db_table_is_valid(table, false))
        .then_some(DbLayout::Database(children))
}

fn db_table_is_valid(table: &SchemaNode, nested: bool) -> bool {
    if table.recursive_ref.is_some()
        || nested
            && table
                .name
                .split_once('|')
                .is_none_or(|(name, column)| name.is_empty() || column.is_empty())
        || !nested && table.name.contains('|')
    {
        return false;
    }
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
    } = &table.kind
    else {
        return false;
    };
    alternatives.is_empty()
        && dynamic.is_none()
        && children.iter().all(|child| match child.kind {
            SchemaKind::Scalar { .. } => {
                !child.repeating && !child.attribute && !child.text && child.recursive_ref.is_none()
            }
            SchemaKind::Group { .. } => child.repeating && db_table_is_valid(child, true),
        })
}

fn db_entries_xml(
    layout: &DbLayout<'_>,
    ports: &PortTree,
    attr: &str,
    branches: Option<&TargetBranches>,
) -> Result<String, MfdError> {
    let mut output = String::new();
    match layout {
        DbLayout::Table(table) => {
            render_db_table(
                table,
                &mut Vec::new(),
                ports,
                attr,
                9,
                &mut output,
                branches,
                None,
            )?;
        }
        DbLayout::Database(tables) => {
            let mut occurrences = BTreeMap::<&str, usize>::new();
            for table in *tables {
                let mut path = vec![table.name.clone()];
                let index = *occurrences.entry(&table.name).or_default();
                *occurrences.entry(&table.name).or_default() += 1;
                let branch_count = branches.and_then(|branches| branches.count(&path));
                if index > 0 && branch_count.is_none() {
                    return Err(MfdError::Unsupported(format!(
                        "database table `{}` is duplicated without a concatenated target scope",
                        table.name
                    )));
                }
                let branch_root = path.clone();
                render_db_table(
                    table,
                    &mut path,
                    ports,
                    attr,
                    9,
                    &mut output,
                    branches,
                    branch_count.map(|_| (branch_root.as_slice(), index)),
                )?;
            }
            for (name, rendered) in occurrences {
                let root = vec![name.to_string()];
                let Some(count) = branches.and_then(|branches| branches.count(&root)) else {
                    continue;
                };
                if rendered >= count {
                    continue;
                }
                let Some(table) = tables.iter().find(|table| table.name == name) else {
                    continue;
                };
                for index in rendered..count {
                    let mut path = root.clone();
                    render_db_table(
                        table,
                        &mut path,
                        ports,
                        attr,
                        9,
                        &mut output,
                        branches,
                        Some((&root, index)),
                    )?;
                }
            }
        }
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn render_db_table(
    table: &SchemaNode,
    path: &mut Vec<String>,
    ports: &PortTree,
    attr: &str,
    indent: usize,
    output: &mut String,
    branches: Option<&TargetBranches>,
    branch: Option<(&[String], usize)>,
) -> Result<(), MfdError> {
    let pad = "\t".repeat(indent);
    let key = branch_key(ports, branches, branch, path, "database table")?;
    let clone = if branch.is_some_and(|(_, index)| index > 0) {
        " clone=\"1\""
    } else {
        ""
    };
    let _ = writeln!(
        output,
        "{pad}<entry name=\"{}\" type=\"table\" {attr}=\"{key}\" expanded=\"1\"{clone}>",
        xml_escape(&table.name)
    );
    let SchemaKind::Group { children, .. } = &table.kind else {
        return Err(MfdError::Unsupported(
            "internal database table schema is not a group".to_string(),
        ));
    };
    for child in children {
        path.push(child.name.clone());
        match child.kind {
            SchemaKind::Scalar { ty } => {
                let key = if attr == "inpkey" && child.value_generation.is_some() {
                    String::new()
                } else {
                    format!(
                        " {attr}=\"{}\"",
                        branch_key(ports, branches, branch, path, "database column")?
                    )
                };
                let generation = child
                    .value_generation
                    .map(|generation| match generation {
                        ir::ValueGeneration::MaxNumber => " valuekeygeneration=\"maxnumber\"",
                    })
                    .unwrap_or_default();
                let _ = writeln!(
                    output,
                    "{pad}\t<entry name=\"{}\"{key}{generation} datatype=\"{}\"/>",
                    xml_escape(&child.name),
                    db_type_name(ty)
                );
            }
            SchemaKind::Group { .. } => {
                render_db_table(
                    child,
                    path,
                    ports,
                    attr,
                    indent + 1,
                    output,
                    branches,
                    branch,
                )?;
            }
        }
        path.pop();
    }
    let _ = writeln!(output, "{pad}</entry>");
    Ok(())
}

pub(super) const fn db_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn db_selections_xml(layout: &DbLayout<'_>) -> String {
    fn collect(table: &SchemaNode, names: &mut std::collections::BTreeSet<String>) {
        names.insert(
            table
                .name
                .split_once('|')
                .map_or(table.name.as_str(), |(name, _)| name)
                .to_string(),
        );
        if let SchemaKind::Group { children, .. } = &table.kind {
            for child in children {
                if matches!(child.kind, SchemaKind::Group { .. }) {
                    collect(child, names);
                }
            }
        }
    }
    let mut names = std::collections::BTreeSet::new();
    match layout {
        DbLayout::Table(table) => collect(table, &mut names),
        DbLayout::Database(tables) => {
            for table in *tables {
                collect(table, &mut names);
            }
        }
    }
    let mut output = String::new();
    for name in names {
        let _ = writeln!(
            output,
            "\t\t\t\t\t\t\t\t<selection><PathElement Name=\"main\" Kind=\"Database\"/><PathElement Name=\"{}\" Kind=\"Table\"/></selection>",
            xml_escape(&name)
        );
    }
    output
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
    by_alternative: BTreeMap<(Vec<String>, String), u32>,
}

// Recursive schemas are represented in the IR by finite references. Entry
// trees still need concrete descendant ports for connections, but must not
// turn a recursive declaration into unbounded export work.
const MAX_RECURSIVE_PORT_DEPTH: usize = 32;
const MAX_RECURSIVE_PORT_ELEMENTS: usize = 4_096;

pub(super) enum PortMatch {
    Missing,
    Unique(u32),
    Ambiguous,
}

pub(super) enum PortPairMatch {
    Missing,
    Unique(u32, u32),
    Ambiguous,
}

fn concrete_group_anchors(schema: &SchemaNode) -> BTreeMap<&str, Option<&SchemaNode>> {
    fn collect<'a>(node: &'a SchemaNode, anchors: &mut BTreeMap<&'a str, Option<&'a SchemaNode>>) {
        if node.recursive_ref.is_some() || !matches!(node.kind, SchemaKind::Group { .. }) {
            return;
        }
        anchors
            .entry(&node.name)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(node));
        if let SchemaKind::Group { children, .. } = &node.kind {
            for child in children {
                collect(child, anchors);
            }
        }
    }

    let mut anchors = BTreeMap::new();
    collect(schema, &mut anchors);
    anchors
}

impl PortTree {
    pub(super) fn build(schema: &SchemaNode, keys: &mut KeyAlloc) -> Self {
        Self::build_with_explicit_text(schema, keys, &BTreeSet::new())
    }

    pub(super) fn build_with_explicit_text(
        schema: &SchemaNode,
        keys: &mut KeyAlloc,
        explicit_text: &BTreeSet<Vec<String>>,
    ) -> Self {
        let mut by_abs = BTreeMap::new();
        let mut by_alternative = BTreeMap::new();
        let anchors = concrete_group_anchors(schema);
        let mut recursive_elements = 0;
        // The document root itself: rendered as a port only by row/array
        // shaped components (a csv block, a json root object).
        by_abs.insert(Vec::new(), keys.next());
        #[allow(clippy::too_many_arguments)]
        fn walk<'a>(
            node: &'a SchemaNode,
            path: &mut Vec<String>,
            keys: &mut KeyAlloc,
            by_abs: &mut BTreeMap<Vec<String>, u32>,
            by_alternative: &mut BTreeMap<(Vec<String>, String), u32>,
            explicit_text: &BTreeSet<Vec<String>>,
            anchors: &BTreeMap<&'a str, Option<&'a SchemaNode>>,
            recursive_depth: usize,
            recursive_elements: &mut usize,
        ) {
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    if recursive_depth > 0 {
                        if recursive_depth > MAX_RECURSIVE_PORT_DEPTH
                            || *recursive_elements >= MAX_RECURSIVE_PORT_ELEMENTS
                        {
                            return;
                        }
                        *recursive_elements += 1;
                    }
                    path.push(child.name.clone());
                    if child.text && !explicit_text.contains(path) {
                        let parent = &path[..path.len() - 1];
                        let key = by_abs[parent];
                        by_abs.insert(path.clone(), key);
                        path.pop();
                        continue;
                    }
                    by_abs.insert(path.clone(), keys.next());
                    for alternative in child.alternatives() {
                        by_alternative
                            .insert((path.clone(), alternative.name.clone()), keys.next());
                    }
                    match child.recursive_ref.as_deref() {
                        Some(anchor) if recursive_depth < MAX_RECURSIVE_PORT_DEPTH => {
                            if let Some(Some(anchor)) = anchors.get(anchor) {
                                walk(
                                    anchor,
                                    path,
                                    keys,
                                    by_abs,
                                    by_alternative,
                                    explicit_text,
                                    anchors,
                                    recursive_depth + 1,
                                    recursive_elements,
                                );
                            }
                        }
                        Some(_) => {}
                        None => walk(
                            child,
                            path,
                            keys,
                            by_abs,
                            by_alternative,
                            explicit_text,
                            anchors,
                            recursive_depth,
                            recursive_elements,
                        ),
                    }
                    path.pop();
                }
            }
        }
        walk(
            schema,
            &mut Vec::new(),
            keys,
            &mut by_abs,
            &mut by_alternative,
            explicit_text,
            &anchors,
            0,
            &mut recursive_elements,
        );
        Self {
            by_abs,
            by_alternative,
        }
    }

    pub(super) fn key_for_abs(&self, abs: &[String]) -> Option<u32> {
        self.by_abs.get(abs).copied()
    }

    pub(super) fn key_for_alternative(&self, abs: &[String], name: &str) -> Option<u32> {
        self.by_alternative
            .get(&(abs.to_vec(), name.to_string()))
            .copied()
    }

    pub(super) fn required_key_for_abs(&self, abs: &[String], kind: &str) -> Result<u32, MfdError> {
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

    /// Resolves two descendants below one uniquely matched collection. Matching
    /// the descendants independently could silently pair fields from different
    /// same-named collections.
    pub(super) fn match_collection_pair(
        &self,
        collection: &[String],
        first: &[String],
        second: &[String],
    ) -> PortPairMatch {
        let mut matches = self.by_abs.keys().filter_map(|candidate| {
            if !candidate.ends_with(collection) {
                return None;
            }
            let mut first_path = candidate.clone();
            first_path.extend(first.iter().cloned());
            let mut second_path = candidate.clone();
            second_path.extend(second.iter().cloned());
            Some((
                self.key_for_abs(&first_path)?,
                self.key_for_abs(&second_path)?,
            ))
        });
        let Some(first) = matches.next() else {
            return PortPairMatch::Missing;
        };
        if matches.next().is_some() {
            PortPairMatch::Ambiguous
        } else {
            PortPairMatch::Unique(first.0, first.1)
        }
    }

    /// Entry-tree XML for a schema with `attr` (outkey/inpkey) on every
    /// entry.
    pub(super) fn entries_xml(
        &self,
        schema: &SchemaNode,
        attr: &str,
        indent: usize,
        force_root_port: bool,
        root_attr: Option<&str>,
        target_branches: Option<&TargetBranches>,
    ) -> String {
        let mut out = String::new();
        let anchors = concrete_group_anchors(schema);
        // The recursive renderer carries explicit immutable allocation state so
        // branch-specific target ports cannot accidentally fall back to shared keys.
        #[allow(clippy::too_many_arguments)]
        fn walk<'a>(
            node: &'a SchemaNode,
            path: &mut Vec<String>,
            attr: &str,
            indent: usize,
            by_abs: &BTreeMap<Vec<String>, u32>,
            ports: &PortTree,
            target_branches: Option<&TargetBranches>,
            active_branch: Option<(&[String], usize)>,
            anchors: &BTreeMap<&'a str, Option<&'a SchemaNode>>,
            out: &mut String,
        ) {
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    let Some(&base_key) = by_abs.get(path.as_slice()) else {
                        path.pop();
                        continue;
                    };
                    if child.text
                        && by_abs.get(path.as_slice()) == by_abs.get(&path[..path.len() - 1])
                    {
                        path.pop();
                        continue;
                    }
                    let count = active_branch
                        .is_none()
                        .then(|| target_branches.and_then(|branches| branches.count(path)))
                        .flatten()
                        .unwrap_or(1);
                    for index in 0..count {
                        let branch_root = path.clone();
                        let branch = active_branch
                            .or_else(|| (count > 1).then_some((branch_root.as_slice(), index)));
                        let pad = "\t".repeat(indent);
                        let key = match branch {
                            Some((root, index)) => target_branches
                                .and_then(|branches| branches.key_for(ports, root, index, path))
                                .unwrap_or(base_key),
                            None => base_key,
                        };
                        let type_attr = if child.attribute {
                            " type=\"attribute\""
                        } else {
                            ""
                        };
                        let clone = if active_branch.is_none() && index > 0 {
                            " clone=\"1\""
                        } else {
                            ""
                        };
                        let _ = write!(
                            out,
                            "{pad}<entry name=\"{}\"{type_attr} {attr}=\"{key}\" expanded=\"1\"{clone}",
                            xml_escape(&child.name)
                        );
                        if matches!(child.kind, SchemaKind::Scalar { .. }) {
                            out.push_str("/>\n");
                        } else {
                            out.push_str(">\n");
                            if let Some(condition) = branch
                                .and_then(|(root, index)| target_branches?.condition(root, index))
                            {
                                append_xml_type_condition(out, indent + 1, condition);
                            }
                            let shape = child
                                .recursive_ref
                                .as_deref()
                                .and_then(|anchor| anchors.get(anchor).copied().flatten())
                                .unwrap_or(child);
                            walk(
                                shape,
                                path,
                                attr,
                                indent + 1,
                                by_abs,
                                ports,
                                target_branches,
                                branch,
                                anchors,
                                out,
                            );
                            let _ = writeln!(out, "{pad}</entry>");
                        }
                    }
                    if attr == "outkey" && target_branches.is_none() {
                        let pad = "\t".repeat(indent);
                        for alternative in child.alternatives() {
                            let Some(key) = ports.key_for_alternative(path, &alternative.name)
                            else {
                                continue;
                            };
                            let _ = writeln!(
                                out,
                                "{pad}<entry name=\"{}\" {attr}=\"{key}\" expanded=\"1\" clone=\"1\">",
                                xml_escape(&child.name)
                            );
                            append_xml_type_condition(out, indent + 1, &alternative.name);
                            let _ = writeln!(out, "{pad}</entry>");
                        }
                    }
                    path.pop();
                }
            }
        }
        // The document root itself is one entry level wrapping the children.
        let pad = "\t".repeat(indent);
        let root_port = if let Some(root_attr) = root_attr {
            let key = self.by_abs[&Vec::<String>::new()];
            format!(" {root_attr}=\"{key}\"")
        } else if force_root_port || schema.text_child().is_some() {
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
            self,
            target_branches,
            None,
            &anchors,
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
    pub(super) fn json_entries_xml(
        &self,
        schema: &SchemaNode,
        attr: &str,
        indent: usize,
    ) -> String {
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

fn branch_key(
    ports: &PortTree,
    branches: Option<&TargetBranches>,
    branch: Option<(&[String], usize)>,
    path: &[String],
    kind: &str,
) -> Result<u32, MfdError> {
    branch
        .and_then(|(root, index)| branches?.key_for(ports, root, index, path))
        .or_else(|| ports.key_for_abs(path))
        .ok_or_else(|| {
            let path = if path.is_empty() {
                "<root>".to_string()
            } else {
                path.join("/")
            };
            MfdError::Unsupported(format!("internal {kind} port `{path}` was not allocated"))
        })
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

fn flextext_instance_escape(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn json_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

fn append_xml_type_condition(output: &mut String, indent: usize, type_name: &str) {
    let pad = "\t".repeat(indent);
    let _ = writeln!(
        output,
        "{pad}<condition><expression><function name=\"equal\" library=\"core\"><expression><attribute ns=\"http://www.w3.org/2001/XMLSchema-instance\" name=\"type\"/></expression><expression><constant value=\"{}\" datatype=\"QName\"/></expression></function></expression></condition>",
        xml_escape(type_name)
    );
}

pub(super) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
