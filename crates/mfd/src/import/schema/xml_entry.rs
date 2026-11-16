use ir::{ScalarType, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD};

use super::{generic_xml::generic_entry_schema, xml_attribute_required};

/// Fallback schema straight from the entry tree: groups where there are
/// children, string scalars at the leaves (attribute entries flagged as
/// such), no repeating flags.
///
/// Built with an explicit stack: the recursive form took about 9 KiB of
/// debug-build stack per entry level.
pub(in super::super) fn entry_tree_schema(entry: &roxmltree::Node) -> SchemaNode {
    let mut current = match plain_xml_entry(entry) {
        PlainXmlEntry::Complete(schema) => return *schema,
        PlainXmlEntry::Group(group) => group,
    };
    let mut ancestors = Vec::new();
    loop {
        match current.pending.next() {
            Some(child) => match plain_xml_entry(&child) {
                PlainXmlEntry::Complete(schema) => current.children.push(*schema),
                PlainXmlEntry::Group(group) => {
                    ancestors.push(std::mem::replace(&mut current, group));
                }
            },
            None => {
                let mut children = current.children;
                if current.simple_content {
                    children.push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
                }
                let schema = SchemaNode::group(current.name, children);
                let Some(parent) = ancestors.pop() else {
                    return schema;
                };
                current = parent;
                current.children.push(schema);
            }
        }
    }
}

/// One level of [`entry_tree_schema`].
enum PlainXmlEntry<'a, 'input> {
    /// A typed, generic, attribute or leaf entry.
    Complete(Box<SchemaNode>),
    Group(PlainXmlGroup<'a, 'input>),
}

/// A plain entry whose `<entry>` children are still being read.
struct PlainXmlGroup<'a, 'input> {
    name: &'a str,
    simple_content: bool,
    pending: std::vec::IntoIter<roxmltree::Node<'a, 'input>>,
    children: Vec<SchemaNode>,
}

fn plain_xml_entry<'a, 'input>(entry: &roxmltree::Node<'a, 'input>) -> PlainXmlEntry<'a, 'input> {
    if entry.attribute("ferrule-kind").is_some()
        && let Some(schema) = typed_xml_entry_tree_schema(entry)
    {
        return PlainXmlEntry::Complete(Box::new(schema));
    }
    let raw_name = entry.attribute("name").unwrap_or("root");
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if name == XML_ELEMENTS_FIELD {
        return PlainXmlEntry::Complete(Box::new(generic_entry_schema(entry)));
    }
    if legacy_attribute || entry.attribute("type") == Some("attribute") {
        let mut schema = SchemaNode::scalar(name, ir::ScalarType::String).attribute();
        // Public import validates annotation syntax and role before any fallback.
        schema.xml_attribute_required = xml_attribute_required::decode(entry)
            .ok()
            .flatten()
            .unwrap_or(false);
        return PlainXmlEntry::Complete(Box::new(schema));
    }
    let entry_children = entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .collect::<Vec<_>>();
    if entry_children.is_empty() {
        return PlainXmlEntry::Complete(Box::new(SchemaNode::scalar(name, ir::ScalarType::String)));
    }
    let simple_content = (entry.attribute("inpkey").is_some()
        || entry.attribute("outkey").is_some())
        && entry_children.iter().all(|child| {
            let (_, legacy_attribute) =
                normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
            legacy_attribute || child.attribute("type") == Some("attribute")
        });
    PlainXmlEntry::Group(PlainXmlGroup {
        name,
        simple_content,
        pending: entry_children.into_iter(),
        children: Vec::new(),
    })
}

/// Builds the schema with an explicit stack: the recursive form took about
/// 18 KiB of debug-build stack per entry level, the most of any entry walker.
pub(super) fn typed_xml_entry_tree_schema(entry: &roxmltree::Node<'_, '_>) -> Option<SchemaNode> {
    let mut current = match typed_xml_entry(entry)? {
        TypedXmlEntry::Complete(schema) => return Some(schema),
        TypedXmlEntry::Group(group) => group,
    };
    let mut ancestors = Vec::new();
    loop {
        match current.pending.next() {
            Some(child) => match typed_xml_entry(&child)? {
                TypedXmlEntry::Complete(schema) => current.children.push(schema),
                TypedXmlEntry::Group(group) => {
                    ancestors.push(std::mem::replace(&mut current, group));
                }
            },
            None => {
                let mut schema = current.schema;
                if let SchemaKind::Group { children, .. } = &mut schema.kind {
                    *children = current.children;
                }
                let Some(parent) = ancestors.pop() else {
                    return Some(schema);
                };
                current = parent;
                current.children.push(schema);
            }
        }
    }
}

/// One level of [`typed_xml_entry_tree_schema`].
enum TypedXmlEntry<'a, 'input> {
    Complete(SchemaNode),
    Group(TypedXmlGroup<'a, 'input>),
}

/// A typed group, its other metadata already applied, whose `<entry>`
/// children are still being read.
struct TypedXmlGroup<'a, 'input> {
    schema: SchemaNode,
    pending: std::vec::IntoIter<roxmltree::Node<'a, 'input>>,
    children: Vec<SchemaNode>,
}

fn typed_xml_entry<'a, 'input>(
    entry: &roxmltree::Node<'a, 'input>,
) -> Option<TypedXmlEntry<'a, 'input>> {
    let raw_name = entry.attribute("name")?;
    let (name, legacy_attribute) = normalize_xml_entry_name(raw_name);
    if name.is_empty() {
        return None;
    }
    let repeating = match entry.attribute("ferrule-repeating")? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let optional = match entry.attribute("ferrule-xml-optional") {
        None | Some("0") => false,
        Some("1") => true,
        _ => return None,
    };
    if name == XML_ELEMENTS_FIELD {
        if entry.attribute("ferrule-kind") != Some("group") || !repeating {
            return None;
        }
        let mut schema = generic_entry_schema(entry);
        schema.xml_optional = optional;
        if let Some(index) = entry.attribute("ferrule-text-index") {
            let index = index.parse::<usize>().ok()?;
            let SchemaKind::Group { children, .. } = &mut schema.kind else {
                return None;
            };
            let current = children
                .iter()
                .position(|child| child.name == XML_TEXT_FIELD)?;
            let text = children.remove(current);
            if index > children.len() {
                return None;
            }
            children.insert(index, text);
        }
        return Some(TypedXmlEntry::Complete(schema));
    }
    let children = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let group = match entry.attribute("ferrule-kind")? {
        "group" if entry.attribute("datatype").is_none() => true,
        "scalar" if children.is_empty() => false,
        _ => return None,
    };
    let mut schema = if group {
        SchemaNode::group(name, Vec::new())
    } else {
        let scalar_type = match entry.attribute("datatype")? {
            "string" => ScalarType::String,
            "integer" => ScalarType::Int,
            "decimal" => ScalarType::Float,
            "boolean" => ScalarType::Bool,
            _ => return None,
        };
        SchemaNode::scalar(name, scalar_type)
    };
    schema.repeating = repeating;
    schema.xml_optional = optional;
    schema.xml_attribute_required = xml_attribute_required::decode(entry).ok()?.unwrap_or(false);
    schema.attribute = legacy_attribute || entry.attribute("type") == Some("attribute");
    schema.text = entry.attribute("ferrule-text") == Some("1");
    schema.nillable = entry.attribute("ferrule-nillable") == Some("1");
    schema.fixed = entry.attribute("ferrule-fixed").map(str::to_string);
    schema.default = entry.attribute("ferrule-default").map(str::to_string);
    Some(if group {
        TypedXmlEntry::Group(TypedXmlGroup {
            schema,
            pending: children.into_iter(),
            children: Vec::new(),
        })
    } else {
        TypedXmlEntry::Complete(schema)
    })
}

/// Older `.mfd` entry trees prefix XML names with a decimal namespace-slot
/// index and encode attributes with a leading `@` (`0:Person`, `12:@type`).
/// Real QName prefixes are left intact because only all-decimal prefixes are
/// an entry-tree index.
pub(in super::super) fn normalize_xml_entry_name(name: &str) -> (&str, bool) {
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
