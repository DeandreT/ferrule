//! Bounded compiler for selected SWIFT MT generic-item configurations.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr};
use thiserror::Error;

const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 1_024;
const MAX_RAW_NODES: usize = 65_536;
const MAX_EXPANSION_DEPTH: usize = 128;

#[derive(Debug)]
pub struct CompiledSwift {
    pub schema: SchemaNode,
    pub layout: SwiftMtLayout,
}

#[derive(Debug, Error)]
pub enum SwiftConfigError {
    #[error("could not read SWIFT configuration `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse SWIFT configuration `{path}`: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("invalid SWIFT configuration: {0}")]
    Invalid(String),
    #[error("SWIFT configuration exceeds the {0} limit")]
    Limit(&'static str),
}

pub fn import_config(
    envelope_path: &Path,
    selected_messages: &[String],
) -> Result<CompiledSwift, SwiftConfigError> {
    if selected_messages.is_empty() {
        return Err(SwiftConfigError::Invalid(
            "envelope has no selected message types".into(),
        ));
    }
    let envelope_text = read_text(envelope_path)?;
    let envelope = parse_xml(envelope_path, &envelope_text)?;
    if !envelope
        .root_element()
        .descendants()
        .any(|node| node.has_tag_name("Format") && node.attribute("standard") == Some("SWIFTMT"))
    {
        return Err(SwiftConfigError::Invalid(
            "configuration is not SWIFTMT".into(),
        ));
    }
    let common_href = envelope
        .root_element()
        .children()
        .find(|node| node.has_tag_name("Include") && node.attribute("href").is_some())
        .and_then(|node| node.attribute("href"))
        .ok_or_else(|| SwiftConfigError::Invalid("envelope has no common Include".into()))?;
    let common_path = resolve_sibling(envelope_path, common_href)?;
    let common_text = read_text(&common_path)?;
    let common_doc = parse_xml(&common_path, &common_text)?;
    let mut raw_nodes = 0usize;
    let common = definitions(common_doc.root_element(), &mut raw_nodes)?;

    let mut message_layouts = Vec::new();
    let mut message_schemas = Vec::new();
    let mut total_bytes = envelope_text
        .len()
        .checked_add(common_text.len())
        .ok_or(SwiftConfigError::Limit("total file size"))?;
    for selected in selected_messages {
        let path = resolve_message(envelope_path, selected)?;
        let text = read_text(&path)?;
        total_bytes = total_bytes
            .checked_add(text.len())
            .ok_or(SwiftConfigError::Limit("total file size"))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(SwiftConfigError::Limit("total file size"));
        }
        let doc = parse_xml(&path, &text)?;
        let message_type = doc
            .descendants()
            .find(|node| node.has_tag_name("MessageType"))
            .and_then(|node| node.text())
            .ok_or_else(|| SwiftConfigError::Invalid("message has no MessageType".into()))?;
        let message = definitions(doc.root_element(), &mut raw_nodes)?;
        let generic_root = doc
            .descendants()
            .find(|node| node.has_tag_name("GenericRoot"))
            .and_then(|node| node.attribute("ref"))
            .ok_or_else(|| SwiftConfigError::Invalid("message has no GenericRoot/@ref".into()))?;
        let defs = DefinitionSet {
            common: &common,
            message: &message,
        };
        let root = defs.get(generic_root).ok_or_else(|| {
            SwiftConfigError::Invalid(format!("message root `{generic_root}` is not defined"))
        })?;
        let mut fields = Vec::new();
        let mut schemas = Vec::new();
        let mut active = HashSet::new();
        collect_message_items(
            root,
            &[message_type.to_string()],
            &defs,
            &mut fields,
            &mut schemas,
            0,
            &mut active,
        )?;
        message_layouts.push(SwiftMessageLayout::new(message_type, fields));
        message_schemas.push(SchemaNode::group(message_type, schemas));
    }

    let layout = SwiftMtLayout::new(message_layouts)
        .map_err(|error| SwiftConfigError::Invalid(error.to_string()))?;
    let mut message = SchemaNode::group(
        "Message",
        std::iter::once(SchemaNode::group(
            "Application Header",
            vec![
                SchemaNode::group("Input", Vec::new()),
                SchemaNode::group("Output", Vec::new()),
            ],
        ))
        .chain(message_schemas)
        .collect(),
    );
    message.repeating = true;
    Ok(CompiledSwift {
        schema: SchemaNode::group("SWIFT", vec![message]),
        layout,
    })
}

#[derive(Debug, Clone)]
struct RawItem {
    kind: String,
    attrs: BTreeMap<String, String>,
    children: Vec<RawItem>,
}

impl RawItem {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    fn output_name(&self) -> Option<&str> {
        self.attr("nodeName").or_else(|| self.attr("name"))
    }
}

struct DefinitionSet<'a> {
    common: &'a BTreeMap<String, RawItem>,
    message: &'a BTreeMap<String, RawItem>,
}

impl DefinitionSet<'_> {
    fn get(&self, name: &str) -> Option<&RawItem> {
        self.message.get(name).or_else(|| self.common.get(name))
    }
}

fn definitions(
    root: roxmltree::Node<'_, '_>,
    nodes: &mut usize,
) -> Result<BTreeMap<String, RawItem>, SwiftConfigError> {
    let generic = root
        .children()
        .find(|node| node.has_tag_name("GenericItems"))
        .ok_or_else(|| SwiftConfigError::Invalid("configuration has no GenericItems".into()))?;
    let mut result = BTreeMap::new();
    for node in generic.children().filter(roxmltree::Node::is_element) {
        let raw = raw_item(node, nodes)?;
        if let Some(name) = raw.attr("name").map(str::to_string)
            && result.insert(name.clone(), raw).is_some()
        {
            return Err(SwiftConfigError::Invalid(format!(
                "duplicate generic item `{name}`"
            )));
        }
    }
    Ok(result)
}

fn raw_item(node: roxmltree::Node<'_, '_>, nodes: &mut usize) -> Result<RawItem, SwiftConfigError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or(SwiftConfigError::Limit("raw node count"))?;
    if *nodes > MAX_RAW_NODES {
        return Err(SwiftConfigError::Limit("raw node count"));
    }
    Ok(RawItem {
        kind: node.tag_name().name().to_string(),
        attrs: node
            .attributes()
            .map(|attribute| (attribute.name().to_string(), attribute.value().to_string()))
            .collect(),
        children: node
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|child| raw_item(child, nodes))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_message_items(
    item: &RawItem,
    parent_path: &[String],
    defs: &DefinitionSet<'_>,
    fields: &mut Vec<SwiftFieldLayout>,
    schemas: &mut Vec<SchemaNode>,
    depth: usize,
    active: &mut HashSet<String>,
) -> Result<(), SwiftConfigError> {
    check_depth(depth)?;
    let resolved = resolve(item, defs, active)?;
    match resolved.base.kind.as_str() {
        "Sequence" => {
            for child in resolved.children() {
                collect_message_items(
                    child,
                    parent_path,
                    defs,
                    fields,
                    schemas,
                    depth + 1,
                    active,
                )?;
            }
        }
        "Choice" => {
            let name = resolved
                .output_name()
                .ok_or_else(|| SwiftConfigError::Invalid("message Choice has no name".into()))?;
            let mut path = parent_path.to_vec();
            path.push(name.to_string());
            let mut choice_schemas = Vec::new();
            for child in resolved.children() {
                collect_message_items(
                    child,
                    &path,
                    defs,
                    fields,
                    &mut choice_schemas,
                    depth + 1,
                    active,
                )?;
            }
            schemas.push(SchemaNode::group(name, choice_schemas));
        }
        "SwiftField" => {
            let tag = resolved
                .attr("tag")
                .ok_or_else(|| SwiftConfigError::Invalid("SwiftField has no tag".into()))?;
            let name = resolved.output_name().unwrap_or(tag);
            let mut path = parent_path.to_vec();
            path.push(name.to_string());
            let value = compile_field_value(&resolved, defs, depth + 1, active)?;
            let mut schema = schema_for_field(name, &value);
            let repeating = resolved.attr("maxOccurs") == Some("unbounded")
                || resolved
                    .attr("maxOccurs")
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|value| value > 1);
            schema.repeating = repeating;
            fields.push(SwiftFieldLayout::new(tag, path, repeating, value));
            schemas.push(schema);
        }
        other => {
            return Err(SwiftConfigError::Invalid(format!(
                "unsupported message item `{other}`"
            )));
        }
    }
    if let Some(reference) = item.attr("ref") {
        active.remove(reference);
    }
    Ok(())
}

struct Resolved<'a> {
    call: &'a RawItem,
    base: &'a RawItem,
}

impl Resolved<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.call.attr(name).or_else(|| self.base.attr(name))
    }

    fn output_name(&self) -> Option<&str> {
        self.attr("nodeName").or_else(|| self.base.output_name())
    }

    fn children(&self) -> &[RawItem] {
        if self.call.children.is_empty() {
            &self.base.children
        } else {
            &self.call.children
        }
    }
}

fn resolve<'a>(
    item: &'a RawItem,
    defs: &'a DefinitionSet<'_>,
    active: &mut HashSet<String>,
) -> Result<Resolved<'a>, SwiftConfigError> {
    let Some(reference) = item.attr("ref") else {
        return Ok(Resolved {
            call: item,
            base: item,
        });
    };
    if !active.insert(reference.to_string()) {
        return Err(SwiftConfigError::Invalid(format!(
            "recursive generic item `{reference}`"
        )));
    }
    let base = defs.get(reference).ok_or_else(|| {
        SwiftConfigError::Invalid(format!("generic item `{reference}` is not defined"))
    })?;
    Ok(Resolved { call: item, base })
}

fn compile_field_value(
    field: &Resolved<'_>,
    defs: &DefinitionSet<'_>,
    depth: usize,
    active: &mut HashSet<String>,
) -> Result<SwiftValueExpr, SwiftConfigError> {
    if let Some(format) = field.attr("format") {
        return capture(format, Vec::new());
    }
    let separator = field
        .attr("item-separator")
        .map(decode_literal)
        .transpose()?;
    let mut parts = Vec::new();
    for (index, child) in field.children().iter().enumerate() {
        let expression = compile_value(child, &[], defs, depth + 1, active)?;
        let expression = if index > 0 {
            if let Some(separator) = &separator {
                prepend_optional_literal(expression, separator.clone())
            } else {
                expression
            }
        } else {
            expression
        };
        parts.push(expression);
    }
    Ok(sequence(parts))
}

fn compile_value(
    item: &RawItem,
    parent_path: &[String],
    defs: &DefinitionSet<'_>,
    depth: usize,
    active: &mut HashSet<String>,
) -> Result<SwiftValueExpr, SwiftConfigError> {
    check_depth(depth)?;
    let resolved = resolve(item, defs, active)?;
    let mut path = parent_path.to_vec();
    if let Some(name) = resolved.output_name() {
        path.push(name.to_string());
    }
    let expression = match resolved.base.kind.as_str() {
        "SwiftFormat" => capture(
            resolved
                .attr("format")
                .ok_or_else(|| SwiftConfigError::Invalid("SwiftFormat has no format".into()))?,
            path,
        )?,
        "Constant" => SwiftValueExpr::Literal {
            value: decode_literal(resolved.attr("value").unwrap_or_default())?,
        },
        "Sequence" => {
            let parts = resolved
                .children()
                .iter()
                .map(|child| compile_value(child, &path, defs, depth + 1, active))
                .collect::<Result<Vec<_>, _>>()?;
            sequence(parts)
        }
        "Choice" => {
            let values = resolved
                .children()
                .iter()
                .filter(|child| child.kind == "Constant")
                .filter_map(|child| child.attr("value"))
                .map(decode_literal)
                .collect::<Result<Vec<_>, _>>()?;
            if !values.is_empty() && !path.is_empty() {
                SwiftValueExpr::EnumCapture { path, values }
            } else {
                SwiftValueExpr::Alternatives {
                    choices: resolved
                        .children()
                        .iter()
                        .map(|child| compile_value(child, &path, defs, depth + 1, active))
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
        }
        other => {
            return Err(SwiftConfigError::Invalid(format!(
                "unsupported field item `{other}`"
            )));
        }
    };
    if let Some(reference) = item.attr("ref") {
        active.remove(reference);
    }
    Ok(if resolved.attr("minOccurs") == Some("0") {
        SwiftValueExpr::Optional {
            value: Box::new(expression),
        }
    } else {
        expression
    })
}

fn capture(format: &str, path: Vec<String>) -> Result<SwiftValueExpr, SwiftConfigError> {
    let format = format
        .rsplit_once('*')
        .map_or(format, |(_, trailing)| trailing);
    let digits = format.bytes().take_while(u8::is_ascii_digit).count();
    let max = format[..digits]
        .parse::<u16>()
        .map_err(|_| SwiftConfigError::Invalid(format!("invalid SwiftFormat `{format}`")))?;
    let exact = format.as_bytes().get(digits) == Some(&b'!');
    let charset = format
        .chars()
        .last()
        .ok_or_else(|| SwiftConfigError::Invalid("empty SwiftFormat".into()))?;
    let charset = match charset {
        'n' => SwiftCharset::Numeric,
        'a' => SwiftCharset::Alphabetic,
        'c' | 'h' => SwiftCharset::Alphanumeric,
        'd' => SwiftCharset::Decimal,
        'x' | 'y' | 'z' => SwiftCharset::Text,
        other => {
            return Err(SwiftConfigError::Invalid(format!(
                "unsupported SwiftFormat charset `{other}`"
            )));
        }
    };
    Ok(SwiftValueExpr::Capture {
        path,
        min: if exact { max } else { 1 },
        max,
        charset,
    })
}

fn prepend_optional_literal(expression: SwiftValueExpr, literal: String) -> SwiftValueExpr {
    match expression {
        SwiftValueExpr::Optional { value } => SwiftValueExpr::Optional {
            value: Box::new(sequence(vec![
                SwiftValueExpr::Literal { value: literal },
                *value,
            ])),
        },
        other => sequence(vec![SwiftValueExpr::Literal { value: literal }, other]),
    }
}

fn sequence(mut parts: Vec<SwiftValueExpr>) -> SwiftValueExpr {
    if parts.len() == 1 {
        parts.pop().unwrap_or(SwiftValueExpr::Empty)
    } else {
        SwiftValueExpr::Sequence { parts }
    }
}

fn schema_for_field(name: &str, expression: &SwiftValueExpr) -> SchemaNode {
    let mut paths = Vec::new();
    capture_schema_paths(expression, &mut paths);
    let direct_types = paths
        .iter()
        .filter(|(path, _)| path.is_empty())
        .map(|(_, ty)| *ty)
        .collect::<Vec<_>>();
    if !direct_types.is_empty() {
        return SchemaNode::scalar(name, common_capture_type(&direct_types));
    }
    SchemaNode::group(name, path_nodes(&paths, 0))
}

fn capture_schema_paths(expression: &SwiftValueExpr, paths: &mut Vec<(Vec<String>, ScalarType)>) {
    match expression {
        SwiftValueExpr::Capture { path, charset, .. } => {
            let ty = if *charset == SwiftCharset::Decimal {
                ScalarType::Float
            } else {
                ScalarType::String
            };
            paths.push((path.clone(), ty));
        }
        SwiftValueExpr::EnumCapture { path, .. } => {
            paths.push((path.clone(), ScalarType::String));
        }
        SwiftValueExpr::Sequence { parts } => {
            for part in parts {
                capture_schema_paths(part, paths);
            }
        }
        SwiftValueExpr::Alternatives { choices } => {
            for choice in choices {
                capture_schema_paths(choice, paths);
            }
        }
        SwiftValueExpr::Optional { value } => capture_schema_paths(value, paths),
        SwiftValueExpr::Empty | SwiftValueExpr::Literal { .. } => {}
    }
}

fn path_nodes(paths: &[(Vec<String>, ScalarType)], depth: usize) -> Vec<SchemaNode> {
    let mut names = Vec::new();
    for (path, _) in paths {
        if let Some(name) = path.get(depth)
            && !names.contains(name)
        {
            names.push(name.clone());
        }
    }
    names
        .into_iter()
        .map(|name| {
            let nested = paths
                .iter()
                .filter(|(path, _)| path.get(depth) == Some(&name))
                .cloned()
                .collect::<Vec<_>>();
            let leaf_types = nested
                .iter()
                .filter(|(path, _)| path.len() == depth + 1)
                .map(|(_, ty)| *ty)
                .collect::<Vec<_>>();
            if !leaf_types.is_empty() {
                SchemaNode::scalar(&name, common_capture_type(&leaf_types))
            } else {
                SchemaNode::group(&name, path_nodes(&nested, depth + 1))
            }
        })
        .collect()
}

fn common_capture_type(types: &[ScalarType]) -> ScalarType {
    let first = types.first().copied().unwrap_or(ScalarType::String);
    if types.iter().all(|ty| *ty == first) {
        first
    } else {
        ScalarType::String
    }
}

fn check_depth(depth: usize) -> Result<(), SwiftConfigError> {
    if depth > MAX_EXPANSION_DEPTH {
        Err(SwiftConfigError::Limit("expansion depth"))
    } else {
        Ok(())
    }
}

fn decode_literal(value: &str) -> Result<String, SwiftConfigError> {
    let mut output = String::new();
    let mut rest = value;
    while let Some(index) = rest.find("#U+") {
        output.push_str(&rest[..index]);
        let encoded = &rest[index + 3..];
        let end = encoded
            .find(';')
            .ok_or_else(|| SwiftConfigError::Invalid(format!("invalid literal `{value}`")))?;
        let scalar = u32::from_str_radix(&encoded[..end], 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| SwiftConfigError::Invalid(format!("invalid literal `{value}`")))?;
        output.push(scalar);
        rest = &encoded[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn resolve_sibling(base: &Path, relative: &str) -> Result<PathBuf, SwiftConfigError> {
    let path = base
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative);
    path.is_file().then_some(path).ok_or_else(|| {
        SwiftConfigError::Invalid(format!("configuration `{relative}` was not found"))
    })
}

fn resolve_message(base: &Path, message_type: &str) -> Result<PathBuf, SwiftConfigError> {
    let direct = resolve_sibling(base, &format!("{message_type}.Config"));
    if direct.is_ok() {
        return direct;
    }
    let directory = base.parent().unwrap_or_else(|| Path::new("."));
    let mut found = None;
    let mut total_bytes = 0usize;
    for (index, entry) in std::fs::read_dir(directory)
        .map_err(|source| SwiftConfigError::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .enumerate()
    {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(SwiftConfigError::Limit("configuration directory entries"));
        }
        let path = entry
            .map_err(|source| SwiftConfigError::Io {
                path: directory.to_path_buf(),
                source,
            })?
            .path();
        if path.extension().and_then(|value| value.to_str()) != Some("Config") {
            continue;
        }
        let text = read_text(&path)?;
        total_bytes = total_bytes
            .checked_add(text.len())
            .ok_or(SwiftConfigError::Limit("configuration scan size"))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(SwiftConfigError::Limit("configuration scan size"));
        }
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        if doc
            .descendants()
            .any(|node| node.has_tag_name("MessageType") && node.text() == Some(message_type))
            && found.replace(path).is_some()
        {
            return Err(SwiftConfigError::Invalid(format!(
                "message type `{message_type}` has multiple configurations"
            )));
        }
    }
    found.ok_or_else(|| {
        SwiftConfigError::Invalid(format!(
            "message type `{message_type}` has no sibling configuration"
        ))
    })
}

fn read_text(path: &Path) -> Result<String, SwiftConfigError> {
    let file = std::fs::File::open(path).map_err(|source| SwiftConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut text = String::new();
    file.take((MAX_TOTAL_BYTES + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|source| SwiftConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if text.len() > MAX_TOTAL_BYTES {
        return Err(SwiftConfigError::Limit("file size"));
    }
    Ok(text)
}

fn parse_xml<'a>(path: &Path, text: &'a str) -> Result<roxmltree::Document<'a>, SwiftConfigError> {
    roxmltree::Document::parse(text).map_err(|source| SwiftConfigError::Xml {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use ir::SchemaKind;

    use super::*;

    #[test]
    fn compiles_selected_fields_choices_and_optional_parts() {
        let directory = std::env::temp_dir().join(format!(
            "ferrule_swift_config_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("Envelope.Config"),
            r#"<Config><Format standard="SWIFTMT"/><Include href="Common.Config"/><GenericRoot ref="Envelope"/></Config>"#,
        )
        .unwrap();
        std::fs::write(
            directory.join("Common.Config"),
            r#"<Config><GenericItems><Choice name="Mark" type="string"><Constant value="C"/><Constant value="D"/></Choice></GenericItems></Config>"#,
        )
        .unwrap();
        std::fs::write(
            directory.join("MT950.Config"),
            r#"<Config><Format standard="SWIFTMT"/><GenericItems>
              <SwiftField name="Reference" tag="20" format="16x"/>
              <SwiftField name="Line" tag="61"><SwiftFormat nodeName="Date" format="6!n"/><Choice ref="Mark" nodeName="Mark"/><SwiftFormat nodeName="Amount" format="15d"/></SwiftField>
              <Sequence name="MT950"><SwiftField ref="Reference" nodeName="20"/><SwiftField ref="Line" nodeName="61" minOccurs="0" maxOccurs="unbounded"/></Sequence>
            </GenericItems><Message><MessageType>MT950</MessageType><GenericRoot ref="MT950" nodeName="MT950"/></Message></Config>"#,
        )
        .unwrap();

        let compiled =
            import_config(&directory.join("Envelope.Config"), &["MT950".into()]).unwrap();
        let message = compiled
            .schema
            .child("Message")
            .and_then(|node| node.child("MT950"))
            .unwrap();
        assert!(message.child("20").is_some());
        assert!(matches!(
            message
                .child("61")
                .and_then(|node| node.child("Amount"))
                .map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Float
            })
        ));
        let layout = compiled.layout.message("MT950").unwrap();
        assert_eq!(layout.fields().len(), 2);
        assert!(layout.fields()[1].repeating());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
