use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD};
use mapping::{FormatOptions, TabularBoundaryKind};

mod csv;
mod database_relation;
pub(super) mod database_xml;
mod definition_parameter;
mod edi;
mod fixed_width;
mod flextext;
mod generic_xml;
mod http_get;
mod pdf;
mod protobuf;
mod shared;
mod wsdl;
mod xbrl;
mod xlsx;
mod xml_ports;

pub(super) use shared::{
    XmlSchemaReadError, entry_key_sets, is_default_output, parse_u32, read_xml_schema_file,
    resolve_xml_schema_reference,
};

use csv::select_block as select_csv_block;
pub(crate) use csv::{SingletonPosition as CsvSingletonPosition, split_singleton_port};
use generic_xml::{generic_entry_schema, merge_entries as merge_generic_xml_entries};
use xml_ports::{normalize_xml_text_ports, reconcile_explicit_text_entries};

pub(super) fn restore_connected_structural_ports(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
) {
    xml_ports::restore_connected_structural_ports(components, edge_from);
}

pub(super) fn read_xlsx_component(
    component: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    xlsx::read(component, warnings)
}

pub(super) fn read_fixed_width_component(
    component: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    fixed_width::read(component, warnings)
}

pub(super) fn read_flextext_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<SchemaComponent, String> {
    flextext::read(component, mfd_path)
}

pub(super) fn read_http_get_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    http_get::read(component, mfd_path, warnings)
}

pub(super) fn read_wsdl_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    wsdl::read(component, mfd_path, warnings)
}

pub(super) fn refine_wsdl_target_schemas(
    components: &mut [SchemaComponent],
    functions: &[super::function::FnComponent],
    edge_from: &BTreeMap<u32, u32>,
) {
    wsdl::refine_connected_targets(components, functions, edge_from);
}

pub(super) fn read_protobuf_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    protobuf::read(component, mfd_path, warnings)
}

pub(super) fn read_pdf_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    pdf::read(component, mfd_path, warnings)
}

pub(super) fn read_xbrl_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    xbrl::read(component, mfd_path, warnings)
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ComponentFormat {
    Xml,
    Json,
    Csv,
    Xlsx,
    Edi,
    Db,
    Protobuf,
    FlexText,
    Pdf,
    Xbrl,
}

impl ComponentFormat {
    pub(super) const fn is_xml_like(self) -> bool {
        matches!(self, Self::Xml | Self::Xbrl)
    }

    pub(super) const fn supports_cloned_target_branches(self) -> bool {
        matches!(self, Self::Xml | Self::Edi | Self::Db | Self::Xbrl)
    }
}

/// One schema (source or target) component's extracted facts.
pub(super) struct SchemaComponent {
    pub(super) name: String,
    pub(super) format: ComponentFormat,
    pub(super) schema: SchemaNode,
    pub(super) input_instance: Option<String>,
    pub(super) output_instance: Option<String>,
    pub(super) options: FormatOptions,
    pub(super) is_source: bool,
    pub(super) is_default_output: bool,
    pub(super) is_variable: bool,
    pub(super) is_pass_through: bool,
    /// Input key of a variable component's compute-when control entry.
    pub(super) compute_when_key: Option<u32>,
    /// Port key -> absolute entry path (segments below the schema root).
    pub(super) ports: BTreeMap<u32, Vec<String>>,
    /// Input port -> enclosing input ports, outermost first. This preserves
    /// cloned target-entry branch identity when several ports share one path.
    pub(super) input_ancestors: BTreeMap<u32, Vec<u32>>,
    pub(super) input_keys: BTreeSet<u32>,
    pub(super) output_keys: BTreeSet<u32>,
    pub(super) db_queries: Vec<super::db_query::DbQuery>,
    pub(super) db_xml_columns: BTreeMap<u32, database_xml::Column>,
    pub(super) dynamic_json: Option<super::dynamic_json::DynamicJsonTarget>,
}

impl SchemaComponent {
    pub(super) fn is_target(&self) -> bool {
        (!self.is_variable && !self.is_source) || self.is_pass_through
    }
}

const JSON_DYNAMIC_NAME_PORT: &str = "\u{1f}ferrule-json-dynamic-name";
const JSON_DYNAMIC_BOOL_PORT: &str = "\u{1f}ferrule-json-dynamic-bool";
const JSON_DYNAMIC_INT_PORT: &str = "\u{1f}ferrule-json-dynamic-int";
const JSON_DYNAMIC_FLOAT_PORT: &str = "\u{1f}ferrule-json-dynamic-float";
const JSON_DYNAMIC_STRING_PORT: &str = "\u{1f}ferrule-json-dynamic-string";
pub(super) const SOURCE_DOCUMENT_PATH_PORT: &str = "\u{1f}ferrule-source-document-path";
pub(super) const SOURCE_INPUT_DOCUMENT_PATH_PORT: &str = "\u{1f}ferrule-source-input-document-path";
pub(super) const TARGET_DOCUMENT_PATH_PORT: &str = "\u{1f}ferrule-target-document-path";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JsonDynamicPort {
    Name,
    Value(ScalarType),
}

pub(super) fn split_json_dynamic_port(path: &[String]) -> Option<(&[String], JsonDynamicPort)> {
    let (marker, owner) = path.split_last()?;
    let port = match marker.as_str() {
        JSON_DYNAMIC_NAME_PORT => JsonDynamicPort::Name,
        JSON_DYNAMIC_BOOL_PORT => JsonDynamicPort::Value(ScalarType::Bool),
        JSON_DYNAMIC_INT_PORT => JsonDynamicPort::Value(ScalarType::Int),
        JSON_DYNAMIC_FLOAT_PORT => JsonDynamicPort::Value(ScalarType::Float),
        JSON_DYNAMIC_STRING_PORT => JsonDynamicPort::Value(ScalarType::String),
        _ => return None,
    };
    Some((owner, port))
}

fn nested_file_instance(root: &roxmltree::Node<'_, '_>, role: &str) -> Option<String> {
    root.descendants()
        .find(|node| node.has_tag_name("file") && node.attribute("role") == Some(role))
        .and_then(|node| node.attribute("name"))
        .map(str::to_string)
}

/// Reads an xml schema component: entry tree, ports, and the schema itself
/// (from the referenced XSD when it resolves, else derived from entries).
pub(super) fn read_schema_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let document = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "document");
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Prefer the payload below the synthetic `document` entry wherever it
    // appears. Variable components can put compute-when or parent-context
    // entries before/around it instead of using the ordinary
    // FileInstance/document wrapper.
    let document_entry = root_el
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"));
    let mut entry = document_entry
        .and_then(|document| {
            document
                .children()
                .find(|node| node.has_tag_name("entry") && !is_document_decoration_entry(node))
        })
        .or_else(|| root_el.children().find(|node| node.has_tag_name("entry")))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    // The root entry's own port is a document-level connection.
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_entry_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
    );
    let input_ancestors = input_port_ancestors(&entry, &input_keys);
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // Schema: prefer the referenced XSD (types + repeating info), picking
    // the top-level element the design says the document uses -- an XSD
    // can declare several document roots ("{ns}Local" strips to "Local").
    let instance_root: Vec<String> = document
        .and_then(|d| d.attribute("instanceroot"))
        .map(instance_root_segments)
        .unwrap_or_default();
    let mut schema = document
        .and_then(|d| d.attribute("schema"))
        .and_then(|rel| {
            let schema_path = match resolve_xml_schema_reference(mfd_path, rel) {
                Ok(path) => path,
                Err(error) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({error}); \
                         falling back to the entry tree (no types, no repeating info)"
                    ));
                    return None;
                }
            };
            match read_xml_schema_file(&schema_path, instance_root.first().map(String::as_str)) {
                Ok(schema) => {
                    if instance_root.len() <= 1 {
                        Some(schema)
                    } else {
                        match schema_node_at(&schema, &instance_root[1..]).cloned() {
                            Some(nested) => Some(nested),
                            None => {
                                warnings.push(format!(
                                    "component `{name}`: instance root `{}` does not exist in \
                                     schema `{rel}`; falling back to the entry tree",
                                    instance_root.join("/")
                                ));
                                None
                            }
                        }
                    }
                }
                Err(XmlSchemaReadError::Xsd(
                    format_xml::XmlFormatError::SchemaMaterializationLimit { .. },
                )) if !is_source => None,
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree (no types, no repeating info)"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| entry_tree_schema(&entry));
    if let Some(rel) = document.and_then(|document| document.attribute("schema"))
        && let Ok(schema_path) = resolve_xml_schema_reference(mfd_path, rel)
        && !schema_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dtd"))
    {
        super::alternatives::merge_conditioned_xml_types(
            &entry,
            &mut schema,
            &schema_path,
            warnings,
        );
    }
    merge_generic_xml_entries(&entry, &mut schema);
    reconcile_explicit_text_entries(&entry, &mut schema);
    normalize_xml_text_ports(&schema, &mut ports);

    let input_instance = document
        .and_then(|document| document.attribute("inputinstance"))
        .map(str::to_string)
        .or_else(|| nested_file_instance(&root_el, "inputinstance"));
    let local_xml_file_set = input_instance
        .as_deref()
        .is_some_and(is_local_file_set_pattern);
    if local_xml_file_set
        && is_source
        && let Some(key) = root_el
            .descendants()
            .find(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("name") == Some("FileInstance")
                    && entry.attribute("outkey").is_some()
            })
            .and_then(|entry| parse_u32(entry.attribute("outkey")))
    {
        ports.insert(key, vec![SOURCE_DOCUMENT_PATH_PORT.to_string()]);
    }
    if is_source
        && let Some(key) = root_el
            .descendants()
            .find(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("name") == Some("FileInstance")
                    && entry.attribute("inpkey").is_some()
            })
            .and_then(|entry| parse_u32(entry.attribute("inpkey")))
    {
        ports.insert(key, vec![SOURCE_INPUT_DOCUMENT_PATH_PORT.to_string()]);
    }
    if !is_source
        && let Some(key) = root_el
            .descendants()
            .find(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("name") == Some("FileInstance")
                    && entry.attribute("inpkey").is_some()
            })
            .and_then(|entry| parse_u32(entry.attribute("inpkey")))
    {
        ports.insert(key, vec![TARGET_DOCUMENT_PATH_PORT.to_string()]);
    }
    let is_pass_through = component
        .children()
        .any(|node| node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1"));
    Some(SchemaComponent {
        name,
        format: ComponentFormat::Xml,
        schema,
        input_instance,
        output_instance: document
            .and_then(|d| d.attribute("outputinstance"))
            .map(str::to_string)
            .or_else(|| nested_file_instance(&root_el, "outputinstance")),
        options: FormatOptions {
            xml_document: true,
            local_xml_file_set,
            ..FormatOptions::default()
        },
        is_source,
        is_default_output: is_default_output(component),
        is_variable: data.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        }) || is_pass_through,
        is_pass_through,
        compute_when_key: root_el
            .descendants()
            .find(|node| {
                node.has_tag_name("entry") && node.attribute("name") == Some("compute-when")
            })
            .and_then(|entry| parse_u32(entry.attribute("inpkey"))),
        ports,
        input_ancestors,
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

fn is_local_file_set_pattern(path: &str) -> bool {
    !path.contains("://")
        && Path::new(path.replace('\\', "/").as_str())
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(['*', '?']))
}

fn instance_root_segments(root: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_namespace = false;
    for (index, character) in root.char_indices() {
        match character {
            '{' => in_namespace = true,
            '}' => in_namespace = false,
            '/' if !in_namespace => {
                segments.push(&root[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(&root[start..]);
    segments
        .into_iter()
        .filter_map(|segment| segment.rsplit('}').next())
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn note_skipped_library(skipped: &mut Vec<String>, label: &str) {
    if !skipped.iter().any(|l| l == label) {
        skipped.push(label.to_string());
    }
}

/// Records an entry's own port keys under `path`.
pub(super) fn record_entry_keys(
    entry: &roxmltree::Node,
    path: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    if let Some(key) = parse_u32(entry.attribute("outkey")) {
        *out_count += 1;
        ports.insert(key, path.to_vec());
    }
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        *in_count += 1;
        ports.insert(key, path.to_vec());
    }
}

/// Reads a JSON component and normalizes structural entry wrappers away.
pub(super) fn read_json_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let json_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "json");
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Strip the synthetic FileInstance/document levels down to the JSON
    // document root wrapper (an entry conventionally named `root`).
    let mut entry = root_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    let json_lines = json_el.is_some_and(|json| json.attribute("jsonlines") == Some("1"));
    let external_source = json_el.and_then(|json| {
        let metadata = json
            .children()
            .find(|node| node.has_tag_name("ferrule-external-source"))?;
        if metadata.attribute("version") != Some("1") {
            warnings.push(format!(
                "captured user-function JSON source `{name}` has an unsupported provenance metadata version; imported as an ordinary JSON component"
            ));
            return None;
        }
        let source = metadata
            .text()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "metadata is empty".to_string())
            .and_then(|encoded| {
                serde_json::from_str::<mapping::ExternalSourceOptions>(encoded)
                    .map_err(|error| error.to_string())
            });
        match source {
            Ok(source)
                if source.payload() == mapping::ExternalPayloadFormat::Json
                    && matches!(
                        source.origin(),
                        mapping::ExternalSourceOrigin::UserFunction { .. }
                    ) =>
            {
                Some(source)
            }
            Ok(_) => {
                warnings.push(format!(
                    "captured user-function JSON source `{name}` has provenance for a different boundary kind; imported as an ordinary JSON component"
                ));
                None
            }
            Err(error) => {
                warnings.push(format!(
                    "captured user-function JSON source `{name}` has invalid provenance metadata ({error}); imported as an ordinary JSON component"
                ));
                None
            }
        }
    });

    let dynamic_json = match super::dynamic_json::read_target(&entry) {
        Ok(target) => target,
        Err(reason) => {
            warnings.push(format!(
                "dynamic JSON target `{name}` is unsupported: {reason}"
            ));
            None
        }
    };
    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_json_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        warnings,
    );
    if let Some(target) = &dynamic_json {
        in_count += target.input_count();
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // Schema: prefer the referenced JSON Schema (types + repeating info).
    let mut schema = json_el
        .and_then(|j| j.attribute("schema"))
        .and_then(|rel| {
            let schema_path = mfd_path.parent().unwrap_or(Path::new(".")).join(rel);
            match format_json::json_schema::import(&schema_path) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| {
            if json_el.and_then(|j| j.attribute("schema")).is_none() {
                warnings.push(format!(
                    "component `{name}` has no schema reference; deriving the \
                     schema from the entry tree"
                ));
            }
            json_entry_value_schema(&name, &entry)
        });
    if is_source && let Err(reason) = attach_json_dynamic_source_schema(&mut schema, &ports) {
        warnings.push(format!(
            "dynamic JSON source `{name}` is unsupported: {reason}"
        ));
    }
    if let Some(target) = &dynamic_json
        && !target.attach_schema(&mut schema)
    {
        warnings.push(format!(
            "dynamic JSON target `{name}` is unsupported: open fields cannot be combined with JSON alternatives"
        ));
        return None;
    }

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Json,
        schema,
        input_instance: json_el
            .and_then(|j| j.attribute("inputinstance"))
            .map(str::to_string),
        output_instance: json_el
            .and_then(|j| j.attribute("outputinstance"))
            .map(str::to_string),
        options: FormatOptions {
            external_source,
            json_document: true,
            json_lines,
            ..FormatOptions::default()
        },
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json,
    })
}

/// Maps JSON property ports to schema paths; structural wrappers are transparent.
pub(super) fn collect_json_ports(
    entry: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
    warnings: &mut Vec<String>,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        match child.attribute("type") {
            Some("json-property") => {
                if record_json_dynamic_source_ports(&child, path, ports, out_count) {
                    continue;
                }
                if child.attribute("inpkey").is_some()
                    && child.children().any(|entry| {
                        entry.has_tag_name("entry")
                            && entry.attribute("type") == Some("json-propertyname")
                            && entry.attribute("inpkey").is_some()
                    })
                {
                    continue;
                }
                path.push(child.attribute("name").unwrap_or_default().to_string());
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
                path.pop();
            }
            Some("json-subtype") => {
                // A subtype is an alternative object view, not a path
                // segment. Its root and descendant ports address the same
                // property path represented by the merged schema projection.
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
            }
            Some("json-propertyname") => {
                if child.attribute("outkey").is_some()
                    || child.attribute("inpkey").is_some()
                    || child
                        .descendants()
                        .any(|d| d.attribute("outkey").is_some() || d.attribute("inpkey").is_some())
                {
                    warnings.push(format!(
                        "dynamic `{}` entries are not supported; connections \
                         under `{}` skipped",
                        child.attribute("type").unwrap_or_default(),
                        path.join("/")
                    ));
                }
            }
            _ => {
                // Ferrule carries explicit JSON null through the nullable
                // typed value rather than a second target port. When both
                // alternatives are connected, keep the typed port and discard
                // the redundant null port so one field receives one binding.
                if child.attribute("name") == Some("null")
                    && child.attribute("inpkey").is_some()
                    && entry.children().any(|sibling| {
                        sibling.has_tag_name("entry")
                            && sibling.attribute("name") != Some("null")
                            && sibling.attribute("inpkey").is_some()
                    })
                {
                    continue;
                }
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
            }
        }
    }
}

fn record_json_dynamic_source_ports(
    property: &roxmltree::Node<'_, '_>,
    owner: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
) -> bool {
    let children = property
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let Some(name) = children.iter().find(|node| {
        node.attribute("type") == Some("json-propertyname")
            && parse_u32(node.attribute("outkey")).is_some()
    }) else {
        return false;
    };
    let values = children
        .iter()
        .filter_map(|node| {
            let ty = match node.attribute("name")? {
                "boolean" => ScalarType::Bool,
                "integer" => ScalarType::Int,
                "number" => ScalarType::Float,
                "string" => ScalarType::String,
                _ => return None,
            };
            Some((parse_u32(node.attribute("outkey"))?, ty))
        })
        .collect::<Vec<_>>();
    let [(value_key, value_type)] = values.as_slice() else {
        return false;
    };
    let Some(name_key) = parse_u32(name.attribute("outkey")) else {
        return false;
    };
    let value_marker = match value_type {
        ScalarType::Bool => JSON_DYNAMIC_BOOL_PORT,
        ScalarType::Int => JSON_DYNAMIC_INT_PORT,
        ScalarType::Float => JSON_DYNAMIC_FLOAT_PORT,
        ScalarType::String => JSON_DYNAMIC_STRING_PORT,
    };
    let mut name_path = owner.to_vec();
    name_path.push(JSON_DYNAMIC_NAME_PORT.to_string());
    let mut value_path = owner.to_vec();
    value_path.push(value_marker.to_string());
    ports.insert(name_key, name_path);
    ports.insert(*value_key, value_path);
    *out_count += 2;
    true
}

fn attach_json_dynamic_source_schema(
    schema: &mut SchemaNode,
    ports: &BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    for path in ports.values() {
        let Some((owner_path, JsonDynamicPort::Value(value_type))) = split_json_dynamic_port(path)
        else {
            continue;
        };
        let owner = schema_node_at_path_mut(schema, owner_path).ok_or_else(|| {
            format!(
                "open object `{}` is absent from its schema",
                owner_path.join("/")
            )
        })?;
        if let Some(existing) = owner.dynamic_fields() {
            if existing.repeating
                || !matches!(existing.kind, SchemaKind::Scalar { ty } if ty == value_type)
            {
                return Err(format!(
                    "open object `{}` declares incompatible dynamic value shapes",
                    owner_path.join("/")
                ));
            }
        } else if !owner.set_dynamic_fields(Some(SchemaNode::scalar("*", value_type))) {
            return Err(format!(
                "open object `{}` cannot combine dynamic fields with schema alternatives",
                owner_path.join("/")
            ));
        }
    }
    Ok(())
}

fn schema_node_at_path_mut<'a>(
    schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    let mut current = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut current.kind else {
            return None;
        };
        current = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(current)
}

/// Fallback JSON schema straight from the entry tree: `json-property`
/// children become fields, an enclosing `array` marks the property
/// repeating, type-leaf names give scalar types.
pub(super) fn json_entry_value_schema(name: &str, entry: &roxmltree::Node) -> SchemaNode {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        if child.attribute("type").is_some() && child.attribute("type") != Some("json-item") {
            continue;
        }
        match child.attribute("name") {
            Some("object") => {
                let children = child
                    .children()
                    .filter(|n| {
                        n.is_element()
                            && n.tag_name().name() == "entry"
                            && n.attribute("type") == Some("json-property")
                    })
                    .map(|p| json_entry_value_schema(p.attribute("name").unwrap_or_default(), &p))
                    .collect();
                return SchemaNode::group(name, children);
            }
            Some("array") => {
                return json_entry_value_schema(name, &child).repeating();
            }
            Some("item") => {
                // An array's item wrapper: its child describes the value.
                return json_entry_value_schema(name, &child);
            }
            Some("number") => return SchemaNode::scalar(name, ir::ScalarType::Float),
            Some("integer") => return SchemaNode::scalar(name, ir::ScalarType::Int),
            Some("boolean") => return SchemaNode::scalar(name, ir::ScalarType::Bool),
            Some("string") | Some("null") => {
                return SchemaNode::scalar(name, ir::ScalarType::String);
            }
            _ => continue,
        }
    }
    SchemaNode::scalar(name, ir::ScalarType::String)
}

/// Reads a csv text component: flat schema and delimiter/header options
/// from the inline `<settings>`; the block entry's own port is the row
/// iteration (path `[]`), field entries map to `[field]`.
pub(super) fn read_csv_component(
    component: &roxmltree::Node,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let text_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "text")?;
    let settings = text_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "settings");
    let names_el = settings.and_then(|s| {
        s.children()
            .find(|n| n.is_element() && n.tag_name().name() == "names")
    });

    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;
    let configured_block = names_el.and_then(|names| names.attribute("block"));
    let block = select_csv_block(root_el, configured_block, &name, "csv", warnings)?;
    let singleton_rows = csv::singleton_rows(root_el, configured_block, block);

    let fields: Vec<SchemaNode> = names_el
        .map(|names| {
            names
                .children()
                .filter(|n| n.is_element() && n.tag_name().name().starts_with("field"))
                .map(|f| {
                    let field_name = f.attribute("name").unwrap_or_default();
                    let ty = match f.attribute("type") {
                        Some("number") | Some("decimal") | Some("double") | Some("float") => {
                            ir::ScalarType::Float
                        }
                        Some("integer") | Some("int") => ir::ScalarType::Int,
                        Some("boolean") => ir::ScalarType::Bool,
                        _ => ir::ScalarType::String,
                    };
                    SchemaNode::scalar(field_name, ty)
                })
                .collect()
        })
        .unwrap_or_default();
    if fields.is_empty() {
        warnings.push(format!(
            "csv component `{name}` declares no fields; skipped"
        ));
        return None;
    }
    let root_name = names_el
        .and_then(|n| n.attribute("root"))
        .filter(|r| !r.is_empty())
        .unwrap_or(&name);
    let schema = SchemaNode::group(root_name, fields);

    let mut options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        ..FormatOptions::default()
    };
    if let Some(settings) = settings {
        if let Some(separator) = settings.attribute("separator") {
            let mut chars = separator.chars();
            options.delimiter = chars.next();
            if chars.next().is_some() {
                warnings.push(format!(
                    "csv component `{name}`: multi-character separator \
                     `{separator}` truncated to its first character"
                ));
            }
        }
        options.has_header_row = Some(settings.attribute("firstrownames") == Some("true"));
        if let Some(quote) = settings.attribute("quote")
            && !quote.is_empty()
            && quote != "\""
        {
            warnings.push(format!(
                "csv component `{name}`: quote character `{quote}` is not \
                 supported (ferrule always quotes with `\"`)"
            ));
        }
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    record_entry_keys(&block, &[], &mut ports, &mut out_count, &mut in_count);
    for field_entry in block
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        let field_name = field_entry.attribute("name").unwrap_or_default();
        record_entry_keys(
            &field_entry,
            &[field_name.to_string()],
            &mut ports,
            &mut out_count,
            &mut in_count,
        );
    }
    for row in singleton_rows {
        let mut connected = false;
        for field_entry in row
            .entry
            .children()
            .filter(|node| node.has_tag_name("entry"))
        {
            let Some(key) = parse_u32(field_entry.attribute("inpkey")) else {
                continue;
            };
            connected = true;
            in_count += 1;
            ports.insert(
                key,
                csv::singleton_port_path(
                    row.position,
                    field_entry.attribute("name").unwrap_or_default(),
                ),
            );
        }
        if !connected {
            warnings.push(format!(
                "csv target component `{name}` contains an unconnected singleton row; that row was skipped"
            ));
        }
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;
    Some(SchemaComponent {
        name,
        format: ComponentFormat::Csv,
        schema,
        input_instance: text_el
            .attribute("inputinstance")
            .map(str::to_string)
            .or_else(|| nested_file_instance(&root_el, "inputinstance")),
        output_instance: text_el
            .attribute("outputinstance")
            .map(str::to_string)
            .or_else(|| nested_file_instance(&root_el, "outputinstance")),
        options,
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

/// Reads an EDI text component from the visible entry tree. MapForce's
/// external configuration files are not portable with the design, so the
/// fallback preserves connected paths while being explicit about the lost
/// types, qualifiers, and exact cardinalities.
pub(super) fn read_edi_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    edi::read(component, mfd_path, warnings, true)
}

/// Reads an internal structured UDF parameter as a schema declaration only.
/// These components are not independent runtime boundaries, so their EDI and
/// database configuration diagnostics belong to the caller's real component.
pub(super) fn read_definition_parameter_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    definition_parameter::read(component, mfd_path, warnings)
}

fn collect_entry_ports(
    entry: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        if is_document_decoration_entry(&child) {
            continue;
        }
        let raw_name = child.attribute("name").unwrap_or_default();
        let (name, _) = normalize_xml_entry_name(raw_name);
        if child.attribute("type") == Some("xml-type") && name != XML_TEXT_FIELD {
            // xml-type entries are transparent type wrappers: descend
            // without extending the path.
            collect_entry_ports(&child, path, ports, out_count, in_count);
            continue;
        }
        path.push(name.to_string());
        if let Some(key) = parse_u32(child.attribute("outkey")) {
            *out_count += 1;
            ports.insert(key, path.clone());
        }
        if let Some(key) = parse_u32(child.attribute("inpkey")) {
            *in_count += 1;
            ports.insert(key, path.clone());
        }
        collect_entry_ports(&child, path, ports, out_count, in_count);
        path.pop();
    }
}

fn input_port_ancestors(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
) -> BTreeMap<u32, Vec<u32>> {
    let mut result = BTreeMap::new();
    let mut ancestors = Vec::new();
    let mut next_branch = input_keys
        .last()
        .copied()
        .and_then(|key| key.checked_add(1))
        .unwrap_or_default();
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        result.insert(key, Vec::new());
        ancestors.push(key);
    }
    collect_input_port_ancestors(
        entry,
        input_keys,
        &mut next_branch,
        &mut ancestors,
        &mut result,
    );
    result
}

fn collect_input_port_ancestors(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
    next_branch: &mut u32,
    ancestors: &mut Vec<u32>,
    result: &mut BTreeMap<u32, Vec<u32>>,
) {
    let children = entry
        .children()
        .filter(|node| {
            node.is_element() && node.has_tag_name("entry") && !is_document_decoration_entry(node)
        })
        .collect::<Vec<_>>();
    let mut name_counts = BTreeMap::new();
    for child in &children {
        let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        *name_counts.entry(normalized).or_insert(0usize) += 1;
    }
    for child in children {
        let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        let duplicate_branch = name_counts.get(&normalized).copied().unwrap_or_default() > 1;
        let branch = duplicate_branch.then(|| take_branch_marker(input_keys, next_branch));
        if let Some(branch) = branch {
            ancestors.push(branch);
        }
        let key = parse_u32(child.attribute("inpkey"));
        if let Some(key) = key {
            result.insert(key, ancestors.clone());
            ancestors.push(key);
        }
        collect_input_port_ancestors(&child, input_keys, next_branch, ancestors, result);
        if key.is_some() {
            ancestors.pop();
        }
        if branch.is_some() {
            ancestors.pop();
        }
    }
}

fn is_document_decoration_entry(entry: &roxmltree::Node<'_, '_>) -> bool {
    matches!(
        entry.attribute("type"),
        Some(
            "comment-before"
                | "comment-after"
                | "processing-instruction-before"
                | "processing-instruction-after"
        )
    )
}

fn take_branch_marker(input_keys: &BTreeSet<u32>, next_branch: &mut u32) -> u32 {
    while input_keys.contains(next_branch) {
        *next_branch = next_branch.wrapping_add(1);
    }
    let marker = *next_branch;
    *next_branch = next_branch.wrapping_add(1);
    marker
}

/// Fallback schema straight from the entry tree: groups where there are
/// children, string scalars at the leaves (attribute entries flagged as
/// such), no repeating flags.
fn entry_tree_schema(entry: &roxmltree::Node) -> SchemaNode {
    let raw_name = entry.attribute("name").unwrap_or("root");
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if name == XML_ELEMENTS_FIELD {
        return generic_entry_schema(entry);
    }
    if legacy_attribute || entry.attribute("type") == Some("attribute") {
        return SchemaNode::scalar(name, ir::ScalarType::String).attribute();
    }
    let entry_children = entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .collect::<Vec<_>>();
    let simple_content = (entry.attribute("inpkey").is_some()
        || entry.attribute("outkey").is_some())
        && !entry_children.is_empty()
        && entry_children.iter().all(|child| {
            let (_, legacy_attribute) =
                normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
            legacy_attribute || child.attribute("type") == Some("attribute")
        });
    let mut children = entry_children
        .iter()
        .map(|child| entry_tree_schema(child))
        .collect::<Vec<_>>();
    if simple_content {
        children.push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    }
    if children.is_empty() {
        SchemaNode::scalar(name, ir::ScalarType::String)
    } else {
        SchemaNode::group(name, children)
    }
}

/// Older `.mfd` entry trees prefix XML names with a decimal namespace-slot
/// index and encode attributes with a leading `@` (`0:Person`, `12:@type`).
/// Real QName prefixes are left intact because only all-decimal prefixes are
/// an entry-tree index.
pub(super) fn normalize_xml_entry_name(name: &str) -> (&str, bool) {
    let name = match name.split_once(':') {
        Some((prefix, local))
            if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            local
        }
        _ => name,
    };
    match name.strip_prefix('@') {
        Some(attribute) => (attribute, true),
        None => (name, false),
    }
}

/// Reads a database schema component. A lone flat table preserves the
/// historical flat-row shape: its own port maps to `[]` and its columns map
/// below that. Relational components retain each repeating table level;
/// several top-level tables live below a non-repeating `database` root and
/// nested relationship names keep MapForce's `PhysicalTable|JoinColumn`
/// convention understood by `format_db::read_instance`.
pub(super) fn read_db_component(
    component: &roxmltree::Node,
    mapping_el: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    match super::db_query::read_embedded_catalog(component, mapping_el, mfd_path) {
        Ok(Some(catalog)) => return Some(catalog),
        Ok(None) => {}
        Err(reason) => warnings.push(format!(
            "database component `{name}` has an unsupported inline query: {reason}; parent table fields were imported"
        )),
    }
    if component.attribute("kind") == Some("28") {
        return match super::db_query::read_component(component, mapping_el, mfd_path) {
            Ok(query) => Some(query),
            Err(reason) => {
                warnings.push(format!(
                    "skipped database query component `{name}`: {reason}"
                ));
                None
            }
        };
    }
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Descend through the synthetic FileInstance/document levels to the
    // level whose entries are the tables.
    let mut container = root_el;
    loop {
        let mut entries = container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "entry");
        let (first, second) = (entries.next(), entries.next());
        match (first, second) {
            (Some(entry), None)
                if matches!(
                    entry.attribute("name"),
                    Some("FileInstance") | Some("document")
                ) =>
            {
                container = entry;
            }
            _ => break,
        }
    }
    let entries: Vec<roxmltree::Node> = container
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .collect();
    let all_tables: Vec<roxmltree::Node> = entries
        .iter()
        .copied()
        .filter(|entry| entry.attribute("type") == Some("table"))
        .collect();
    if all_tables.is_empty() {
        warnings.push(format!(
            "skipped database component `{name}`: it contains no table entries"
        ));
        return None;
    }
    let connected_tables = all_tables
        .iter()
        .copied()
        .filter(|table| {
            let (inputs, outputs) = entry_key_sets(table);
            !inputs.is_empty() || !outputs.is_empty()
        })
        .collect::<Vec<_>>();
    // Database designers retain selected but disconnected tables in the
    // entry tree. Once any connected branch exists, those empty branches
    // have no mapping semantics and must not become invalid runtime tables.
    let tables = if connected_tables.is_empty() {
        all_tables
    } else {
        connected_tables
    };
    let single_plain_table = tables.len() == 1
        && !tables[0]
            .children()
            .any(|n| n.attribute("type") == Some("table"));
    let canonical_database_wrapper = tables.len() == 1
        && !single_plain_table
        && container.attribute("ferrule-database-wrapper") == Some("1");
    let db_xml_columns = database_xml::collect(
        &tables,
        tables.len() > 1 || canonical_database_wrapper,
        mfd_path,
        &name,
        warnings,
    );
    if container.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry
                .attribute("type")
                .is_some_and(|entry_type| !matches!(entry_type, "table" | "doc-xml"))
    }) {
        warnings.push(format!(
            "database component `{name}` contains non-table database entries; those entries were skipped"
        ));
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    for table in &tables {
        let mut path = if single_plain_table || tables.len() == 1 && !canonical_database_wrapper {
            Vec::new()
        } else {
            vec![table.attribute("name").unwrap_or_default().to_string()]
        };
        collect_db_ports(table, &mut path, &mut ports, &mut out_count, &mut in_count);
    }
    for (&input, column) in &db_xml_columns {
        ports.insert(input, column.path.clone());
        in_count += 1;
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    let input_ancestors = input_port_ancestors(&root_el, &input_keys);
    // The connection string lives in the mapping's datasource registry,
    // linked from the component by name.
    let connection_node = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "database")
        .and_then(|db| db.attribute("ref"))
        .and_then(|r| {
            mapping_el
                .descendants()
                .find(|n| n.has_tag_name("database_connection") && n.attribute("name") == Some(r))
        });
    let connection = connection_node
        .as_ref()
        .and_then(|c| c.attribute("ConnectionString"))
        .map(str::to_string);
    if connection.is_none() {
        warnings.push(format!(
            "database component `{name}` has no resolvable connection; the \
             project needs an instance path filled in"
        ));
    }

    let embedded_types = embedded_db_column_types(&tables);

    for generation in root_el.descendants().filter(|entry| {
        entry.has_tag_name("entry") && entry.attribute("valuekeygeneration").is_some()
    }) {
        if generation.attribute("valuekeygeneration") != Some("maxnumber") {
            warnings.push(format!(
                "component `{name}`: database column `{}` uses unsupported value generation `{}`; a mapped value is required",
                generation.attribute("name").unwrap_or_default(),
                generation.attribute("valuekeygeneration").unwrap_or_default()
            ));
        }
    }

    let db_path = connection.as_deref().and_then(|conn| {
        let path = mfd_path.parent().unwrap_or(Path::new(".")).join(conn);
        if path.exists() {
            Some(path)
        } else {
            if embedded_types.is_none() {
                warnings.push(format!(
                    "component `{name}`: database `{conn}` not found next to the \
                     design; falling back to untyped columns"
                ));
            }
            None
        }
    });

    // Keep the exact existing flat-table behavior, including exposing every
    // introspected column. Relational entry trees instead select their own
    // columns and use introspection only to recover each selected leaf type.
    let mut schema = if single_plain_table {
        let table = tables[0];
        let table_name = table.attribute("name").unwrap_or_default();
        let introspected =
            db_path
                .as_deref()
                .and_then(|path| match format_db::introspect(path, table_name) {
                    Ok(schema) => Some(schema),
                    Err(error) => {
                        warnings.push(format!(
                            "component `{name}`: could not introspect `{}` ({error}); \
                         falling back to untyped columns",
                            connection.as_deref().unwrap_or_default()
                        ));
                        None
                    }
                });
        if let Some(mut schema) = introspected {
            apply_db_value_generation(&table, &mut schema);
            schema
        } else {
            let empty = BTreeMap::new();
            db_table_schema(&table, embedded_types.as_ref().unwrap_or(&empty))
        }
    } else {
        let mut introspected_types = BTreeMap::new();
        let mut introspected_generation = BTreeMap::new();
        if let Some(path) = db_path.as_deref() {
            collect_db_column_types(
                path,
                &tables,
                &name,
                warnings,
                &mut introspected_types,
                &mut introspected_generation,
            );
        }
        let types = if db_path.is_some() {
            &introspected_types
        } else {
            embedded_types.as_ref().unwrap_or(&introspected_types)
        };
        let mut schema = if tables.len() == 1 && !canonical_database_wrapper {
            db_table_schema(&tables[0], types)
        } else {
            SchemaNode::group(
                "database",
                tables
                    .iter()
                    .map(|table| db_table_schema(table, types))
                    .collect(),
            )
        };
        if db_path.is_some() {
            apply_introspected_value_generation(&tables, &mut schema, &introspected_generation);
        }
        schema
    };
    database_relation::apply(connection_node.as_ref(), &mut schema, &name, warnings);
    if !single_plain_table
        && let Some(path) = db_path.as_deref()
        && let Err(error) = format_db::validate_relational_schema(path, &schema)
    {
        warnings.push(format!(
            "component `{name}`: relational schema does not match SQLite foreign-key metadata \
             ({error}); execution is disabled until the relationship is corrected"
        ));
    }

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Db,
        schema,
        input_instance: connection.clone(),
        output_instance: connection,
        options: FormatOptions::default(),
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors,
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        db_xml_columns,
        dynamic_json: None,
    })
}

/// Reads ferrule's canonical self-describing database entry metadata. The
/// metadata is trusted only when every selected scalar entry has a supported
/// type and repeated declarations agree, so ordinary database designs still
/// require live introspection.
fn embedded_db_column_types(
    tables: &[roxmltree::Node<'_, '_>],
) -> Option<BTreeMap<String, BTreeMap<String, ScalarType>>> {
    fn collect(
        table: &roxmltree::Node<'_, '_>,
        types: &mut BTreeMap<String, BTreeMap<String, ScalarType>>,
        leaf_count: &mut usize,
    ) -> Option<()> {
        let name = table.attribute("name")?;
        let physical_table = name.split_once('|').map_or(name, |(table, _)| table);
        if physical_table.is_empty() {
            return None;
        }
        for entry in table.children().filter(|node| node.has_tag_name("entry")) {
            match entry.attribute("type") {
                Some("table") => collect(&entry, types, leaf_count)?,
                None => {
                    let column = entry.attribute("name")?;
                    if column.is_empty() {
                        return None;
                    }
                    let ty = match entry.attribute("datatype")? {
                        "string" => ScalarType::String,
                        "integer" | "int" | "long" => ScalarType::Int,
                        "decimal" | "double" | "float" | "number" => ScalarType::Float,
                        "boolean" => ScalarType::Bool,
                        _ => return None,
                    };
                    let columns = types.entry(physical_table.to_string()).or_default();
                    if let Some(existing) = columns
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(column))
                        .map(|(_, ty)| *ty)
                        && existing != ty
                    {
                        return None;
                    }
                    columns.insert(column.to_string(), ty);
                    *leaf_count += 1;
                }
                Some(_) => return None,
            }
        }
        Some(())
    }

    let mut types = BTreeMap::new();
    let mut leaf_count = 0;
    for table in tables {
        collect(table, &mut types, &mut leaf_count)?;
    }
    (leaf_count > 0).then_some(types)
}

fn collect_db_ports(
    table: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    record_entry_keys(table, path, ports, out_count, in_count);
    for child in table
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
    {
        path.push(child.attribute("name").unwrap_or_default().to_string());
        match child.attribute("type") {
            Some("table") => collect_db_ports(&child, path, ports, out_count, in_count),
            None => record_entry_keys(&child, path, ports, out_count, in_count),
            Some(_) => {}
        }
        path.pop();
    }
}

fn collect_db_column_types(
    db_path: &Path,
    tables: &[roxmltree::Node<'_, '_>],
    component_name: &str,
    warnings: &mut Vec<String>,
    types: &mut BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
    generated: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for entry in tables {
        let physical_table = entry
            .attribute("name")
            .unwrap_or_default()
            .split_once('|')
            .map_or_else(
                || entry.attribute("name").unwrap_or_default(),
                |(table, _)| table,
            );
        if !types.contains_key(physical_table) {
            match format_db::introspect(db_path, physical_table) {
                Ok(schema) => {
                    let mut generated_columns = BTreeSet::new();
                    let column_types = match schema.kind {
                        SchemaKind::Group { children, .. } => children
                            .into_iter()
                            .filter_map(|column| match column.kind {
                                SchemaKind::Scalar { ty } => {
                                    if column.value_generation.is_some() {
                                        generated_columns.insert(column.name.clone());
                                    }
                                    Some((column.name, ty))
                                }
                                SchemaKind::Group { .. } => None,
                            })
                            .collect(),
                        SchemaKind::Scalar { .. } => BTreeMap::new(),
                    };
                    types.insert(physical_table.to_string(), column_types);
                    generated.insert(physical_table.to_string(), generated_columns);
                }
                Err(error) => {
                    warnings.push(format!(
                        "component `{component_name}`: could not introspect table \
                         `{physical_table}` ({error}); its columns are untyped"
                    ));
                    types.insert(physical_table.to_string(), BTreeMap::new());
                }
            }
        }
        let nested = entry
            .children()
            .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("table"))
            .collect::<Vec<_>>();
        collect_db_column_types(db_path, &nested, component_name, warnings, types, generated);
    }
}

fn apply_introspected_value_generation(
    table_entries: &[roxmltree::Node<'_, '_>],
    schema: &mut SchemaNode,
    generated: &BTreeMap<String, BTreeSet<String>>,
) {
    if table_entries.len() == 1 && schema.repeating {
        apply_table_generation(&table_entries[0], schema, generated);
        return;
    }
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for table in table_entries {
        let Some(name) = table.attribute("name") else {
            continue;
        };
        if let Some(child) = children.iter_mut().find(|child| child.name == name) {
            apply_table_generation(table, child, generated);
        }
    }
}

fn apply_table_generation(
    table: &roxmltree::Node<'_, '_>,
    schema: &mut SchemaNode,
    generated: &BTreeMap<String, BTreeSet<String>>,
) {
    let physical_table = table
        .attribute("name")
        .unwrap_or_default()
        .split_once('|')
        .map_or_else(
            || table.attribute("name").unwrap_or_default(),
            |(table, _)| table,
        );
    let generated_columns = generated.iter().find_map(|(table, columns)| {
        table
            .eq_ignore_ascii_case(physical_table)
            .then_some(columns)
    });
    let entries = table
        .children()
        .filter(|entry| entry.is_element() && entry.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for child in children {
        if matches!(child.kind, SchemaKind::Scalar { .. }) {
            let explicitly_exposed = entries.iter().any(|entry| {
                entry.attribute("type") != Some("table")
                    && entry
                        .attribute("name")
                        .is_some_and(|name| name.eq_ignore_ascii_case(&child.name))
            });
            if !explicitly_exposed
                && generated_columns.is_some_and(|columns| {
                    columns
                        .iter()
                        .any(|column| column.eq_ignore_ascii_case(&child.name))
                })
            {
                child.value_generation = Some(ir::ValueGeneration::MaxNumber);
            }
            continue;
        }
        let Some(entry) = entries.iter().find(|entry| {
            entry.attribute("type") == Some("table")
                && entry.attribute("name") == Some(child.name.as_str())
        }) else {
            continue;
        };
        apply_table_generation(entry, child, generated);
    }
}

fn db_table_schema(
    table: &roxmltree::Node,
    types: &BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
) -> SchemaNode {
    db_table_schema_with_complete_row(table, types, false)
}

fn apply_db_value_generation(table: &roxmltree::Node<'_, '_>, schema: &mut SchemaNode) {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    for entry in table
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("entry"))
    {
        let Some(name) = entry.attribute("name") else {
            continue;
        };
        let Some(child) = children.iter_mut().find(|child| child.name == name) else {
            continue;
        };
        if entry.attribute("type") == Some("table") {
            apply_db_value_generation(&entry, child);
        } else if entry.attribute("valuekeygeneration") == Some("maxnumber") {
            child.value_generation = Some(ir::ValueGeneration::MaxNumber);
        } else {
            // Live SQLite introspection marks implicit rowid primary keys as
            // generated. An explicitly exposed MFD column remains caller-
            // supplied unless the design itself requests maxnumber.
            child.value_generation = None;
        }
    }
}

fn db_table_schema_with_complete_row(
    table: &roxmltree::Node,
    types: &BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
    parent_is_complete: bool,
) -> SchemaNode {
    let name = table.attribute("name").unwrap_or_default();
    let physical_table = name.split_once('|').map_or(name, |(table, _)| table);
    let columns = types.get(physical_table);
    let is_complete = parent_is_complete
        || table.attribute("inpkey").is_some()
        || table.attribute("outkey").is_some();
    let mut children = table
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
        .filter_map(|entry| match entry.attribute("type") {
            Some("table") => Some(db_table_schema_with_complete_row(
                &entry,
                types,
                is_complete,
            )),
            None => {
                let column = entry.attribute("name").unwrap_or_default();
                let ty = columns
                    .and_then(|columns| {
                        columns
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(column))
                            .map(|(_, ty)| *ty)
                    })
                    .unwrap_or(ir::ScalarType::String);
                let mut schema = SchemaNode::scalar(column, ty);
                if entry.attribute("valuekeygeneration") == Some("maxnumber") {
                    schema.value_generation = Some(ir::ValueGeneration::MaxNumber);
                }
                Some(schema)
            }
            Some(_) => None,
        })
        .collect::<Vec<_>>();
    if is_complete {
        for (column, ty) in columns.into_iter().flatten() {
            if !children
                .iter()
                .any(|child| child.name.eq_ignore_ascii_case(column))
            {
                children.push(SchemaNode::scalar(column, *ty));
            }
        }
    }
    SchemaNode::group(name, children).repeating()
}

pub(super) fn schema_node_at<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        if let Some(anchor) = &node.recursive_ref {
            node = find_concrete_schema_group(schema, anchor)?;
        }
        node = node.child(segment)?;
    }
    Some(node)
}

pub(super) fn schema_node_at_resolved<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let node = schema_node_at(schema, path)?;
    match &node.recursive_ref {
        Some(anchor) => find_concrete_schema_group(schema, anchor),
        None => Some(node),
    }
}

fn find_concrete_schema_group<'a>(schema: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if schema.recursive_ref.is_none()
        && schema.name == anchor
        && matches!(schema.kind, SchemaKind::Group { .. })
    {
        return Some(schema);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

pub(super) fn collect_matching_scalar_paths(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. }) => paths.push(path.clone()),
        (SchemaKind::Group { .. }, SchemaKind::Group { children, .. }) => {
            for target_child in children {
                if target_child.repeating {
                    continue;
                }
                let Some(source_child) = source.child(&target_child.name) else {
                    continue;
                };
                path.push(target_child.name.clone());
                collect_matching_scalar_paths(source_child, target_child, path, paths);
                path.pop();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
