use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, TabularBoundaryKind};

use crate::resource::ResourceResolver;

use super::{db_query, dynamic_json, json_parser};

mod csv;
mod database;
mod database_relation;
pub(super) mod database_xml;
mod definition_parameter;
mod edi;
mod entry_ports;
mod fixed_width;
mod flextext;
mod generic_xml;
mod http_get;
mod json;
mod json_undeclared;
mod json_variable;
pub(super) mod occurrence_provenance;
mod pdf;
mod port_declarations;
mod protobuf;
mod shared;
mod wsdl;
mod xbrl;
mod xlsx;
mod xml_attribute_required;
mod xml_entry;
mod xml_ports;

pub(super) use database::read_db_component_in_package;
use database::{collect_db_ports, db_table_schema};
pub(super) use entry_ports::collect_entry_ports_with_declarations;
#[cfg(test)]
use entry_ports::input_branch_paths;
use entry_ports::{
    collect_entry_ports, input_port_ancestors, input_port_ancestry, input_port_ancestry_projected,
    is_document_decoration_entry,
};
#[cfg(test)]
pub(super) use json::read_json_component;
pub(super) use json::{
    collect_json_ports_with_declarations, json_entry_value_schema, read_json_component_in_package,
};
use xml_entry::typed_xml_entry_tree_schema;
pub(super) use xml_entry::{entry_tree_schema, normalize_xml_entry_name};

pub(super) use xml_attribute_required::validate_xml_attribute_required_metadata;
#[cfg(test)]
pub(super) use xml_ports::plain_entry_tree_leaf;

pub use flextext::{MAX_FLEXTEXT_CONFIGURATION_BYTES, import_flextext_configuration};

pub(super) use shared::{
    XmlSchemaReadError, XmlSchemaResource, entry_key_sets, is_default_output, parse_u32,
    read_xml_schema_file, read_xsd_metadata_text, resolve_xml_resource,
    resolve_xml_schema_reference,
};

pub(super) use port_declarations::PortDeclarationCensus;
use port_declarations::PortDeclarations;

pub(super) fn enrich_unresolved_edi_source_schemas(project: &mut mapping::Project) {
    edi::enrich_unresolved_source_schemas(project);
}

use csv::select_block as select_csv_block;
pub(crate) use csv::{SingletonPosition as CsvSingletonPosition, split_singleton_port};
use generic_xml::merge_entries as merge_generic_xml_entries;
use xml_ports::{normalize_xml_text_ports, reconcile_explicit_text_entries};

pub(super) fn validate_xml_optional_metadata(
    mapping: &roxmltree::Node<'_, '_>,
) -> Result<(), crate::MfdError> {
    for entry in mapping
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
    {
        if let Some(value) = entry.attribute("ferrule-xml-optional")
            && !matches!(value, "0" | "1")
        {
            return Err(crate::MfdError::InvalidXmlOptionalMetadata {
                entry: entry.attribute("name").unwrap_or_default().to_string(),
                value: value.to_string(),
            });
        }
    }
    Ok(())
}

fn resolve_resource_reference(
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    declared: &str,
    description: &str,
) -> Result<std::path::PathBuf, String> {
    match resources {
        Some(resources) => {
            resources.resolve_file_retaining_boundary_refusals(declared, description)
        }
        None => resolve_xml_schema_reference(mfd_path, declared),
    }
}

pub(super) fn restore_connected_structural_ports(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
) {
    xml_ports::restore_connected_structural_ports(components, edge_from);
}

pub(super) fn refine_copied_fallback_source_shapes(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    fallback_source_outputs: &BTreeSet<u32>,
    fallback_target_inputs: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    xml_ports::refine_copied_fallback_source_shapes(
        components,
        edge_from,
        copy_all_targets,
        fallback_source_outputs,
        fallback_target_inputs,
        warnings,
    );
}

pub(super) fn refine_copied_fallback_target_groups(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    fallback_source_outputs: &BTreeSet<u32>,
    fallback_target_inputs: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    xml_ports::refine_copied_fallback_target_groups(
        components,
        edge_from,
        copy_all_targets,
        fallback_source_outputs,
        fallback_target_inputs,
        warnings,
    );
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
    resources: &ResourceResolver,
) -> Result<SchemaComponent, String> {
    flextext::read(component, resources)
}

pub(super) fn read_http_get_component(
    component: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    http_get::read(component, resources, warnings)
}

pub(super) fn read_wsdl_component(
    component: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    wsdl::read(component, resources, warnings)
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
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    protobuf::read(component, resources, warnings)
}

pub(super) fn read_pdf_component(
    component: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    pdf::read(component, resources, warnings)
}

pub(super) fn parse_native_pdf_template_text(
    source: &str,
    expected_root: &str,
) -> Result<mapping::PdfLayout, String> {
    pdf::parse_native_template_text(source, expected_root)
}

pub(super) fn read_xbrl_component(
    component: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    xbrl::read(component, resources, warnings)
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
        matches!(
            self,
            Self::Xml | Self::Json | Self::Edi | Self::Db | Self::Xbrl
        )
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
    /// Exact entry path owned by each import-only duplicate-entry marker.
    /// Descendant paths alone cannot identify a keyless cloned group.
    pub(super) input_branch_paths: Vec<(u32, Vec<String>)>,
    /// Keys declared beneath distinct target branch lineages cannot be moved
    /// using the legacy one-key/one-ancestor map.
    pub(super) ambiguous_branch_inputs: BTreeSet<u32>,
    /// Target node rules/cast policies need branch-aware post-processing before
    /// new occurrence lowering can carry them into concatenated segments.
    pub(super) has_target_node_controls: bool,
    pub(super) input_keys: BTreeSet<u32>,
    pub(super) output_keys: BTreeSet<u32>,
    /// True only when a referenced JSON Schema was resolved and imported.
    /// Synthetic and entry-tree fallback components never supply this proof.
    pub(super) json_schema_authoritative: bool,
    /// Raw declaration facts retained only for bounded admission proofs.
    pub(super) port_declarations: Option<PortDeclarations>,
    pub(super) db_queries: Vec<super::db_query::DbQuery>,
    pub(super) db_xml_columns: BTreeMap<u32, database_xml::Column>,
    pub(super) dynamic_json: Option<super::dynamic_json::DynamicJsonTarget>,
    /// Output schema-location hint derived from an XML document component's
    /// settings. It applies only where the component is assembled as a
    /// target; source and variable roles never write it.
    pub(super) target_schema_hints: Option<ir::XmlSchemaHints>,
}

impl SchemaComponent {
    pub(super) fn is_target(&self) -> bool {
        (!self.is_variable && !self.is_source) || self.is_pass_through
    }

    pub(super) fn declares_output(&self, key: u32) -> bool {
        self.output_keys.contains(&key)
            || self
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.declares_output(key))
    }

    pub(super) fn unambiguous_declared_output(&self, key: u32) -> bool {
        self.output_keys.contains(&key)
            && self.ports.contains_key(&key)
            && self
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.unambiguous_output(key))
    }

    pub(super) fn unambiguous_declared_input(&self, key: u32) -> bool {
        self.input_keys.contains(&key)
            && self.ports.contains_key(&key)
            && self
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.unambiguous_input(key))
    }

    pub(super) fn ambiguous_input_affects(&self, key: u32, path: &[String]) -> bool {
        self.port_declarations
            .as_ref()
            .is_none_or(|facts| facts.ambiguous_input_affects(key, path))
    }

    pub(super) fn json_scalar_port(&self, key: u32, ty: ScalarType) -> bool {
        self.port_declarations
            .as_ref()
            .is_some_and(|facts| facts.json_scalar(key, ty))
    }

    pub(super) fn json_array_object_frame_port(&self, key: u32) -> bool {
        self.port_declarations
            .as_ref()
            .is_some_and(|facts| facts.json_array_object_frame(key))
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
/// The `n` input of a transposed XLSX target row after the driver row. Its
/// second path segment is the row's one-based position among the data rows.
/// It must carry the same feed as the driver row's `n`, which owns the field.
/// On a transposed source the same path names a row's alignment-only `n`
/// output, which only a recognized alignment filter may read.
pub(super) const TRANSPOSED_ROW_INDEX_PORT: &str = "\u{1f}ferrule-xlsx-transposed-row-index";

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
#[cfg(test)]
pub(super) fn read_schema_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_schema_component_resolved(component, mfd_path, None, None, warnings)
        .map(|read| read.component)
}

pub(super) fn read_schema_component_with_resources(
    component: &roxmltree::Node,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_schema_component_resolved(component, mfd_path, resources, None, warnings)
        .map(|read| read.component)
}

pub(super) fn read_schema_component_in_package(
    component: &roxmltree::Node,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_schema_component_resolved(
        component,
        resources.mapping_path(),
        Some(resources),
        None,
        warnings,
    )
    .map(|read| read.component)
}

pub(super) fn read_schema_component_in_package_with_provenance(
    component: &roxmltree::Node,
    resources: &ResourceResolver,
    root_view_is_source: Option<bool>,
    warnings: &mut Vec<String>,
) -> Option<XmlSchemaComponentRead> {
    read_schema_component_resolved(
        component,
        resources.mapping_path(),
        Some(resources),
        root_view_is_source,
        warnings,
    )
}

pub(super) struct XmlSchemaComponentRead {
    pub(super) component: SchemaComponent,
    pub(super) entry_tree_fallback: bool,
    pub(super) schema_file_authoritative: bool,
    pub(super) authoritative_schema_path: Option<std::path::PathBuf>,
    pub(super) authoritative_schema_bytes: Option<std::sync::Arc<[u8]>>,
    pub(super) no_port_warning_index: Option<usize>,
}

/// Finds a protocol document wrapper without traversing user payload entries.
/// File wrappers and variable-control wrappers may lead to the document level;
/// names below that level are always user data, regardless of their namespace.
pub(super) fn xml_document_wrapper<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    root.descendants()
        .find(|entry| is_xml_document_wrapper(entry, root))
}

/// Shared protocol identity for root inventory; the reader still selects once.
pub(super) fn is_xml_document_wrapper(
    entry: &roxmltree::Node<'_, '_>,
    root: roxmltree::Node<'_, '_>,
) -> bool {
    fn protocol_namespace(entry: roxmltree::Node<'_, '_>, root: roxmltree::Node<'_, '_>) -> bool {
        let Some(slot) = entry.attribute("ns") else {
            return true;
        };
        let Some(slot) = slot.parse::<usize>().ok() else {
            return false;
        };
        root.children()
            .find(|node| node.has_tag_name("header"))
            .and_then(|header| {
                header
                    .children()
                    .find(|node| node.has_tag_name("namespaces"))
            })
            .and_then(|namespaces| {
                namespaces
                    .children()
                    .filter(|node| node.has_tag_name("namespace"))
                    .nth(slot)
            })
            .is_some_and(|namespace| {
                namespace
                    .attribute("uid")
                    .is_none_or(|uri| uri.is_empty() || uri == "http://www.altova.com/mapforce")
            })
    }
    entry.has_tag_name("entry")
        && entry.attribute("name") == Some("document")
        && entry.attribute("ferrule-kind").is_none()
        && protocol_namespace(*entry, root)
        && entry
            .ancestors()
            .skip(1)
            .take_while(|ancestor| *ancestor != root)
            .all(|ancestor| {
                ancestor.has_tag_name("entry")
                    && ancestor.attribute("ferrule-kind").is_none()
                    && match ancestor.attribute("name") {
                        Some("FileInstance") => protocol_namespace(ancestor, root),
                        Some("parent-context" | "compute-when") => true,
                        _ => false,
                    }
            })
}

/// Shared exact selectors: a consumer census must not admit a later content
/// root or a filename alias that the downstream XML reader selects differently.
pub(super) fn xml_content_entry<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    xml_document_wrapper(root)
        .and_then(|document| {
            document
                .children()
                .find(|node| node.has_tag_name("entry") && !is_document_decoration_entry(node))
        })
        .or_else(|| root.children().find(|node| node.has_tag_name("entry")))
}

pub(super) fn xml_file_instance_input(root: roxmltree::Node<'_, '_>) -> Option<u32> {
    root.descendants()
        .find(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("name") == Some("FileInstance")
                && entry.attribute("inpkey").is_some()
        })
        .and_then(|entry| parse_u32(entry.attribute("inpkey")))
}

fn read_schema_component_resolved(
    component: &roxmltree::Node,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    root_view_is_source: Option<bool>,
    warnings: &mut Vec<String>,
) -> Option<XmlSchemaComponentRead> {
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

    // The document wrapper is a protocol level. Descend through it once,
    // then retain the complete payload even when its local name or namespace
    // intentionally matches either reserved wrapper.
    let entry = xml_content_entry(root_el)?;

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    // The root entry's own port is a document-level connection.
    let mut declarations = PortDeclarationCensus::default();
    declarations.record(entry, &[]);
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_entry_ports_with_declarations(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        Some(&mut declarations),
    );
    let ancestry = input_port_ancestry(&entry, &input_keys);
    let input_ancestors = ancestry.ancestors;
    let no_port_warning_index = (out_count == 0 && in_count == 0).then(|| {
        let index = warnings.len();
        warnings.push(if root_view_is_source.is_some() {
            format!("component `{name}` has connected root-view ports outside its first-entry projection; diagnostic direction is retained without lowering those ports")
        } else {
            format!("component `{name}` has no connected ports")
        });
        index
    });
    let is_source = if out_count == 0 && in_count == 0 {
        root_view_is_source.unwrap_or(true)
    } else {
        out_count >= in_count
    };
    let has_typed_entry_schema = entry.attribute("ferrule-kind").is_some();
    let typed_entry_schema = has_typed_entry_schema
        .then(|| typed_xml_entry_tree_schema(&entry))
        .flatten();
    if has_typed_entry_schema && typed_entry_schema.is_none() {
        warnings.push(format!(
            "component `{name}` has invalid Ferrule typed entry-schema metadata; metadata ignored"
        ));
    }

    // Schema: prefer the referenced XSD (types + repeating info), picking
    // the top-level element the design says the document uses -- an XSD
    // can declare several document roots ("{ns}Local" strips to "Local").
    let instance_root: Vec<String> = document
        .and_then(|d| d.attribute("instanceroot"))
        .map(instance_root_segments)
        .unwrap_or_default();
    let mut authoritative_schema_path = None;
    let mut authoritative_schema_bytes = None;
    let schema_from_file = document
        .and_then(|d| d.attribute("schema"))
        .and_then(|rel| {
            let schema_path = match resolve_xml_resource(mfd_path, resources, rel, "XML Schema") {
                Ok(path) => path,
                Err(error) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({error}); \
                         falling back to the entry tree (no types, no repeating info)"
                    ));
                    return None;
                }
            };
            match read_xml_schema_file(
                &schema_path,
                instance_root.first().map(String::as_str),
                resources,
            ) {
                Ok(schema) => {
                    // A physical unused-source witness must not reopen a bound
                    // pathname behind its immutable-byte reader.
                    authoritative_schema_path =
                        (!schema_path.is_bound()).then(|| schema_path.to_path_buf());
                    authoritative_schema_bytes = schema_path.witness_bytes();
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
                Err(XmlSchemaReadError::Xsd(
                    format_xml::XmlFormatError::UnsupportedXmlWildcard { .. }
                    | format_xml::XmlFormatError::UnsupportedXmlAttributeWildcard { .. },
                )) if typed_entry_schema.is_some() => None,
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree (no types, no repeating info)"
                    ));
                    None
                }
            }
        });
    let schema_file_authoritative = schema_from_file.is_some();
    if !schema_file_authoritative {
        authoritative_schema_path = None;
        authoritative_schema_bytes = None;
    }
    let schema_from_entry_tree = schema_from_file.is_none() && typed_entry_schema.is_none();
    let mut schema = schema_from_file
        .or(typed_entry_schema)
        .unwrap_or_else(|| entry_tree_schema(&entry));
    if let Some(rel) = document.and_then(|document| document.attribute("schema"))
        && let Ok(schema_path) = resolve_xml_resource(mfd_path, resources, rel, "XML Schema")
        && (schema_path.is_bound()
            || !schema_path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dtd")))
    {
        super::alternatives::merge_conditioned_xml_resource_types(
            &entry,
            &mut schema,
            &schema_path,
            resources,
            schema_from_entry_tree,
            warnings,
        );
    }
    merge_generic_xml_entries(&entry, &mut schema);
    reconcile_explicit_text_entries(&entry, &mut schema);
    normalize_xml_text_ports(&schema, &mut ports);

    let is_pass_through = component
        .children()
        .any(|node| node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1"));
    let input_instance = document
        .and_then(|document| document.attribute("inputinstance"))
        .map(str::to_string)
        .or_else(|| nested_file_instance(&root_el, "inputinstance"));
    // The input instance is read only in a source role; a target's is a
    // design-time preview. Pass-through components keep it for their source
    // role and drop it in their target role.
    let local_xml_file_set = (is_source || is_pass_through)
        && input_instance
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
    if is_source && let Some(key) = xml_file_instance_input(root_el) {
        ports.insert(key, vec![SOURCE_INPUT_DOCUMENT_PATH_PORT.to_string()]);
    }
    if !is_source && let Some(key) = xml_file_instance_input(root_el) {
        ports.insert(key, vec![TARGET_DOCUMENT_PATH_PORT.to_string()]);
    }
    let schema_setting = component
        .children()
        .filter(|node| node.has_tag_name("properties"))
        .find_map(|node| node.attribute(crate::schema_location::PROPERTY));
    // The hint names the stored schema, so its own namespace must be known.
    let schema_namespace = authoritative_schema_namespace(
        authoritative_schema_path.as_deref(),
        authoritative_schema_bytes.as_deref(),
    );
    if is_source
        && in_count == 0
        && !is_pass_through
        && !local_xml_file_set
        && !has_typed_entry_schema
        && schema_file_authoritative
        && schema_namespace == Some(None)
        && root_view_is_source.is_none()
        && document.and_then(|d| d.attribute("instanceroot"))
            == Some(format!("{{}}{}", schema.name).as_str())
        && input_instance.is_some()
        && !data.descendants().any(|n| n.has_tag_name("parameter"))
        && schema.closed_source_selection_schema().is_some()
    {
        schema.xml_closed_source_selection = true;
    }
    let target_schema_hints = match crate::schema_location::derived_hints(
        schema_setting,
        schema_namespace
            .as_ref()
            .and(document.and_then(|document| document.attribute("schema"))),
        schema_namespace.clone().flatten().as_deref(),
        &schema,
    ) {
        Ok(hints) => hints,
        Err(error) => {
            if !is_source || is_pass_through {
                warnings.push(format!(
                    "component `{name}`: {error}; its output schema-location hint is not imported"
                ));
            }
            None
        }
    };
    Some(XmlSchemaComponentRead {
        component: SchemaComponent {
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
            input_branch_paths: ancestry.branches,
            ambiguous_branch_inputs: ancestry.ambiguous_inputs,
            has_target_node_controls: has_target_node_controls(component),
            input_keys,
            output_keys,
            json_schema_authoritative: false,
            port_declarations: Some(declarations.finish(root_el)),
            db_queries: Vec::new(),
            db_xml_columns: BTreeMap::new(),
            dynamic_json: None,
            target_schema_hints,
        },
        entry_tree_fallback: schema_from_entry_tree,
        schema_file_authoritative,
        authoritative_schema_path,
        authoritative_schema_bytes,
        no_port_warning_index,
    })
}

/// The authoritative schema document's own `targetNamespace`, or `None` when
/// it cannot be read as an XML Schema within the metadata bound.
fn authoritative_schema_namespace(
    path: Option<&Path>,
    bytes: Option<&[u8]>,
) -> Option<Option<String>> {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let text = match (bytes, path) {
        (Some(bytes), _) => format_xml::xsd::decode_schema_bytes(bytes, MAX_BYTES).ok()?,
        (None, Some(path)) => {
            let file = std::fs::File::open(path).ok()?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(file, MAX_BYTES + 1), &mut bytes)
                .ok()?;
            format_xml::xsd::decode_schema_bytes(&bytes, MAX_BYTES).ok()?
        }
        (None, None) => return None,
    };
    crate::schema_location::schema_target_namespace(&text)
}

pub(super) fn is_local_file_set_pattern(path: &str) -> bool {
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
    let (utf8_bom, csv_text_repair_dependency) = csv::read_text_settings(&text_el, &name, warnings);
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

    let fields = match names_el
        .map(|names| csv::field_declarations(&names))
        .transpose()
    {
        Ok(fields) => fields.unwrap_or_default(),
        Err(reason) => {
            warnings.push(format!("csv component `{name}` {reason}; skipped"));
            return None;
        }
    };
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
        csv_utf8_bom: utf8_bom,
        csv_text_repair_dependency,
        ..FormatOptions::default()
    };
    if let Some(settings) = settings {
        csv::read_settings(&settings, &name, &mut options, warnings);
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
    if is_source && options.csv_preserve_empty_strings {
        csv::warn_typed_empty_cells(&schema, &name, &mut options, warnings);
    }
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
        input_branch_paths: Vec::new(),
        ambiguous_branch_inputs: Default::default(),
        has_target_node_controls: false,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
        json_schema_authoritative: false,
        port_declarations: None,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
        target_schema_hints: None,
    })
}

/// Reads an EDI text component from the visible entry tree. The reference application's
/// external configuration files are not portable with the design, so the
/// fallback preserves connected paths while being explicit about the lost
/// types, qualifiers, and exact cardinalities.
pub(super) fn read_edi_component(
    component: &roxmltree::Node,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    edi::read(
        component,
        resources.mapping_path(),
        Some(resources),
        warnings,
        true,
    )
}

/// Reads an internal structured UDF parameter as a schema declaration only.
/// These components are not independent runtime boundaries, so their EDI and
/// database configuration diagnostics belong to the caller's real component.
pub(super) fn read_definition_parameter_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    definition_parameter::read(component, mfd_path, resources, warnings)
}

fn has_target_node_controls(component: &roxmltree::Node<'_, '_>) -> bool {
    component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && (entry.attribute("casttotargettypemode").is_some()
                || entry
                    .children()
                    .any(|child| child.has_tag_name("inputnodefunctions")))
    })
}

pub(super) fn schema_node_at<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        if let Some(anchor) = &node.recursive_ref {
            node = schema.find_concrete_group(anchor)?;
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
        Some(anchor) => schema.find_concrete_group(anchor),
        None => Some(node),
    }
}

pub(super) fn collect_matching_scalar_paths(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    match (&source.kind, &target.kind) {
        (
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
        ) => paths.push(path.clone()),
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
