//! A deliberately small XSD importer: enough to turn the common
//! `xs:element` / `xs:complexType` / `xs:sequence` shapes into a
//! [`SchemaNode`] tree, including the "wrap a single element in an
//! `xs:sequence maxOccurs="unbounded"`" idiom real-world schemas use for
//! repeating groups. `xs:attribute` declarations directly under a
//! `xs:complexType` (or its `complexContent` extension) become
//! attribute-flagged scalar children; `xs:element ref="..."`, named
//! top-level complex/simple types, and `complexContent`/`xs:extension`
//! resolve across local `xs:include` and `xs:import` schema locations
//! (recursive references and include cycles degrade safely); `xs:choice`
//! and `xs:all` import as if they were sequences (every branch becomes a
//! child -- ferrule has no exclusivity concept). `xs:simpleContent` becomes
//! a `#text` scalar plus attribute scalars. Repeating `xs:sequence` particles
//! with more than one element member are rejected because flattening them
//! would lose tuple association. It does not support unions, `xs:any`, or
//! remote schema URLs -- that's the "lite" in the name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD};
use roxmltree::Node;

use crate::XmlFormatError;

const MAX_MATERIALIZED_SCHEMA_ELEMENTS: usize = 256;

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
    complex_types: BTreeMap<ActiveDeclaration, Vec<SchemaNode>>,
    declaration_paths: BTreeMap<DeclarationQuery, Option<PathBuf>>,
    materialized_elements: usize,
    materialization_limit_reached: bool,
    unsupported_particle: Option<XmlFormatError>,
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

    fn reject_repeating_particle(&mut self, compositor: &str, element_count: usize) {
        self.unsupported_particle
            .get_or_insert(XmlFormatError::UnsupportedRepeatingParticle {
                compositor: compositor.to_string(),
                element_count,
            });
    }

    fn finish(self, schema: SchemaNode) -> Result<SchemaNode, XmlFormatError> {
        match self.unsupported_particle {
            Some(error) => Err(error),
            None if self.materialization_limit_reached => {
                Err(XmlFormatError::SchemaMaterializationLimit {
                    limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
                })
            }
            None => Ok(schema),
        }
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
    let mut chunks = bytes.chunks_exact(2);
    let units = chunks
        .by_ref()
        .map(|chunk| decode([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    if !chunks.remainder().is_empty() {
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
        let mut state = ParseState::default();
        let schema = parse_element(&root_element, &schema_el, path, &mut state);
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
            let mut state = ParseState::default();
            let schema = parse_element(&root_element, &external_schema, &external_path, &mut state);
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
    let mut state = ParseState::default();
    let (namespace, local) = type_name
        .strip_prefix('{')
        .and_then(|name| name.split_once('}'))
        .map_or((None, local_name(type_name)), |(namespace, local)| {
            (Some(namespace), local)
        });
    let children = match namespace {
        None => resolve_complex_type(type_name, &schema_el, path, &mut state),
        Some(namespace) if schema_el.attribute("targetNamespace") == Some(namespace) => {
            resolve_complex_type(local, &schema_el, path, &mut state)
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
                )
            })
        }
    }
    .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:complexType `{type_name}`")))?;
    state.finish(SchemaNode::group(local, children))
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
        // Unresolvable or recursive reference: degrade to a string scalar.
        return SchemaNode::scalar(local, ScalarType::String);
    }
    let name = el.attribute("name").unwrap_or_default().to_string();
    let node = if let Some(complex_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "complexType")
    {
        SchemaNode::group(
            name,
            parse_complex_type(&complex_type, schema_el, schema_path, state),
        )
    } else if let Some(simple_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "simpleType")
    {
        SchemaNode::scalar(name, simple_type_scalar(&simple_type))
    } else if let Some(ty) = el.attribute("type") {
        if let Some(children) = resolve_complex_type(ty, schema_el, schema_path, state) {
            SchemaNode::group(name, children)
        } else if let Some(ty) = resolve_simple_type(ty, schema_el, schema_path, state) {
            SchemaNode::scalar(name, ty)
        } else {
            SchemaNode::scalar(name, map_xsd_type(ty))
        }
    } else {
        SchemaNode::scalar(name, ScalarType::String)
    };
    if el
        .attribute("nillable")
        .is_some_and(|value| matches!(value, "true" | "1"))
    {
        node.nillable()
    } else {
        node
    }
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
    if !state.enter(schema_path, "element", name) {
        return None;
    }
    let node = parse_element(declaration, schema_el, schema_path, state);
    state.leave();
    Some(node)
}

fn resolve_complex_type(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<Vec<SchemaNode>> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "complexType", local)
    {
        return parse_complex_type_declaration(&declaration, schema_el, schema_path, local, state);
    }

    let path = state.find_external_declaration(schema_el, schema_path, "complexType", qname)?;
    let text = read_xml_text(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let external_schema = doc.root_element();
    let declaration = top_level(&external_schema, "complexType", local)?;
    parse_complex_type_declaration(&declaration, &external_schema, &path, local, state)
}

fn parse_complex_type_declaration(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
) -> Option<Vec<SchemaNode>> {
    let identity = ParseState::declaration(schema_path, "complexType", name);
    if let Some(children) = state.complex_types.get(&identity).cloned() {
        return state
            .reserve_elements(children.iter().map(schema_node_count).sum())
            .then_some(children);
    }
    if !state.enter(schema_path, "complexType", name) {
        return None;
    }
    let children = parse_complex_type(declaration, schema_el, schema_path, state);
    state.leave();
    state.complex_types.insert(identity, children.clone());
    Some(children)
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
) -> Vec<SchemaNode> {
    let mut children = Vec::new();
    if complex_type.attribute("mixed") == Some("true") {
        children.push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    }
    for child in complex_type.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "sequence" | "choice" | "all" => {
                collect_sequence(
                    &child,
                    is_repeating(&child),
                    schema_el,
                    schema_path,
                    state,
                    &mut children,
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
                        && let Some(base_children) =
                            resolve_complex_type(base, schema_el, schema_path, state)
                    {
                        children.extend(base_children);
                    }
                    children.extend(parse_complex_type(&ext, schema_el, schema_path, state));
                }
            }
            "simpleContent" => {
                if let Some(content) = child.children().find(|node| {
                    node.is_element()
                        && matches!(node.tag_name().name(), "extension" | "restriction")
                }) {
                    let mut resolved_base = false;
                    if let Some(base) = content.attribute("base") {
                        if let Some(base_children) =
                            resolve_complex_type(base, schema_el, schema_path, state)
                        {
                            children.extend(base_children);
                            resolved_base = true;
                        } else {
                            let ty = resolve_simple_type(base, schema_el, schema_path, state)
                                .unwrap_or_else(|| map_xsd_type(base));
                            children.push(SchemaNode::scalar(XML_TEXT_FIELD, ty).text());
                            resolved_base = true;
                        }
                    }
                    if !resolved_base {
                        children
                            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
                    }
                    children.extend(parse_complex_type(&content, schema_el, schema_path, state));
                }
            }
            _ => {}
        }
    }
    for attr in complex_type
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "attribute")
    {
        if attr.attribute("use") == Some("prohibited") {
            continue;
        }
        let name = attr.attribute("name").unwrap_or_default().to_string();
        let ty = attr
            .attribute("type")
            .map(map_xsd_type)
            .unwrap_or(ScalarType::String);
        children.push(SchemaNode::scalar(name, ty).attribute());
    }
    children
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
    if sequence.tag_name().name() == "sequence" && is_repeating(sequence) {
        let element_count = particle_element_count(sequence);
        if element_count > 1 {
            state.reject_repeating_particle(sequence.tag_name().name(), element_count);
            return;
        }
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
            _ => {}
        }
    }
}

/// Counts element particles without descending into an element's own type.
/// A repeating compositor is losslessly flattenable only when this is one:
/// otherwise the IR would turn its associated tuple into independent arrays.
fn particle_element_count(particle: &Node) -> usize {
    if is_disabled_particle(particle) {
        return 0;
    }
    particle
        .children()
        .filter(|node| node.is_element())
        .map(|child| {
            if is_disabled_particle(&child) {
                return 0;
            }
            match child.tag_name().name() {
                "element" => 1,
                "sequence" | "choice" | "all" => particle_element_count(&child),
                _ => 0,
            }
        })
        .sum()
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

/// Renders a [`SchemaNode`] as XSD text -- the inverse of [`import`],
/// producing the same `xs:element`/`xs:complexType`/`xs:sequence` subset it
/// reads (repeating nodes get `maxOccurs="unbounded"`). Returns an error when
/// XML role flags describe mixed content or another shape this subset cannot
/// preserve.
pub fn export(schema: &SchemaNode) -> Result<String, XmlFormatError> {
    validate_export_node(schema, true)?;
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" elementFormDefault=\"qualified\">\n",
    );
    write_element(schema, 1, &mut out)?;
    out.push_str("</xs:schema>\n");
    Ok(out)
}

fn validate_export_node(node: &SchemaNode, is_root: bool) -> Result<(), XmlFormatError> {
    if node.attribute && node.text {
        return Err(XmlFormatError::ConflictingSchemaRoles {
            node: node.name.clone(),
        });
    }
    let role = if node.attribute {
        Some("attribute")
    } else if node.text {
        Some("text")
    } else {
        None
    };
    if let Some(role) = role {
        if is_root {
            return Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role,
                kind: "document root",
            });
        }
        if matches!(node.kind, ir::SchemaKind::Group { .. }) {
            return Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role,
                kind: "group",
            });
        }
        if node.repeating {
            return Err(XmlFormatError::RepeatingSchemaRole {
                node: node.name.clone(),
                role,
            });
        }
    }
    let ir::SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &node.kind
    else {
        return Ok(());
    };
    if !alternatives.is_empty() {
        return Err(XmlFormatError::UnsupportedGroupAlternatives {
            group: node.name.clone(),
        });
    }
    if node.name == XML_ELEMENTS_FIELD {
        return if node.repeating {
            Ok(())
        } else {
            Err(XmlFormatError::UnsupportedSchemaRole {
                node: node.name.clone(),
                role: "generic elements",
                kind: "non-repeating group",
            })
        };
    }
    for child in children {
        validate_export_node(child, false)?;
    }
    let text_count = children.iter().filter(|child| child.text).count();
    if text_count > 1 {
        return Err(XmlFormatError::MultipleTextFields {
            group: node.name.clone(),
            count: text_count,
        });
    }
    if text_count == 1 && children.iter().any(|child| !child.attribute && !child.text) {
        return Err(XmlFormatError::MixedContent {
            group: node.name.clone(),
        });
    }
    Ok(())
}

fn write_element(node: &SchemaNode, depth: usize, out: &mut String) -> Result<(), XmlFormatError> {
    let pad = "  ".repeat(depth);
    if node.name == XML_ELEMENTS_FIELD {
        out.push_str(&format!(
            "{pad}<xs:any minOccurs=\"0\" maxOccurs=\"unbounded\" processContents=\"lax\"/>\n"
        ));
        return Ok(());
    }
    let occurs = if node.repeating {
        " minOccurs=\"0\" maxOccurs=\"unbounded\""
    } else {
        ""
    };
    let nillable = if node.nillable {
        " nillable=\"true\""
    } else {
        ""
    };
    match &node.kind {
        ir::SchemaKind::Scalar { ty } => {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{}\"{occurs}{nillable}/>\n",
                node.name,
                xsd_type_name(ty)
            ));
        }
        ir::SchemaKind::Group { children, .. } => {
            // XSD requires attributes after the content model, so partition
            // on the fly; only scalar children can be attributes.
            let (attrs, elements): (Vec<_>, Vec<_>) =
                children.iter().partition(|child| child.attribute);
            let text = elements.iter().find(|child| child.text);
            let nested_elements: Vec<_> = elements
                .iter()
                .filter(|child| !child.text)
                .copied()
                .collect();
            if let Some(text) = text
                && nested_elements.is_empty()
            {
                let ir::SchemaKind::Scalar { ty } = &text.kind else {
                    return Err(XmlFormatError::UnsupportedSchemaRole {
                        node: text.name.clone(),
                        role: "text",
                        kind: "group",
                    });
                };
                out.push_str(&format!(
                    "{pad}<xs:element name=\"{}\"{occurs}{nillable}>\n{pad}  <xs:complexType>\n{pad}    <xs:simpleContent>\n{pad}      <xs:extension base=\"{}\">\n",
                    node.name,
                    xsd_type_name(ty)
                ));
                for attr in attrs {
                    let ir::SchemaKind::Scalar { ty } = &attr.kind else {
                        return Err(XmlFormatError::UnsupportedSchemaRole {
                            node: attr.name.clone(),
                            role: "attribute",
                            kind: "group",
                        });
                    };
                    out.push_str(&format!(
                        "{pad}        <xs:attribute name=\"{}\" type=\"{}\"/>\n",
                        attr.name,
                        xsd_type_name(ty)
                    ));
                }
                out.push_str(&format!(
                    "{pad}      </xs:extension>\n{pad}    </xs:simpleContent>\n{pad}  </xs:complexType>\n{pad}</xs:element>\n"
                ));
                return Ok(());
            }
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\"{occurs}{nillable}>\n{pad}  <xs:complexType>\n{pad}    <xs:sequence>\n",
                node.name
            ));
            for child in nested_elements {
                write_element(child, depth + 3, out)?;
            }
            out.push_str(&format!("{pad}    </xs:sequence>\n"));
            for attr in attrs {
                let ir::SchemaKind::Scalar { ty } = &attr.kind else {
                    return Err(XmlFormatError::UnsupportedSchemaRole {
                        node: attr.name.clone(),
                        role: "attribute",
                        kind: "group",
                    });
                };
                out.push_str(&format!(
                    "{pad}    <xs:attribute name=\"{}\" type=\"{}\"/>\n",
                    attr.name,
                    xsd_type_name(ty)
                ));
            }
            out.push_str(&format!("{pad}  </xs:complexType>\n{pad}</xs:element>\n"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "xsd_tests.rs"]
mod tests;
