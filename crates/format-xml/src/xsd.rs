//! A deliberately small XSD importer: enough to turn the common
//! `xs:element` / `xs:complexType` / `xs:sequence` shapes into a
//! [`SchemaNode`] tree, including the "wrap a single element in an
//! `xs:sequence maxOccurs="unbounded"`" idiom real-world schemas use for
//! repeating groups. `xs:attribute` declarations directly under a
//! `xs:complexType` (or its `complexContent` extension) become
//! attribute-flagged scalar children; `xs:element ref="..."`, named
//! top-level complex/simple types, and `complexContent`/`xs:extension`
//! resolve across local `xs:include` and `xs:import` schema locations
//! (recursive declarations remain finite named references); anonymous
//! `xs:sequence`, `xs:choice`, and `xs:all` particles import as named child
//! fields. Repeating anonymous sequences retain their member occurrence
//! metadata and input order while projecting repeating named ports, allowing
//! exact read/write/export roundtrips and rejecting ambiguous newly constructed
//! tuples. `xs:simpleContent` becomes a `#text` scalar plus attribute scalars.
//! It does not support unions, `xs:any`, or
//! remote schema URLs -- that's the "lite" in the name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ir::{
    ScalarType, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD, XmlNamespace,
    XmlRepeatingSequence, XmlSequenceMember,
};
use roxmltree::Node;

use crate::XmlFormatError;

mod export;
mod groups;
mod restriction;
mod substitution;

pub use export::{XsdExportArtifact, XsdExportSet, export, export_namespace, export_set};

const MAX_MATERIALIZED_SCHEMA_ELEMENTS: usize = 4_096;
const MAX_TYPE_DERIVATION_DEPTH: usize = 256;
const ALTERNATIVE_VIEW_NAMESPACE: &str = "urn:ferrule:xsd:group-alternatives";
const LEGACY_NAME_NAMESPACE: &str = "urn:ferrule:xsd:legacy-name";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveDeclaration {
    path: PathBuf,
    kind: &'static str,
    name: String,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DeclarationQuery {
    path: PathBuf,
    kind: &'static str,
    qname: String,
}

#[derive(Default)]
struct ParseState {
    active: Vec<ActiveDeclaration>,
    complex_types: BTreeMap<ActiveDeclaration, CachedComplexType>,
    complex_type_anchors: Vec<String>,
    declaration_paths: BTreeMap<DeclarationQuery, Option<PathBuf>>,
    materialized_elements: usize,
    materialization_limit_reached: bool,
    unsupported_particle: Option<XmlFormatError>,
    unsupported_default: Option<XmlFormatError>,
    unsupported_schema_group: Option<XmlFormatError>,
    unsupported_restriction: Option<XmlFormatError>,
    unsupported_substitution: Option<XmlFormatError>,
    substitutions: substitution::SubstitutionIndex,
}

enum ComplexTypeResolution {
    Group(ParsedComplexType),
    Recursive(String),
}

#[derive(Clone)]
struct CachedComplexType {
    group: ParsedComplexType,
    anchor: String,
}

#[derive(Clone, Default)]
struct ParsedComplexType {
    children: Vec<SchemaNode>,
    repeating_sequences: Vec<XmlRepeatingSequence>,
}

impl ParsedComplexType {
    fn into_schema(self, name: impl Into<String>) -> SchemaNode {
        let mut schema = SchemaNode::group(name, self.children);
        schema.xml_repeating_sequences = self.repeating_sequences;
        schema
    }

    fn extend(&mut self, other: Self) {
        self.children.extend(other.children);
        self.repeating_sequences.extend(other.repeating_sequences);
    }
}

impl ParseState {
    fn declaration(path: &Path, kind: &'static str, name: &str) -> ActiveDeclaration {
        ActiveDeclaration {
            path: normalized_path(path),
            kind,
            name: name.to_string(),
        }
    }

    fn enter(&mut self, path: &Path, kind: &'static str, name: &str) -> bool {
        let declaration = Self::declaration(path, kind, name);
        if self.active.contains(&declaration) {
            return false;
        }
        self.active.push(declaration);
        true
    }

    fn leave(&mut self) {
        self.active.pop();
    }

    fn reserve_element(&mut self) -> bool {
        self.reserve_elements(1)
    }

    fn reserve_elements(&mut self, count: usize) -> bool {
        let Some(total) = self.materialized_elements.checked_add(count) else {
            self.materialization_limit_reached = true;
            return false;
        };
        if total > MAX_MATERIALIZED_SCHEMA_ELEMENTS {
            self.materialization_limit_reached = true;
            return false;
        }
        self.materialized_elements = total;
        true
    }

    fn has_element_capacity(&mut self) -> bool {
        let has_capacity = self.materialized_elements < MAX_MATERIALIZED_SCHEMA_ELEMENTS;
        if !has_capacity {
            self.materialization_limit_reached = true;
        }
        has_capacity
    }

    fn find_external_declaration(
        &mut self,
        schema_el: &Node,
        schema_path: &Path,
        kind: &'static str,
        qname: &str,
    ) -> Option<PathBuf> {
        let query = DeclarationQuery {
            path: normalized_path(schema_path),
            kind,
            qname: qname.to_string(),
        };
        if let Some(path) = self.declaration_paths.get(&query) {
            return path.clone();
        }
        let path = find_external_declaration(schema_el, schema_path, kind, qname);
        self.declaration_paths.insert(query, path.clone());
        path
    }

    fn finish(self, schema: SchemaNode) -> Result<SchemaNode, XmlFormatError> {
        if let Some(error) = self.unsupported_particle {
            return Err(error);
        }
        if let Some(error) = self.unsupported_default {
            return Err(error);
        }
        if let Some(error) = self.unsupported_schema_group {
            return Err(error);
        }
        if let Some(error) = self.unsupported_restriction {
            return Err(error);
        }
        if let Some(error) = self.unsupported_substitution {
            return Err(error);
        }
        if self.materialization_limit_reached {
            return Err(XmlFormatError::SchemaMaterializationLimit {
                limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
            });
        }
        if let Some(group) = invalid_repeating_sequence_group(&schema) {
            return Err(XmlFormatError::InvalidRepeatingSequenceSchema {
                group: group.to_string(),
            });
        }
        crate::instance::validate_namespace_siblings(&schema)?;
        Ok(schema)
    }

    fn reject_repeating_particle(&mut self, error: XmlFormatError) {
        self.unsupported_particle.get_or_insert(error);
    }

    fn reject_default(&mut self, error: XmlFormatError) {
        self.unsupported_default.get_or_insert(error);
    }

    fn reject_schema_group(&mut self, error: XmlFormatError) {
        self.unsupported_schema_group.get_or_insert(error);
    }

    fn reject_restriction(&mut self, error: XmlFormatError) {
        self.unsupported_restriction.get_or_insert(error);
    }

    fn reject_substitution(&mut self, error: XmlFormatError) {
        self.unsupported_substitution.get_or_insert(error);
    }
}

fn read_xml_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0x00, 0x00, 0xfe, 0xff]) || bytes.starts_with(&[0xff, 0xfe, 0x00, 0x00])
    {
        return Err(invalid_xml_encoding("UTF-32 schemas are not supported"));
    }
    if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(body, u16::from_le_bytes);
    }
    if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(body, u16::from_be_bytes);
    }
    if bytes.starts_with(&[b'<', 0, b'?', 0]) {
        return decode_utf16(&bytes, u16::from_le_bytes);
    }
    if bytes.starts_with(&[0, b'<', 0, b'?']) {
        return decode_utf16(&bytes, u16::from_be_bytes);
    }
    let decoded = match bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        Some(body) => String::from_utf8(body.to_vec()),
        None => String::from_utf8(bytes),
    };
    decoded.map_err(|error| invalid_xml_encoding(&format!("schema is not UTF-8: {error}")))
}

fn decode_utf16(bytes: &[u8], decode: fn([u8; 2]) -> u16) -> std::io::Result<String> {
    let (chunks, remainder) = bytes.as_chunks::<2>();
    let units = chunks.iter().copied().map(decode).collect::<Vec<_>>();
    if !remainder.is_empty() {
        return Err(invalid_xml_encoding(
            "UTF-16 schema contains an incomplete code unit",
        ));
    }
    String::from_utf16(&units)
        .map_err(|error| invalid_xml_encoding(&format!("schema is not valid UTF-16: {error}")))
}

fn invalid_xml_encoding(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

/// Imports the first root element declaration of an XSD file as a
/// [`SchemaNode`].
pub fn import(path: &std::path::Path) -> Result<SchemaNode, XmlFormatError> {
    import_root(path, None)
}

/// Imports the named top-level element declaration -- for schemas that
/// declare several document roots, where the caller knows which one an
/// instance actually uses. `None` falls back to the first declaration.
pub fn import_root(
    path: &std::path::Path,
    root: Option<&str>,
) -> Result<SchemaNode, XmlFormatError> {
    let text = read_xml_text(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let schema_el = doc.root_element();
    let root_local =
        root.map(|name| expanded_name(name).map_or(local_name(name), |(_, local)| local));
    let root_namespace = root.and_then(expanded_name).map(|(namespace, _)| namespace);
    let root_element = schema_el.children().find(|n| {
        n.is_element()
            && n.tag_name().name() == "element"
            && root_local.is_none_or(|name| n.attribute("name") == Some(name))
            && root_namespace
                .is_none_or(|namespace| schema_el.attribute("targetNamespace") == Some(namespace))
    });
    if let Some(root_element) = root_element {
        let mut state = ParseState {
            substitutions: substitution::build(&schema_el, path),
            ..ParseState::default()
        };
        let schema = parse_element_declaration(
            &root_element,
            &schema_el,
            path,
            root_element.attribute("name").unwrap_or_default(),
            &mut state,
        )
        .unwrap_or_else(|| {
            SchemaNode::recursive_group(
                root_element.attribute("name").unwrap_or_default(),
                root_element.attribute("name").unwrap_or_default(),
            )
        });
        return state.finish(schema);
    }

    // An included schema contributes its declarations to the including
    // document. When the caller names the instance root, honor a root that
    // lives in one of those sibling files too.
    if let Some(root) = root
        && let Some(external_path) = find_external_declaration(&schema_el, path, "element", root)
    {
        let external_text = read_xml_text(&external_path)?;
        let external_doc = roxmltree::Document::parse(&external_text)?;
        let external_schema = external_doc.root_element();
        let root_local = expanded_name(root).map_or(local_name(root), |(_, local)| local);
        if let Some(root_element) = top_level(&external_schema, "element", root_local) {
            let mut state = ParseState {
                substitutions: substitution::build(&schema_el, path),
                ..ParseState::default()
            };
            let schema = parse_element_declaration(
                &root_element,
                &external_schema,
                &external_path,
                root_local,
                &mut state,
            )
            .unwrap_or_else(|| SchemaNode::recursive_group(root_local, root_local));
            return state.finish(schema);
        }
    }

    Err(XmlFormatError::MissingElement(match root {
        Some(r) => format!("root xs:element `{r}`"),
        None => "root xs:element".to_string(),
    }))
}

/// Imports a named complex type, resolving it through local includes and
/// imports. The returned group is named after the type's local QName.
pub fn import_type(path: &Path, type_name: &str) -> Result<SchemaNode, XmlFormatError> {
    let text = read_xml_text(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let schema_el = doc.root_element();
    let mut state = ParseState {
        substitutions: substitution::build(&schema_el, path),
        ..ParseState::default()
    };
    let (namespace, local) = type_name
        .strip_prefix('{')
        .and_then(|name| name.split_once('}'))
        .map_or((None, local_name(type_name)), |(namespace, local)| {
            (Some(namespace), local)
        });
    let group = match namespace {
        None => resolve_complex_type(type_name, &schema_el, path, &mut state, Some(local)),
        Some(namespace) if schema_el.attribute("targetNamespace") == Some(namespace) => {
            resolve_complex_type(local, &schema_el, path, &mut state, Some(local))
        }
        Some(namespace) => {
            let mut visited = BTreeSet::new();
            visited.insert(normalized_path(path));
            let effective_namespace = schema_el.attribute("targetNamespace");
            search_dependencies(
                &schema_el,
                path,
                "complexType",
                local,
                Some(namespace),
                effective_namespace,
                &mut visited,
            )
            .and_then(|external_path| {
                let text = read_xml_text(&external_path).ok()?;
                let doc = roxmltree::Document::parse(&text).ok()?;
                let external_schema = doc.root_element();
                let declaration = top_level(&external_schema, "complexType", local)?;
                parse_complex_type_declaration(
                    &declaration,
                    &external_schema,
                    &external_path,
                    local,
                    &mut state,
                    Some(local),
                )
            })
        }
    }
    .and_then(|resolved| match resolved {
        ComplexTypeResolution::Group(group) => Some(group),
        ComplexTypeResolution::Recursive(_) => None,
    })
    .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:complexType `{type_name}`")))?;
    state.finish(group.into_schema(local))
}

/// Resolves the direct complex-content base of a named complex type.
/// The returned name uses ferrule's expanded `{namespace}local` form when
/// the base belongs to a target namespace.
pub fn import_type_base(path: &Path, type_name: &str) -> Result<Option<String>, XmlFormatError> {
    let text = read_xml_text(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let schema_el = doc.root_element();
    let local = expanded_name(type_name).map_or(local_name(type_name), |(_, local)| local);
    let belongs_to_schema = expanded_name(type_name).map_or_else(
        || is_local_qname(&schema_el, type_name),
        |(namespace, _)| schema_el.attribute("targetNamespace") == Some(namespace),
    );
    let local_declaration = belongs_to_schema
        .then(|| top_level(&schema_el, "complexType", local))
        .flatten();
    if let Some(declaration) = local_declaration {
        return Ok(complex_type_base_name(&declaration, &schema_el));
    }

    let declaration_path = find_external_declaration(
        &schema_el,
        path,
        "complexType",
        if expanded_name(type_name).is_some() {
            type_name
        } else {
            local
        },
    )
    .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:complexType `{type_name}`")))?;
    let external_text = read_xml_text(&declaration_path)?;
    let external_doc = roxmltree::Document::parse(&external_text)?;
    let external_schema = external_doc.root_element();
    let declaration = top_level(&external_schema, "complexType", local).ok_or_else(|| {
        XmlFormatError::MissingElement(format!("named xs:complexType `{type_name}`"))
    })?;
    Ok(complex_type_base_name(&declaration, &external_schema))
}

fn complex_type_base_name(declaration: &Node, schema: &Node) -> Option<String> {
    let base = declaration
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "complexContent")?
        .children()
        .find(|node| {
            node.is_element() && matches!(node.tag_name().name(), "extension" | "restriction")
        })?
        .attribute("base")?;
    if expanded_name(base).is_some() {
        return Some(base.to_string());
    }
    match base.split_once(':') {
        Some((prefix, local)) => schema
            .lookup_namespace_uri(Some(prefix))
            .filter(|namespace| !namespace.is_empty())
            .map(|namespace| format!("{{{namespace}}}{local}")),
        None => schema
            .attribute("targetNamespace")
            .filter(|namespace| !namespace.is_empty())
            .map_or_else(
                || Some(base.to_string()),
                |namespace| Some(format!("{{{namespace}}}{base}")),
            ),
    }
}

fn parse_element(
    el: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> SchemaNode {
    let fallback_name = el
        .attribute("name")
        .or_else(|| el.attribute("ref").map(local_name))
        .unwrap_or_default();
    if !state.reserve_element() {
        return SchemaNode::scalar(fallback_name, ScalarType::String);
    }
    if el.attribute("name").is_none()
        && let Some(r) = el.attribute("ref")
    {
        let local = r.rsplit(':').next().unwrap_or(r);
        if let Some(node) = resolve_element(r, schema_el, schema_path, state) {
            return node;
        }
        // An unresolved non-recursive reference still degrades leniently.
        let mut node = SchemaNode::scalar(local, ScalarType::String);
        node.xml_namespace = qname_namespace(el, r);
        return node;
    }
    let name = el.attribute("name").unwrap_or_default().to_string();
    let mut node = if let Some(complex_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "complexType")
    {
        parse_complex_type(&complex_type, schema_el, schema_path, state).into_schema(name)
    } else if let Some(simple_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "simpleType")
    {
        SchemaNode::scalar(name, simple_type_scalar(&simple_type))
    } else if let Some(ty) = el.attribute("type") {
        if let Some(resolved) = resolve_complex_type(ty, schema_el, schema_path, state, Some(&name))
        {
            match resolved {
                ComplexTypeResolution::Group(group) => {
                    let mut node = group.into_schema(name);
                    attach_type_alternatives(&mut node, ty, schema_el, schema_path, state);
                    node
                }
                ComplexTypeResolution::Recursive(anchor) => {
                    SchemaNode::recursive_group(name, anchor)
                }
            }
        } else if let Some(ty) = resolve_simple_type(ty, schema_el, schema_path, state) {
            SchemaNode::scalar(name, ty)
        } else {
            SchemaNode::scalar(name, map_xsd_type(ty))
        }
    } else {
        SchemaNode::scalar(name, ScalarType::String)
    };
    node.xml_namespace = declaration_namespace(el, schema_el, false);
    apply_exported_alternative_view(el, &mut node);
    apply_fixed_value(el, &mut node);
    apply_default_value(el, &mut node, state);
    if el
        .attribute("nillable")
        .is_some_and(|value| matches!(value, "true" | "1"))
    {
        node.nillable()
    } else {
        node
    }
}

fn declaration_namespace(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    attribute: bool,
) -> Option<XmlNamespace> {
    if declaration.attribute((LEGACY_NAME_NAMESPACE, "namespace")) == Some("legacy") {
        return None;
    }
    let global = declaration.parent() == Some(*schema);
    let default = if attribute {
        "attributeFormDefault"
    } else {
        "elementFormDefault"
    };
    let qualified = global
        || declaration.attribute("form") == Some("qualified")
        || (declaration.attribute("form") != Some("unqualified")
            && schema.attribute(default) == Some("qualified"));
    if qualified
        && let Some(namespace) = schema
            .attribute("targetNamespace")
            .filter(|namespace| !namespace.is_empty())
        && let Some(namespace) = XmlNamespace::qualified(namespace)
    {
        Some(namespace)
    } else {
        let explicitly_unqualified = declaration.attribute("form") == Some("unqualified");
        let schema_has_namespace = schema
            .attribute("targetNamespace")
            .is_some_and(|namespace| !namespace.is_empty());
        (explicitly_unqualified || schema_has_namespace).then_some(XmlNamespace::Unqualified)
    }
}

fn qname_namespace(node: &Node<'_, '_>, qname: &str) -> Option<XmlNamespace> {
    let (prefix, _) = qname.split_once(':')?;
    node.lookup_namespace_uri(Some(prefix))
        .filter(|namespace| !namespace.is_empty())
        .and_then(XmlNamespace::qualified)
}

fn apply_fixed_value(declaration: &Node<'_, '_>, node: &mut SchemaNode) {
    let Some(fixed) = declaration.attribute("fixed") else {
        return;
    };
    match &mut node.kind {
        SchemaKind::Scalar { .. } => node.fixed = Some(fixed.to_string()),
        SchemaKind::Group { children, .. } => {
            if let Some(text) = children.iter_mut().find(|child| child.text) {
                text.fixed = Some(fixed.to_string());
            }
        }
    }
}

fn apply_default_value(declaration: &Node<'_, '_>, node: &mut SchemaNode, state: &mut ParseState) {
    let Some(default) = declaration.attribute("default") else {
        return;
    };
    match &mut node.kind {
        SchemaKind::Scalar { .. } => node.default = Some(default.to_string()),
        SchemaKind::Group { children, .. } => {
            if let Some(text) = children.iter_mut().find(|child| child.text) {
                text.default = Some(default.to_string());
            } else {
                state.reject_default(XmlFormatError::UnsupportedSchemaDefault {
                    name: node.name.clone(),
                    reason: "only scalar elements and simple-content elements support defaults",
                });
            }
        }
    }
}

fn apply_exported_alternative_view(el: &Node, node: &mut SchemaNode) {
    let names = el
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "annotation")
        .and_then(|annotation| {
            annotation.children().find(|child| {
                child.is_element()
                    && child.tag_name().name() == "appinfo"
                    && child.attribute("source") == Some(ALTERNATIVE_VIEW_NAMESPACE)
            })
        })
        .into_iter()
        .flat_map(|appinfo| appinfo.children())
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == "type"
                && child.tag_name().namespace() == Some(ALTERNATIVE_VIEW_NAMESPACE)
        })
        .filter_map(|child| child.attribute("name").map(str::to_string))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return;
    }
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &node.kind
    else {
        return;
    };
    let selected = names
        .iter()
        .map(|name| {
            alternatives
                .iter()
                .find(|alternative| alternative.name == *name)
                .cloned()
        })
        .collect::<Option<Vec<_>>>();
    let Some(selected) = selected else {
        return;
    };
    let selected_members = selected
        .iter()
        .flat_map(|alternative| alternative.members.iter().cloned())
        .collect::<BTreeSet<_>>();
    let original_children = children.clone();
    let retained = children
        .iter()
        .filter(|child| selected_members.contains(&child.name))
        .cloned()
        .collect::<Vec<_>>();
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return;
    };
    *children = retained;
    if !node.set_alternatives(selected) {
        // The exported view is advisory metadata; malformed external metadata
        // leaves the ordinary XSD-derived alternatives intact.
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            *children = original_children;
        }
    }
}

fn attach_type_alternatives(
    node: &mut SchemaNode,
    base_qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) {
    let base_identity = expanded_qname_identity(
        schema_el,
        schema_el.attribute("targetNamespace"),
        base_qname,
    );
    let Some(base_identity) = base_identity else {
        return;
    };
    let base_is_abstract = complex_type_is_abstract(schema_el, schema_path, base_qname, state);
    let mut index = DerivedTypeIndex::default();
    collect_derived_type_declarations(
        schema_el,
        schema_path,
        schema_el.attribute("targetNamespace"),
        &mut BTreeSet::new(),
        &mut index,
    );
    if index.limit_reached {
        state.materialization_limit_reached = true;
        return;
    }
    let Some(derived) = index.concrete_descendants(&base_identity) else {
        return;
    };
    if derived.is_empty() {
        return;
    }

    let SchemaKind::Group {
        children: base_children,
        ..
    } = &node.kind
    else {
        return;
    };
    let base_members = base_children
        .iter()
        .map(|child| child.name.clone())
        .collect::<Vec<_>>();
    let original_children = base_children.clone();
    let mut resolved = Vec::new();
    for derived in derived {
        let Ok(text) = read_xml_text(&derived.path) else {
            return;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            return;
        };
        let derived_schema = document.root_element();
        let Some(declaration) = top_level(&derived_schema, "complexType", &derived.local) else {
            return;
        };
        let Some(ComplexTypeResolution::Group(group)) = parse_complex_type_declaration(
            &declaration,
            &derived_schema,
            &derived.path,
            &derived.local,
            state,
            None,
        ) else {
            return;
        };
        if !group.repeating_sequences.is_empty() {
            return;
        }
        resolved.push((derived.identity, group.children));
    }
    let alternative_count = resolved.len() + usize::from(!base_is_abstract);

    let mut merged = base_children.clone();
    for (_, children) in &resolved {
        for child in children {
            if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
                if existing != child {
                    return;
                }
            } else {
                merged.push(child.clone());
            }
        }
    }
    let mut alternatives = Vec::with_capacity(alternative_count);
    if !base_is_abstract {
        alternatives.push(ir::GroupAlternative {
            name: base_identity,
            members: base_members,
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }
    alternatives.extend(
        resolved
            .into_iter()
            .map(|(name, children)| ir::GroupAlternative {
                name,
                members: children.into_iter().map(|child| child.name).collect(),
                required: Vec::new(),
                constraints: Vec::new(),
            }),
    );
    if let SchemaKind::Group { children, .. } = &mut node.kind {
        *children = merged;
    }
    if !node.set_alternatives(alternatives)
        && let SchemaKind::Group { children, .. } = &mut node.kind
    {
        *children = original_children;
    }
}

#[derive(Debug, Clone)]
struct DerivedTypeDeclaration {
    path: PathBuf,
    local: String,
    identity: String,
    base_identity: String,
    abstract_type: bool,
}

#[derive(Default)]
struct DerivedTypeIndex {
    declarations: Vec<DerivedTypeDeclaration>,
    by_identity: BTreeMap<String, usize>,
    conflicting_identity: bool,
    limit_reached: bool,
}

impl DerivedTypeIndex {
    fn insert(&mut self, declaration: DerivedTypeDeclaration) {
        if let Some(existing) = self
            .by_identity
            .get(&declaration.identity)
            .and_then(|index| self.declarations.get(*index))
        {
            if existing.path != declaration.path
                || existing.local != declaration.local
                || existing.base_identity != declaration.base_identity
                || existing.abstract_type != declaration.abstract_type
            {
                self.conflicting_identity = true;
            }
            return;
        }
        if self.declarations.len() >= MAX_MATERIALIZED_SCHEMA_ELEMENTS {
            self.limit_reached = true;
            return;
        }
        self.by_identity
            .insert(declaration.identity.clone(), self.declarations.len());
        self.declarations.push(declaration);
    }

    fn concrete_descendants(&self, base_identity: &str) -> Option<Vec<DerivedTypeDeclaration>> {
        if self.conflicting_identity {
            return None;
        }
        let mut by_base = BTreeMap::<&str, Vec<&DerivedTypeDeclaration>>::new();
        for declaration in &self.declarations {
            by_base
                .entry(&declaration.base_identity)
                .or_default()
                .push(declaration);
        }
        let mut descendants = Vec::new();
        let mut active = BTreeSet::from([base_identity.to_string()]);
        let mut visited = BTreeSet::new();
        let mut stack = vec![(base_identity.to_string(), 0usize)];
        while let Some((identity, child_index)) = stack.last_mut() {
            let children = by_base
                .get(identity.as_str())
                .map_or(&[][..], Vec::as_slice);
            let Some(declaration) = children.get(*child_index).copied() else {
                let (completed, _) = stack.pop()?;
                active.remove(&completed);
                continue;
            };
            *child_index += 1;
            if active.contains(&declaration.identity) {
                return None;
            }
            if stack.len() >= MAX_TYPE_DERIVATION_DEPTH {
                return None;
            }
            if !visited.insert(declaration.identity.clone()) {
                continue;
            }
            if !declaration.abstract_type {
                descendants.push(declaration.clone());
            }
            active.insert(declaration.identity.clone());
            stack.push((declaration.identity.clone(), 0));
        }
        Some(descendants)
    }
}

fn complex_type_is_abstract(
    schema_el: &Node,
    schema_path: &Path,
    qname: &str,
    state: &mut ParseState,
) -> bool {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "complexType", local)
    {
        return declaration
            .attribute("abstract")
            .is_some_and(|value| matches!(value, "true" | "1"));
    }
    let Some(path) = state.find_external_declaration(schema_el, schema_path, "complexType", qname)
    else {
        return false;
    };
    let Ok(text) = read_xml_text(&path) else {
        return false;
    };
    let Ok(document) = roxmltree::Document::parse(&text) else {
        return false;
    };
    top_level(&document.root_element(), "complexType", local)
        .and_then(|declaration| declaration.attribute("abstract"))
        .is_some_and(|value| matches!(value, "true" | "1"))
}

fn collect_derived_type_declarations(
    schema_el: &Node,
    schema_path: &Path,
    inherited_namespace: Option<&str>,
    visited: &mut BTreeSet<(PathBuf, Option<String>)>,
    index: &mut DerivedTypeIndex,
) {
    if index.limit_reached {
        return;
    }
    let path = normalized_path(schema_path);
    let effective_namespace = schema_el
        .attribute("targetNamespace")
        .or(inherited_namespace);
    if !visited.insert((path.clone(), effective_namespace.map(str::to_string))) {
        return;
    }
    for declaration in schema_el
        .children()
        .filter(|candidate| candidate.is_element() && candidate.tag_name().name() == "complexType")
    {
        let Some(local) = declaration.attribute("name") else {
            continue;
        };
        let Some(base) = direct_extension_base(&declaration) else {
            continue;
        };
        let Some(base_identity) = expanded_qname_identity(schema_el, effective_namespace, &base)
        else {
            continue;
        };
        let Some(identity) = type_identity_in_namespace(effective_namespace, local) else {
            continue;
        };
        index.insert(DerivedTypeDeclaration {
            path: path.clone(),
            local: local.to_string(),
            identity,
            base_identity,
            abstract_type: declaration
                .attribute("abstract")
                .is_some_and(|value| matches!(value, "true" | "1")),
        });
        if index.limit_reached {
            return;
        }
    }

    for link in schema_el
        .children()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "include" | "import"))
    {
        let Some(location) = link.attribute("schemaLocation") else {
            continue;
        };
        if location.contains("://") {
            continue;
        }
        let dependency = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(location);
        let Ok(text) = read_xml_text(&dependency) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let dependency_schema = document.root_element();
        let dependency_inherited = (link.tag_name().name() == "include")
            .then_some(effective_namespace)
            .flatten();
        collect_derived_type_declarations(
            &dependency_schema,
            &dependency,
            dependency_inherited,
            visited,
            index,
        );
    }
}

fn expanded_qname_identity(
    schema_el: &Node,
    effective_namespace: Option<&str>,
    qname: &str,
) -> Option<String> {
    if let Some((namespace, local)) = expanded_name(qname) {
        return (!namespace.is_empty() && !local.is_empty())
            .then(|| format!("{{{namespace}}}{local}"));
    }
    match qname.split_once(':') {
        Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() => schema_el
            .lookup_namespace_uri(Some(prefix))
            .filter(|namespace| !namespace.is_empty())
            .map(|namespace| format!("{{{namespace}}}{local}")),
        Some(_) => None,
        None => type_identity_in_namespace(effective_namespace, qname),
    }
}

fn type_identity_in_namespace(namespace: Option<&str>, local: &str) -> Option<String> {
    if local.is_empty() {
        return None;
    }
    Some(
        namespace
            .filter(|namespace| !namespace.is_empty())
            .map_or_else(
                || local.to_string(),
                |namespace| format!("{{{namespace}}}{local}"),
            ),
    )
}

fn direct_extension_base(declaration: &Node<'_, '_>) -> Option<String> {
    declaration
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "complexContent")?
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "extension")?
        .attribute("base")
        .map(str::to_string)
}

/// Finds a named top-level declaration (`xs:complexType name=..` etc.).
fn top_level<'a>(schema_el: &Node<'a, 'a>, tag: &str, name: &str) -> Option<Node<'a, 'a>> {
    schema_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == tag && n.attribute("name") == Some(name))
}

fn resolve_element(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "element", local)
    {
        return parse_element_declaration(&declaration, schema_el, schema_path, local, state);
    }

    let path = state.find_external_declaration(schema_el, schema_path, "element", qname)?;
    let text = read_xml_text(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let external_schema = doc.root_element();
    let declaration = top_level(&external_schema, "element", local)?;
    parse_element_declaration(&declaration, &external_schema, &path, local, state)
}

fn parse_element_declaration(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    parse_element_declaration_inner(declaration, schema_el, schema_path, name, state, true)
}

fn parse_element_declaration_inner(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
    attach_substitutions: bool,
) -> Option<SchemaNode> {
    if !state.enter(schema_path, "element", name) {
        let mut node = SchemaNode::recursive_group(name, name);
        node.xml_namespace = declaration_namespace(declaration, schema_el, false);
        return Some(node);
    }
    let mut node = parse_element(declaration, schema_el, schema_path, state);
    if attach_substitutions
        && let Err(error) = attach_substitution_alternatives(&mut node, declaration, state)
    {
        state.reject_substitution(error);
    }
    state.leave();
    Some(node)
}

fn attach_substitution_alternatives(
    node: &mut SchemaNode,
    declaration: &Node<'_, '_>,
    state: &mut ParseState,
) -> Result<(), XmlFormatError> {
    let head = schema_node_identity(node);
    let abstract_head = declaration
        .attribute("abstract")
        .is_some_and(|value| matches!(value, "true" | "1"));
    if declaration.attribute("block").is_some_and(|value| {
        value
            .split_ascii_whitespace()
            .any(|token| matches!(token, "substitution" | "#all"))
    }) {
        return if abstract_head {
            Err(XmlFormatError::UnsupportedSubstitutionGroup {
                head: head.clone(),
                member: head,
                reason: "an abstract head blocks every substitution",
            })
        } else {
            Ok(())
        };
    }
    let descendants = state.substitutions.concrete_descendants(&head)?;
    if descendants.is_empty() {
        return if abstract_head {
            Err(XmlFormatError::UnsupportedSubstitutionGroup {
                head: head.clone(),
                member: head,
                reason: "an abstract head has no concrete substitution member",
            })
        } else {
            Ok(())
        };
    }
    if node.recursive_ref.is_some() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "recursive substitution heads are not supported",
        ));
    }
    let SchemaKind::Group {
        children: head_children,
        alternatives: head_alternatives,
        dynamic,
    } = &node.kind
    else {
        return Err(unsupported_substitution(
            &head,
            &head,
            "only complex element declarations are supported",
        ));
    };
    if dynamic.is_some() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "substitution groups cannot also carry dynamic alternatives",
        ));
    }
    if !abstract_head && !head_alternatives.is_empty() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "substitution heads with xsi:type alternatives are not supported",
        ));
    }
    if node.nillable {
        return Err(unsupported_substitution(
            &head,
            &head,
            "nillable complex substitution heads are not executable",
        ));
    }
    let original_children = if head_alternatives.is_empty() {
        head_children.clone()
    } else {
        head_children
            .iter()
            .filter(|child| {
                head_alternatives
                    .iter()
                    .all(|alternative| alternative.members.contains(&child.name))
            })
            .cloned()
            .collect()
    };
    let original_sequences = node.xml_repeating_sequences.clone();
    let mut merged = original_children.clone();
    let mut alternatives = Vec::with_capacity(descendants.len() + usize::from(!abstract_head));
    if !abstract_head {
        alternatives.push(ir::GroupAlternative {
            name: head.clone(),
            members: original_children
                .iter()
                .map(|child| child.name.clone())
                .collect(),
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }

    for descendant in descendants {
        let text = read_xml_text(&descendant.path)?;
        let document = roxmltree::Document::parse(&text)?;
        let member_schema = document.root_element();
        let member_declaration = top_level(&member_schema, "element", &descendant.local)
            .ok_or_else(|| XmlFormatError::MissingElement(descendant.identity.clone()))?;
        let member = parse_element_declaration_inner(
            &member_declaration,
            &member_schema,
            &descendant.path,
            &descendant.local,
            state,
            false,
        )
        .ok_or_else(|| XmlFormatError::MissingElement(descendant.identity.clone()))?;
        let SchemaKind::Group {
            children: member_children,
            alternatives: member_alternatives,
            dynamic: member_dynamic,
        } = member.kind
        else {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "only complex element declarations are supported",
            ));
        };
        if member.recursive_ref.is_some()
            || !member_alternatives.is_empty()
            || member_dynamic.is_some()
        {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "recursive, dynamic, or type-alternative members are not supported",
            ));
        }
        if member.nillable
            || member.fixed != node.fixed
            || member.default != node.default
            || member.xml_repeating_sequences != original_sequences
        {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "member occurrence metadata differs from the head",
            ));
        }
        let members = member_children
            .iter()
            .map(|child| child.name.clone())
            .collect::<Vec<_>>();
        for child in member_children {
            if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
                if existing != &child {
                    return Err(unsupported_substitution(
                        &head,
                        &descendant.identity,
                        "same-named member fields have incompatible schemas",
                    ));
                }
            } else {
                merged.push(child);
            }
        }
        alternatives.push(ir::GroupAlternative {
            name: descendant.identity,
            members,
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }

    {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return Err(unsupported_substitution(
                &head,
                &head,
                "head shape changed during substitution expansion",
            ));
        };
        *children = merged;
    }
    if !node.set_substitution_group_alternatives(alternatives) {
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            *children = original_children;
        }
        return Err(unsupported_substitution(
            &head,
            &head,
            "member projections have inconsistent alternative metadata",
        ));
    }
    Ok(())
}

fn schema_node_identity(node: &SchemaNode) -> String {
    match &node.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => {
            format!("{{{}}}{}", namespace.as_str(), node.name)
        }
        Some(XmlNamespace::Unqualified) | None => node.name.clone(),
    }
}

fn unsupported_substitution(head: &str, member: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedSubstitutionGroup {
        head: head.to_string(),
        member: member.to_string(),
        reason,
    }
}

fn resolve_complex_type(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
) -> Option<ComplexTypeResolution> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "complexType", local)
    {
        return parse_complex_type_declaration(
            &declaration,
            schema_el,
            schema_path,
            local,
            state,
            occurrence_anchor,
        );
    }

    let path = state.find_external_declaration(schema_el, schema_path, "complexType", qname)?;
    let text = read_xml_text(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let external_schema = doc.root_element();
    let declaration = top_level(&external_schema, "complexType", local)?;
    parse_complex_type_declaration(
        &declaration,
        &external_schema,
        &path,
        local,
        state,
        occurrence_anchor,
    )
}

fn parse_complex_type_declaration(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
) -> Option<ComplexTypeResolution> {
    let identity = ParseState::declaration(schema_path, "complexType", name);
    if let Some(cached) = state.complex_types.get(&identity).cloned() {
        let anchor = occurrence_anchor.unwrap_or(&cached.anchor);
        let mut group = cached.group;
        rebase_recursive_anchor(&mut group.children, &cached.anchor, anchor);
        return state
            .reserve_elements(group.children.iter().map(schema_node_count).sum())
            .then_some(ComplexTypeResolution::Group(group));
    }
    if !state.enter(schema_path, "complexType", name) {
        let anchor = state
            .complex_type_anchors
            .last()
            .map_or_else(|| name.to_string(), Clone::clone);
        return Some(ComplexTypeResolution::Recursive(anchor));
    }
    let anchor = occurrence_anchor
        .map(str::to_string)
        .or_else(|| state.complex_type_anchors.last().cloned())
        .unwrap_or_else(|| name.to_string());
    state.complex_type_anchors.push(anchor.clone());
    let group = parse_complex_type(declaration, schema_el, schema_path, state);
    state.complex_type_anchors.pop();
    state.leave();
    state.complex_types.insert(
        identity,
        CachedComplexType {
            group: group.clone(),
            anchor,
        },
    );
    Some(ComplexTypeResolution::Group(group))
}

fn rebase_recursive_anchor(children: &mut [SchemaNode], from: &str, to: &str) {
    if from == to {
        return;
    }
    for child in children {
        if child.recursive_ref.as_deref() == Some(from) {
            child.recursive_ref = Some(to.to_string());
        }
        if let ir::SchemaKind::Group { children, .. } = &mut child.kind {
            rebase_recursive_anchor(children, from, to);
        }
    }
}

fn schema_node_count(node: &SchemaNode) -> usize {
    1 + match &node.kind {
        ir::SchemaKind::Scalar { .. } => 0,
        ir::SchemaKind::Group { children, .. } => children.iter().map(schema_node_count).sum(),
    }
}

fn resolve_simple_type(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<ScalarType> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "simpleType", local)
    {
        return Some(simple_type_scalar(&declaration));
    }

    let path = state.find_external_declaration(schema_el, schema_path, "simpleType", qname)?;
    let text = read_xml_text(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    top_level(&doc.root_element(), "simpleType", local)
        .map(|declaration| simple_type_scalar(&declaration))
}

fn local_name(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

fn expanded_name(qname: &str) -> Option<(&str, &str)> {
    qname.strip_prefix('{')?.split_once('}')
}

fn is_local_qname(schema_el: &Node, qname: &str) -> bool {
    let Some((prefix, _)) = qname.split_once(':') else {
        return true;
    };
    schema_el.lookup_namespace_uri(Some(prefix)) == schema_el.attribute("targetNamespace")
}

/// Finds the local schema file containing a top-level declaration reached
/// through `xs:include` or a namespace-matching `xs:import`.
fn find_external_declaration(
    schema_el: &Node,
    schema_path: &Path,
    tag: &str,
    qname: &str,
) -> Option<PathBuf> {
    let expanded = expanded_name(qname);
    let wanted_namespace = expanded
        .map(|(namespace, _)| namespace)
        .or_else(|| {
            qname
                .split_once(':')
                .and_then(|(prefix, _)| schema_el.lookup_namespace_uri(Some(prefix)))
        })
        .map(str::to_string);
    let effective_namespace = schema_el.attribute("targetNamespace").map(str::to_string);
    let mut visited = BTreeSet::new();
    visited.insert(normalized_path(schema_path));
    search_dependencies(
        schema_el,
        schema_path,
        tag,
        expanded.map_or(local_name(qname), |(_, local)| local),
        wanted_namespace.as_deref(),
        effective_namespace.as_deref(),
        &mut visited,
    )
}

fn search_dependencies(
    schema_el: &Node,
    schema_path: &Path,
    tag: &str,
    name: &str,
    wanted_namespace: Option<&str>,
    effective_namespace: Option<&str>,
    visited: &mut BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    for link in schema_el
        .children()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "include" | "import"))
    {
        let is_import = link.tag_name().name() == "import";
        if is_import {
            let Some(wanted) = wanted_namespace else {
                continue;
            };
            if link
                .attribute("namespace")
                .is_some_and(|namespace| namespace != wanted)
            {
                continue;
            }
        }

        let Some(location) = link.attribute("schemaLocation") else {
            continue;
        };
        if location.contains("://") {
            continue;
        }
        let path = schema_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(location);
        let inherited_namespace = (!is_import).then_some(effective_namespace).flatten();
        if let Some(found) = search_schema_file(
            &path,
            tag,
            name,
            wanted_namespace,
            inherited_namespace,
            visited,
        ) {
            return Some(found);
        }
    }
    None
}

fn search_schema_file(
    schema_path: &Path,
    tag: &str,
    name: &str,
    wanted_namespace: Option<&str>,
    inherited_namespace: Option<&str>,
    visited: &mut BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    let path = normalized_path(schema_path);
    if !visited.insert(path.clone()) {
        return None;
    }
    let text = read_xml_text(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let schema_el = doc.root_element();
    let declared_namespace = schema_el.attribute("targetNamespace");
    let effective_namespace = declared_namespace.or(inherited_namespace);

    if wanted_namespace.is_none_or(|wanted| effective_namespace == Some(wanted))
        && top_level(&schema_el, tag, name).is_some()
    {
        return Some(path);
    }

    search_dependencies(
        &schema_el,
        &path,
        tag,
        name,
        wanted_namespace,
        effective_namespace,
        visited,
    )
}

fn normalized_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The scalar type of a named simpleType: its restriction's base.
fn simple_type_scalar(simple_type: &Node) -> ScalarType {
    simple_type
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "restriction")
        .and_then(|r| r.attribute("base"))
        .map(map_xsd_type)
        .unwrap_or(ScalarType::String)
}

fn parse_complex_type(
    complex_type: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> ParsedComplexType {
    let mut parsed = ParsedComplexType::default();
    if complex_type.attribute("mixed") == Some("true") {
        parsed
            .children
            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    }
    for child in complex_type.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "sequence" | "choice" | "all" => {
                if child.tag_name().name() == "sequence" && is_repeating(&child) {
                    match repeating_sequence(&child) {
                        Ok(Some(sequence)) => parsed.repeating_sequences.push(sequence),
                        Ok(None) => {}
                        Err(error) => state.reject_repeating_particle(error),
                    }
                }
                collect_sequence(
                    &child,
                    is_repeating(&child),
                    schema_el,
                    schema_path,
                    state,
                    &mut parsed.children,
                );
            }
            // complexContent/extension: the named base type's children
            // first, then whatever the extension adds.
            "complexContent" => {
                if let Some(ext) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "extension")
                {
                    if let Some(base) = ext.attribute("base")
                        && let Some(ComplexTypeResolution::Group(base_group)) =
                            resolve_complex_type(base, schema_el, schema_path, state, None)
                    {
                        parsed.extend(base_group);
                    }
                    parsed.extend(parse_complex_type(&ext, schema_el, schema_path, state));
                } else if let Some(restriction) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "restriction")
                    && let Some(base) = restriction.attribute("base")
                    && let Some(ComplexTypeResolution::Group(base_group)) =
                        resolve_complex_type(base, schema_el, schema_path, state, None)
                {
                    match restriction::apply(
                        base,
                        base_group,
                        &restriction,
                        schema_el,
                        schema_path,
                        state,
                    ) {
                        Ok(group) => parsed.extend(group),
                        Err(error) => state.reject_restriction(error),
                    }
                }
            }
            "simpleContent" => {
                if let Some(content) = child.children().find(|node| {
                    node.is_element()
                        && matches!(node.tag_name().name(), "extension" | "restriction")
                }) {
                    let mut resolved_base = false;
                    if let Some(base) = content.attribute("base") {
                        if let Some(ComplexTypeResolution::Group(base_group)) =
                            resolve_complex_type(base, schema_el, schema_path, state, None)
                        {
                            parsed.extend(base_group);
                            resolved_base = true;
                        } else {
                            let ty = resolve_simple_type(base, schema_el, schema_path, state)
                                .unwrap_or_else(|| map_xsd_type(base));
                            parsed
                                .children
                                .push(SchemaNode::scalar(XML_TEXT_FIELD, ty).text());
                            resolved_base = true;
                        }
                    }
                    if !resolved_base {
                        parsed
                            .children
                            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
                    }
                    parsed.extend(parse_complex_type(&content, schema_el, schema_path, state));
                }
            }
            _ => {}
        }
    }
    for attr in complex_type.children().filter(|node| {
        node.is_element() && matches!(node.tag_name().name(), "attribute" | "attributeGroup")
    }) {
        match attr.tag_name().name() {
            "attribute" if attr.attribute("use") == Some("prohibited") => continue,
            "attribute" => {
                let attribute = parse_attribute(&attr, schema_el, schema_path, state);
                parsed.children.push(attribute);
            }
            "attributeGroup" => {
                match groups::resolve_attribute_group(&attr, schema_el, schema_path, state) {
                    Ok(attributes) => parsed.children.extend(attributes),
                    Err(error) => state.reject_schema_group(error),
                }
            }
            _ => {}
        }
    }
    parsed
}

fn parse_attribute(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> SchemaNode {
    if declaration.attribute("name").is_none()
        && let Some(reference) = declaration.attribute("ref")
    {
        if let Some(mut attribute) = resolve_attribute(reference, schema, schema_path, state) {
            if let Some(fixed) = declaration.attribute("fixed") {
                attribute.fixed = Some(fixed.to_string());
            }
            if let Some(default) = declaration.attribute("default") {
                attribute.default = Some(default.to_string());
            }
            return attribute;
        }
        let mut attribute =
            SchemaNode::scalar(local_name(reference), ScalarType::String).attribute();
        attribute.xml_namespace =
            qname_namespace(declaration, reference).or(Some(XmlNamespace::Unqualified));
        return attribute;
    }

    let name = declaration.attribute("name").unwrap_or_default();
    let ty = declaration
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "simpleType")
        .map(|simple| simple_type_scalar(&simple))
        .or_else(|| {
            declaration.attribute("type").map(|ty| {
                resolve_simple_type(ty, schema, schema_path, state)
                    .unwrap_or_else(|| map_xsd_type(ty))
            })
        })
        .unwrap_or(ScalarType::String);
    let mut attribute = SchemaNode::scalar(name, ty).attribute();
    attribute.xml_namespace = declaration_namespace(declaration, schema, true);
    if let Some(fixed) = declaration.attribute("fixed") {
        attribute.fixed = Some(fixed.to_string());
    }
    if let Some(default) = declaration.attribute("default") {
        attribute.default = Some(default.to_string());
    }
    attribute
}

fn resolve_attribute(
    qname: &str,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    let local = local_name(qname);
    if is_local_qname(schema, qname)
        && let Some(declaration) = top_level(schema, "attribute", local)
    {
        return Some(parse_attribute(&declaration, schema, schema_path, state));
    }
    let path = state.find_external_declaration(schema, schema_path, "attribute", qname)?;
    let text = read_xml_text(&path).ok()?;
    let document = roxmltree::Document::parse(&text).ok()?;
    let external_schema = document.root_element();
    let declaration = top_level(&external_schema, "attribute", local)?;
    Some(parse_attribute(
        &declaration,
        &external_schema,
        &path,
        state,
    ))
}

fn repeating_sequence(
    sequence: &Node<'_, '_>,
) -> Result<Option<XmlRepeatingSequence>, XmlFormatError> {
    if let Some(compositor) = nested_non_sequence_compositor(sequence) {
        return Err(XmlFormatError::UnsupportedRepeatingSequenceCompositor { compositor });
    }
    if let Some(element_count) = nested_repeating_multi_member_sequence(sequence) {
        return Err(XmlFormatError::UnsupportedNestedRepeatingSequence { element_count });
    }
    let mut members = Vec::new();
    collect_sequence_members(sequence, true, false, false, &mut members);
    if members.len() < 2 {
        return Ok(None);
    }
    let mut names = BTreeSet::new();
    if !members
        .iter()
        .all(|member| names.insert(member.name.as_str()))
    {
        return Err(XmlFormatError::UnsupportedRepeatingParticle {
            compositor: "sequence".to_string(),
            element_count: members.len(),
        });
    }
    Ok(Some(XmlRepeatingSequence {
        required: sequence.attribute("minOccurs") != Some("0"),
        members,
    }))
}

fn nested_non_sequence_compositor(particle: &Node<'_, '_>) -> Option<String> {
    for child in particle.children().filter(|node| node.is_element()) {
        if is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "choice" | "all" => return Some(child.tag_name().name().to_string()),
            "sequence" => {
                if let Some(compositor) = nested_non_sequence_compositor(&child) {
                    return Some(compositor);
                }
            }
            // Element-local complex types own a separate particle tree.
            "element" => {}
            _ => {}
        }
    }
    None
}

fn nested_repeating_multi_member_sequence(particle: &Node<'_, '_>) -> Option<usize> {
    for child in particle.children().filter(|node| node.is_element()) {
        if is_disabled_particle(&child) || child.tag_name().name() == "element" {
            continue;
        }
        if child.tag_name().name() == "sequence" && is_repeating(&child) {
            let mut members = Vec::new();
            collect_sequence_members(&child, true, false, false, &mut members);
            if members.len() > 1 {
                return Some(members.len());
            }
        }
        if let Some(count) = nested_repeating_multi_member_sequence(&child) {
            return Some(count);
        }
    }
    None
}

fn invalid_repeating_sequence_group(schema: &SchemaNode) -> Option<&str> {
    if !schema.xml_repeating_sequences_are_valid() {
        return Some(&schema.name);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children.iter().find_map(invalid_repeating_sequence_group)
}

fn collect_sequence_members(
    particle: &Node<'_, '_>,
    root: bool,
    inherited_optional: bool,
    inherited_repeating: bool,
    out: &mut Vec<XmlSequenceMember>,
) {
    let optional = inherited_optional || (!root && particle.attribute("minOccurs") == Some("0"));
    let repeating = inherited_repeating || (!root && is_repeating(particle));
    for child in particle.children().filter(|node| node.is_element()) {
        if is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "element" => out.push(XmlSequenceMember {
                name: child
                    .attribute("name")
                    .or_else(|| child.attribute("ref").map(local_name))
                    .unwrap_or_default()
                    .to_string(),
                required: !optional && child.attribute("minOccurs") != Some("0"),
                repeating: repeating || is_repeating(&child),
            }),
            "sequence" | "choice" | "all" => {
                collect_sequence_members(&child, false, optional, repeating, out);
            }
            _ => {}
        }
    }
}

/// Recursively walks an `xs:sequence`, collecting the elements it declares.
/// `inherited_repeating` is `true` when an *enclosing* sequence is itself
/// repeating (the "wrap a single element in a repeating sequence" idiom) --
/// it gets propagated onto that sequence's own element(s).
fn collect_sequence(
    sequence: &Node,
    inherited_repeating: bool,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    out: &mut Vec<SchemaNode>,
) {
    if is_disabled_particle(sequence) {
        return;
    }
    for child in sequence.children().filter(|n| n.is_element()) {
        if is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "element" => {
                if !state.has_element_capacity() {
                    return;
                }
                let mut node = parse_element(&child, schema_el, schema_path, state);
                node.repeating = inherited_repeating || is_repeating(&child);
                out.push(node);
            }
            "sequence" | "choice" | "all" => {
                collect_sequence(
                    &child,
                    inherited_repeating || is_repeating(&child),
                    schema_el,
                    schema_path,
                    state,
                    out,
                );
            }
            "group" => match groups::resolve_model_group(&child, schema_el, schema_path, state) {
                Ok(children) => out.extend(children),
                Err(error) => state.reject_schema_group(error),
            },
            _ => {}
        }
    }
}

fn is_disabled_particle(particle: &Node) -> bool {
    particle.attribute("maxOccurs").is_some_and(|value| {
        let digits = value.strip_prefix('+').unwrap_or(value);
        !digits.is_empty()
            && digits.bytes().all(|digit| digit.is_ascii_digit())
            && digits.bytes().all(|digit| digit == b'0')
    })
}

fn is_repeating(el: &Node) -> bool {
    match el.attribute("maxOccurs") {
        Some("unbounded") => true,
        Some(value) => non_negative_integer_exceeds_one(value),
        None => false,
    }
}

fn non_negative_integer_exceeds_one(value: &str) -> bool {
    let digits = value.strip_prefix('+').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
        return false;
    }
    let significant = digits.trim_start_matches('0');
    significant.len() > 1
        || significant
            .as_bytes()
            .first()
            .is_some_and(|digit| *digit > b'1')
}

fn xsd_type_name(ty: &ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "xs:string",
        ScalarType::Int => "xs:integer",
        ScalarType::Float => "xs:decimal",
        ScalarType::Bool => "xs:boolean",
    }
}

fn map_xsd_type(ty: &str) -> ScalarType {
    match ty.rsplit(':').next().unwrap_or(ty) {
        "int" | "integer" | "long" | "short" | "byte" | "unsignedInt" | "unsignedLong"
        | "unsignedShort" | "unsignedByte" | "negativeInteger" | "positiveInteger"
        | "nonNegativeInteger" | "nonPositiveInteger" => ScalarType::Int,
        "decimal" | "double" | "float" => ScalarType::Float,
        "boolean" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

#[cfg(test)]
mod tests;
