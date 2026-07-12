use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use mapping::FormatOptions;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ComponentFormat {
    Xml,
    Json,
    Csv,
    Edi,
    Db,
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
    /// Input key of a variable component's compute-when control entry.
    pub(super) compute_when_key: Option<u32>,
    /// Port key -> absolute entry path (segments below the schema root).
    pub(super) ports: BTreeMap<u32, Vec<String>>,
    pub(super) input_keys: BTreeSet<u32>,
    pub(super) output_keys: BTreeSet<u32>,
}

pub(super) fn parse_u32(attr: Option<&str>) -> Option<u32> {
    attr.and_then(|a| a.parse().ok())
}

fn entry_key_sets(root: &roxmltree::Node) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if let Some(key) = parse_u32(entry.attribute("inpkey")) {
            inputs.insert(key);
        }
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            outputs.insert(key);
        }
    }
    (inputs, outputs)
}

fn is_default_output(component: &roxmltree::Node) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("XSLTDefaultOutput"))
        == Some("1")
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
        .and_then(|document| document.children().find(|node| node.has_tag_name("entry")))
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
        warnings,
    );
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
    let schema = document
        .and_then(|d| d.attribute("schema"))
        .and_then(|rel| {
            let xsd_path = mfd_path.parent().unwrap_or(Path::new(".")).join(rel);
            match format_xml::xsd::import_root(&xsd_path, instance_root.first().map(String::as_str))
            {
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
    normalize_xml_text_ports(&schema, &mut ports);

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Xml,
        schema,
        input_instance: document
            .and_then(|d| d.attribute("inputinstance"))
            .map(str::to_string),
        output_instance: document
            .and_then(|d| d.attribute("outputinstance"))
            .map(str::to_string),
        options: FormatOptions::default(),
        is_source,
        is_default_output: is_default_output(component),
        is_variable: data.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        }),
        compute_when_key: root_el
            .descendants()
            .find(|node| {
                node.has_tag_name("entry") && node.attribute("name") == Some("compute-when")
            })
            .and_then(|entry| parse_u32(entry.attribute("inpkey"))),
        ports,
        input_keys,
        output_keys,
    })
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

/// MapForce puts a simple-content value on its parent element's port. The
/// ferrule schema stores that scalar as a `#text` child, so normalize the
/// port path once at component import and let the graph/scope builders use
/// ordinary scalar paths afterward.
fn normalize_xml_text_ports(schema: &SchemaNode, ports: &mut BTreeMap<u32, Vec<String>>) {
    for path in ports.values_mut() {
        if let Some(text) = schema_node_at(schema, path).and_then(SchemaNode::text_child) {
            path.push(text.name.clone());
        }
    }
}

pub(super) fn note_skipped_library(skipped: &mut Vec<String>, label: &str) {
    if !skipped.iter().any(|l| l == label) {
        skipped.push(label.to_string());
    }
}

/// Records an entry's own port keys under `path`.
fn record_entry_keys(
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

/// Reads a json component: schema from the referenced JSON Schema file
/// (entry tree as fallback), ports normalized so only `json-property`
/// entries contribute path segments -- which lines the paths up with the
/// property/array shapes `json_schema::import` produces.
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

    if json_el.is_some_and(|j| j.attribute("jsonlines") == Some("1")) {
        warnings.push(format!(
            "component `{name}` uses JSON Lines; ferrule reads/writes it as \
             regular JSON, so instances need converting"
        ));
    }

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
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // Schema: prefer the referenced JSON Schema (types + repeating info).
    let schema = json_el
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
        options: FormatOptions::default(),
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
    })
}

/// Walks a json component's entry tree. Only `json-property` entries push
/// a path segment; structural entries (`object`, `array`, `item`, type
/// leaves) are transparent, so a port lands on the path of the property
/// that contains it -- matching the [`SchemaNode`] tree, where a repeating
/// property carries its array's shape directly.
fn collect_json_ports(
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
                path.push(child.attribute("name").unwrap_or_default().to_string());
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
                path.pop();
            }
            Some("json-propertyname") | Some("json-subtype") => {
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
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
            }
        }
    }
}

/// Fallback JSON schema straight from the entry tree: `json-property`
/// children become fields, an enclosing `array` marks the property
/// repeating, type-leaf names give scalar types.
fn json_entry_value_schema(name: &str, entry: &roxmltree::Node) -> SchemaNode {
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

    // The entry tree is FileInstance > document > block(fields).
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;
    let mut block = root_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    while matches!(
        block.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        block = block
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

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

    let mut options = FormatOptions::default();
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
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Csv,
        schema,
        input_instance: text_el.attribute("inputinstance").map(str::to_string),
        output_instance: text_el.attribute("outputinstance").map(str::to_string),
        options,
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
    })
}

/// Reads an EDI text component from the visible entry tree. MapForce's
/// external configuration files are not portable with the design, so the
/// fallback preserves connected paths while being explicit about the lost
/// types, qualifiers, and exact cardinalities.
pub(super) fn read_edi_component(
    component: &roxmltree::Node,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    let text = data
        .children()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"))?;
    let kind = text.attribute("kind").unwrap_or_default();
    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry.children().find(|node| node.has_tag_name("entry"))?;
    }

    let mut schema = edi_entry_tree_schema(&entry, true, false)?;
    schema.name = match kind {
        "EDIX12" => "MFD-X12",
        "EDIFACT" => "MFD-EDIFACT",
        "EDIHL7" => "HL7",
        "EDIFIXED" => "IDOC",
        "SWIFTMT" => "SWIFT",
        "EDITRADACOMS" => "TRADACOMS",
        _ if kind.is_empty() => "EDI",
        other => other,
    }
    .to_string();

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = entry_key_sets(&root);
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_entry_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        warnings,
    );
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }

    warnings.push(format!(
        "EDI component `{name}` uses an entry-tree schema inferred without its external `{kind}` \
         configuration; its mapping graph was imported, but execution is disabled until a schema \
         with element positions, scalar types, fixed qualifiers, and cardinalities is supplied"
    ));
    if !matches!(kind, "EDIX12" | "EDIFACT") {
        warnings.push(format!(
            "EDI component `{name}` uses runtime dialect `{kind}`; its mapping graph was imported, \
             but ferrule currently executes only EDIX12 and EDIFACT instances"
        ));
    }

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Edi,
        schema,
        input_instance: text.attribute("inputinstance").map(str::to_string),
        output_instance: text.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            lenient_segments: true,
            ..FormatOptions::default()
        },
        is_source: out_count >= in_count,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
    })
}

fn edi_entry_tree_schema(
    entry: &roxmltree::Node,
    is_root: bool,
    parent_is_segment: bool,
) -> Option<SchemaNode> {
    let name = entry.attribute("name").unwrap_or("EDI");
    let is_segment = is_inferred_edi_segment(name);
    let child_entries: Vec<_> = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect();
    let children: Vec<_> = child_entries
        .iter()
        .filter_map(|child| edi_entry_tree_schema(child, false, is_segment))
        .collect();
    let connected = entry.attribute("inpkey").is_some() || entry.attribute("outkey").is_some();

    if child_entries.is_empty() {
        return connected.then(|| SchemaNode::scalar(name, ir::ScalarType::String));
    }
    if children.is_empty() && !connected && !is_root {
        return None;
    }

    let node = SchemaNode::group(name, children);
    if !is_root && !parent_is_segment && (connected || !is_segment) {
        Some(node.repeating())
    } else {
        Some(node)
    }
}

fn is_inferred_edi_segment(name: &str) -> bool {
    let name = name.strip_prefix("MF_").unwrap_or(name);
    (2..=3).contains(&name.len())
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
        && name
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        && !name.strip_prefix("SG").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn collect_entry_ports(
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
        let raw_name = child.attribute("name").unwrap_or_default();
        let (name, _) = normalize_xml_entry_name(raw_name);
        if name == "element()" || child.attribute("type") == Some("xml-type") {
            if name == "element()" {
                warnings.push(
                    "generic element() entries are not supported; subtree skipped".to_string(),
                );
                continue;
            }
            // xml-type entries are transparent type wrappers: descend
            // without extending the path.
            collect_entry_ports(&child, path, ports, out_count, in_count, warnings);
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
        collect_entry_ports(&child, path, ports, out_count, in_count, warnings);
        path.pop();
    }
}

/// Fallback schema straight from the entry tree: groups where there are
/// children, string scalars at the leaves (attribute entries flagged as
/// such), no repeating flags.
fn entry_tree_schema(entry: &roxmltree::Node) -> SchemaNode {
    let raw_name = entry.attribute("name").unwrap_or("root");
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if legacy_attribute || entry.attribute("type") == Some("attribute") {
        return SchemaNode::scalar(name, ir::ScalarType::String).attribute();
    }
    let children: Vec<SchemaNode> = entry
        .children()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "entry"
                && n.attribute("name") != Some("element()")
        })
        .map(|c| entry_tree_schema(&c))
        .collect();
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
fn normalize_xml_entry_name(name: &str) -> (&str, bool) {
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
    if component.attribute("kind") == Some("28") {
        warnings.push(format!(
            "skipped database query component `{name}`: SQL query components are not supported yet"
        ));
        return None;
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
    let tables: Vec<roxmltree::Node> = entries
        .iter()
        .copied()
        .filter(|entry| entry.attribute("type") == Some("table"))
        .collect();
    if tables.is_empty() {
        warnings.push(format!(
            "skipped database component `{name}`: it contains no table entries"
        ));
        return None;
    }
    if container.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry
                .attribute("type")
                .is_some_and(|entry_type| entry_type != "table")
    }) {
        warnings.push(format!(
            "database component `{name}` contains non-table database entries; those entries were skipped"
        ));
    }
    let single_plain_table = tables.len() == 1
        && !tables[0]
            .children()
            .any(|n| n.attribute("type") == Some("table"));

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    for table in &tables {
        let mut path = if single_plain_table || tables.len() == 1 {
            Vec::new()
        } else {
            vec![table.attribute("name").unwrap_or_default().to_string()]
        };
        collect_db_ports(table, &mut path, &mut ports, &mut out_count, &mut in_count);
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    if !single_plain_table && !is_source {
        warnings.push(format!(
            "relational database target component `{name}` is non-executable: \
             ferrule can read relational schemas but cannot write them yet"
        ));
    }

    // The connection string lives in the mapping's datasource registry,
    // linked from the component by name.
    let connection = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "database")
        .and_then(|db| db.attribute("ref"))
        .and_then(|r| {
            mapping_el
                .descendants()
                .find(|n| n.has_tag_name("database_connection") && n.attribute("name") == Some(r))
        })
        .and_then(|c| c.attribute("ConnectionString"))
        .map(str::to_string);
    if connection.is_none() {
        warnings.push(format!(
            "database component `{name}` has no resolvable connection; the \
             project needs an instance path filled in"
        ));
    }

    let db_path = connection.as_deref().and_then(|conn| {
        let path = mfd_path.parent().unwrap_or(Path::new(".")).join(conn);
        if path.exists() {
            Some(path)
        } else {
            warnings.push(format!(
                "component `{name}`: database `{conn}` not found next to the \
                 design; falling back to untyped columns"
            ));
            None
        }
    });

    // Keep the exact existing flat-table behavior, including exposing every
    // introspected column. Relational entry trees instead select their own
    // columns and use introspection only to recover each selected leaf type.
    let schema = if single_plain_table {
        let table = tables[0];
        let table_name = table.attribute("name").unwrap_or_default();
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
            })
            .unwrap_or_else(|| db_table_schema(&table, &BTreeMap::new()))
    } else {
        let mut types = BTreeMap::new();
        if let Some(path) = db_path.as_deref() {
            collect_db_column_types(path, &tables, &name, warnings, &mut types);
        }
        if tables.len() == 1 {
            db_table_schema(&tables[0], &types)
        } else {
            SchemaNode::group(
                "database",
                tables
                    .iter()
                    .map(|table| db_table_schema(table, &types))
                    .collect(),
            )
        }
    };
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
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
    })
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
                    let column_types = match schema.kind {
                        SchemaKind::Group { children } => children
                            .into_iter()
                            .filter_map(|column| match column.kind {
                                SchemaKind::Scalar { ty } => Some((column.name, ty)),
                                SchemaKind::Group { .. } => None,
                            })
                            .collect(),
                        SchemaKind::Scalar { .. } => BTreeMap::new(),
                    };
                    types.insert(physical_table.to_string(), column_types);
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
        collect_db_column_types(db_path, &nested, component_name, warnings, types);
    }
}

fn db_table_schema(
    table: &roxmltree::Node,
    types: &BTreeMap<String, BTreeMap<String, ir::ScalarType>>,
) -> SchemaNode {
    let name = table.attribute("name").unwrap_or_default();
    let physical_table = name.split_once('|').map_or(name, |(table, _)| table);
    let columns = types.get(physical_table);
    let children = table
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
        .filter_map(|entry| match entry.attribute("type") {
            Some("table") => Some(db_table_schema(&entry, types)),
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
                Some(SchemaNode::scalar(column, ty))
            }
            Some(_) => None,
        })
        .collect();
    SchemaNode::group(name, children).repeating()
}

pub(super) fn schema_node_at<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

pub(super) fn collect_matching_scalar_paths(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. }) => paths.push(path.clone()),
        (SchemaKind::Group { .. }, SchemaKind::Group { children }) => {
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
mod tests {
    use super::{instance_root_segments, normalize_xml_entry_name};

    #[test]
    fn instance_root_paths_do_not_split_namespace_uris() {
        assert_eq!(
            instance_root_segments(
                "{http://example.com/people}People/{http://example.com/people}Person"
            ),
            ["People", "Person"]
        );
        assert_eq!(
            instance_root_segments("{}People/{}Person"),
            ["People", "Person"]
        );
    }

    #[test]
    fn indexed_xml_entry_names_are_normalized_without_touching_qnames() {
        assert_eq!(normalize_xml_entry_name("0:Person"), ("Person", false));
        assert_eq!(normalize_xml_entry_name("12:@type"), ("type", true));
        assert_eq!(normalize_xml_entry_name("Person"), ("Person", false));
        assert_eq!(normalize_xml_entry_name("ns:Person"), ("ns:Person", false));
    }
}
