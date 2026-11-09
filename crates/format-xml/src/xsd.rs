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
//! tuples. `xs:simpleContent` becomes a `#text` scalar plus attribute scalars;
//! named attribute-only extensions and restrictions participate in executable
//! `xsi:type` alternatives.
//! Optional, unbounded named model-group references that expand to exactly one
//! nonrepeating member flatten losslessly by making that member repeating.
//! `xs:any` imports optional/unbounded namespace predicates declared inline,
//! through a named model group, or inside an ordered choice. Lax wildcards
//! project resolved declarations as typed ports and retain undeclared names in
//! a namespace-aware recursive `element()` fallback; closed strict wildcards
//! contain only typed ports. `xs:anyAttribute` applies the same declaration-
//! first ownership rule beside ordinary attributes. Unknown strict attributes
//! reject at the runtime boundary rather than being treated as lax.
//! It does not support unions or remote schema URLs -- that's the "lite" in
//! the name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ir::{
    ScalarType, SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD, XmlNamespace,
    XmlRepeatingChoice, XmlRepeatingSequence, XmlWildcardProcessContents,
};
#[cfg(test)]
use ir::{SchemaKind, XML_LOCAL_NAME_FIELD, XML_TEXT_FIELD};
use roxmltree::Node;

use crate::XmlFormatError;

mod attributes;
mod complex_types;
mod decoding;
mod derived_types;
mod elements;
mod export;
mod groups;
mod particles;
mod resources;
mod restriction;
mod schema_links;
mod substitution;
mod substitution_expansion;
mod type_alternative_profiles;
mod wildcard_exclusions;
mod wildcard_ports;
mod wildcards;

use attributes::parse_attribute;
use complex_types::{parse_complex_type, parse_complex_type_declaration, resolve_complex_type};
pub use decoding::{decode_schema_bytes, read_text};
use derived_types::{expanded_qname_identity, type_identity_in_namespace};
use elements::{parse_element_declaration, parse_element_declaration_inner};
use particles::{collect_sequence, invalid_repeating_sequence_group};
use resources::SchemaReader;
pub use resources::XmlSchemaByteReader;
use schema_links::{Follow, LinkedSchema};
use substitution_expansion::attach_substitution_alternatives;
#[cfg(test)]
use wildcards::generic_wildcard_schema;
use wildcards::{
    generic_attribute_wildcard_schema, generic_attribute_wildcard_schema_with,
    parse_attribute_wildcard, qualify_wildcard_siblings, unsupported_wildcard,
};

pub use export::{XsdExportArtifact, XsdExportSet, export, export_namespace, export_set};

const MAX_MATERIALIZED_SCHEMA_ELEMENTS: usize = 4_096;
/// Most nesting and reference levels one XSD import may have open at once.
/// Each element declaration or reference, named complex type reference
/// (including a derivation base), complex type content, compositor, and
/// model or attribute group reference takes one level. That allows about 64
/// nested elements of named types, 85 of anonymous types, or 128 derivation
/// or group hops. The import recurses once per level, and the deepest shape
/// this allows uses about 3 MiB of stack in unoptimized and AddressSanitizer
/// builds, under half of a default 8 MiB main thread.
const MAX_IMPORT_DEPTH: usize = 256;
/// Bounds the iterative discovery of derived types below one base. Resolving
/// a derivation takes two import levels per hop, so a discovered type more
/// than about 128 hops below its base fails with the import depth limit.
const MAX_TYPE_DERIVATION_DEPTH: usize = 256;
const MAX_SCHEMA_BYTES: u64 = 64 * 1024 * 1024;
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
    reader: SchemaReader,
    active: Vec<ActiveDeclaration>,
    complex_types: BTreeMap<ActiveDeclaration, CachedComplexType>,
    complex_type_anchors: Vec<String>,
    declaration_paths: BTreeMap<DeclarationQuery, Option<PathBuf>>,
    materialized_elements: usize,
    materialization_limit_reached: bool,
    depth: usize,
    depth_limit_reached: bool,
    unsupported_particle: Option<XmlFormatError>,
    unsupported_default: Option<XmlFormatError>,
    unsupported_schema_group: Option<XmlFormatError>,
    unsupported_restriction: Option<XmlFormatError>,
    unsupported_alternative_occurrence: Option<XmlFormatError>,
    unsupported_attribute_use: Option<XmlFormatError>,
    unsupported_substitution: Option<XmlFormatError>,
    unsupported_wildcard: Option<XmlFormatError>,
    substitutions: substitution::SubstitutionIndex,
    /// Local names of element declarations left without a namespace because
    /// their document is reached through several namespace contexts.
    ambiguous_unqualified_elements: BTreeSet<String>,
}

enum ComplexTypeResolution {
    Group(ParsedComplexType),
    Recursive(String),
}

#[derive(Clone)]
struct CachedComplexType {
    group: ParsedComplexType,
    /// The content before projected wildcard ports were resolved, for
    /// derivations that resolve them against their own elements.
    content: ParsedComplexType,
    anchor: String,
}

/// How a named complex type is used.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ComplexTypeUse {
    /// As complete content, with projected wildcard ports resolved.
    Complete,
    /// As an extension base, with projected wildcard ports still marked.
    Base,
}

#[derive(Clone, Default)]
struct ParsedComplexType {
    children: Vec<SchemaNode>,
    repeating_sequences: Vec<XmlRepeatingSequence>,
    repeating_choices: Vec<XmlRepeatingChoice>,
}

impl ParsedComplexType {
    fn into_schema(self, name: impl Into<String>) -> SchemaNode {
        let mut schema = SchemaNode::group(name, self.children);
        schema.xml_repeating_sequences = self.repeating_sequences;
        schema.xml_repeating_choices = self.repeating_choices;
        schema
    }

    fn extend(&mut self, other: Self) {
        self.children.extend(other.children);
        self.repeating_sequences.extend(other.repeating_sequences);
        self.repeating_choices.extend(other.repeating_choices);
    }
}

impl ParseState {
    fn declaration(&self, path: &Path, kind: &'static str, name: &str) -> ActiveDeclaration {
        ActiveDeclaration {
            path: self.reader.identity(path),
            kind,
            name: name.to_string(),
        }
    }

    fn enter(&mut self, path: &Path, kind: &'static str, name: &str) -> bool {
        let declaration = self.declaration(path, kind, name);
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

    /// Runs `parse` one import level deeper, or returns `fallback` past
    /// [`MAX_IMPORT_DEPTH`]. [`Self::finish`] then refuses the import, so
    /// every later level falls back at once instead of descending further.
    fn nested<T>(&mut self, fallback: impl FnOnce() -> T, parse: impl FnOnce(&mut Self) -> T) -> T {
        if self.depth_limit_reached || self.depth >= MAX_IMPORT_DEPTH {
            self.depth_limit_reached = true;
            return fallback();
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
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
            path: self.reader.identity(schema_path),
            kind,
            qname: qname.to_string(),
        };
        if let Some(path) = self.declaration_paths.get(&query) {
            return path.clone();
        }
        let path = find_external_declaration(schema_el, schema_path, kind, qname, &self.reader);
        self.declaration_paths.insert(query, path.clone());
        path
    }

    fn finish(self, mut schema: SchemaNode) -> Result<SchemaNode, XmlFormatError> {
        // Checked first: the levels past the limit were not imported, so the
        // other refusals may only describe the truncated schema.
        if self.depth_limit_reached {
            return Err(depth_limit_error());
        }
        if let Some(error) = self.unsupported_particle {
            return Err(error);
        }
        if let Some(error) = self.unsupported_attribute_use {
            return Err(error);
        }
        if let Some(error) = self.unsupported_default {
            return Err(error);
        }
        if let Some(error) = self.unsupported_schema_group {
            return Err(error);
        }
        if let Some(error) = self.unsupported_alternative_occurrence {
            return Err(error);
        }
        if let Some(error) = self.unsupported_restriction {
            return Err(error);
        }
        if let Some(error) = self.unsupported_substitution {
            return Err(error);
        }
        if let Some(error) = self.unsupported_wildcard {
            return Err(error);
        }
        if self.materialization_limit_reached {
            return Err(XmlFormatError::SchemaMaterializationLimit {
                limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
            });
        }
        if wildcard_exclusions::has_unresolved_mark(&schema) {
            return Err(unsupported_wildcard(
                "##definedSibling was not resolved against a complete complex type",
            ));
        }
        qualify_wildcard_siblings(&mut schema);
        if let Some(group) = invalid_repeating_sequence_group(&schema) {
            return Err(XmlFormatError::InvalidRepeatingSequenceSchema {
                group: group.to_string(),
            });
        }
        crate::declaration::validate_schema(&schema)?;
        Ok(schema)
    }

    fn reject_repeating_particle(&mut self, error: XmlFormatError) {
        self.unsupported_particle.get_or_insert(error);
    }

    fn reject_attribute_use(&mut self, error: XmlFormatError) {
        self.unsupported_attribute_use.get_or_insert(error);
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

    fn reject_wildcard(&mut self, error: XmlFormatError) {
        self.unsupported_wildcard.get_or_insert(error);
    }
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
    import_root_with_reader(path, root, &SchemaReader::default())
}

/// Imports an element while confining all local XSD reads to one canonical
/// authorizing root. Contained absolute paths, aliases, and symlinks are valid.
pub fn import_root_with_resource_root(
    path: &Path,
    root: Option<&str>,
    resource_root: &Path,
) -> Result<SchemaNode, XmlFormatError> {
    let reader = SchemaReader::confined(resource_root)?;
    reader.finish(import_root_with_reader(path, root, &reader))
}

/// Imports from host-authorized immutable bytes, including every dependency.
/// `path` and `resource_root` are approved absolute identities, not paths this
/// entry point may reopen. Reader failures are authoritative, even in optional
/// declaration searches. Existing path-based APIs retain their behavior.
pub fn import_root_with_byte_reader(
    path: &Path,
    root: Option<&str>,
    resource_root: &Path,
    reader: std::rc::Rc<dyn XmlSchemaByteReader>,
) -> Result<SchemaNode, XmlFormatError> {
    let reader = SchemaReader::supplied(resource_root, reader)?;
    reader.finish(import_root_with_reader(path, root, &reader))
}

fn import_root_with_reader(
    path: &Path,
    root: Option<&str>,
    reader: &SchemaReader,
) -> Result<SchemaNode, XmlFormatError> {
    let text = reader.read(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let schema_el = doc.root_element();
    if reader.is_supplied()
        && (schema_el.tag_name().name() != "schema"
            || schema_el.tag_name().namespace() != Some("http://www.w3.org/2001/XMLSchema"))
    {
        return Err(reader
            .refuse("supplied XML Schema resource is not an XSD document")
            .into());
    }
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
            reader: reader.clone(),
            substitutions: substitution::build(&schema_el, path, reader),
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
        && let Some(external_path) =
            find_external_declaration(&schema_el, path, "element", root, reader)
    {
        let external_text = reader.read(&external_path)?;
        let external_doc = roxmltree::Document::parse(&external_text)?;
        let external_schema = external_doc.root_element();
        let root_local = expanded_name(root).map_or(local_name(root), |(_, local)| local);
        if let Some(root_element) = top_level(&external_schema, "element", root_local) {
            let mut state = ParseState {
                reader: reader.clone(),
                substitutions: substitution::build(&schema_el, path, reader),
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
    import_type_with_reader(path, type_name, &SchemaReader::default())
}

/// Imports a named type with the same resource boundary for all dependencies.
pub fn import_type_with_resource_root(
    path: &Path,
    type_name: &str,
    resource_root: &Path,
) -> Result<SchemaNode, XmlFormatError> {
    let reader = SchemaReader::confined(resource_root)?;
    reader.finish(import_type_with_reader(path, type_name, &reader))
}

/// Named-type counterpart to [`import_root_with_byte_reader`].
pub fn import_type_with_byte_reader(
    path: &Path,
    type_name: &str,
    resource_root: &Path,
    reader: std::rc::Rc<dyn XmlSchemaByteReader>,
) -> Result<SchemaNode, XmlFormatError> {
    let reader = SchemaReader::supplied(resource_root, reader)?;
    reader.finish(import_type_with_reader(path, type_name, &reader))
}

fn import_type_with_reader(
    path: &Path,
    type_name: &str,
    reader: &SchemaReader,
) -> Result<SchemaNode, XmlFormatError> {
    let text = reader.read(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let schema_el = doc.root_element();
    let mut state = ParseState {
        reader: reader.clone(),
        substitutions: substitution::build(&schema_el, path, reader),
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
            visited.insert(reader.identity(path));
            let effective_namespace = schema_el.attribute("targetNamespace");
            search_dependencies(
                &schema_el,
                path,
                "complexType",
                local,
                (Some(namespace), effective_namespace),
                &mut visited,
                reader,
            )
            .and_then(|external_path| {
                let text = reader.read(&external_path).ok()?;
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
                    ComplexTypeUse::Complete,
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
    import_type_base_with_reader(path, type_name, &SchemaReader::default())
}

/// Resolves a type's direct base without opening dependencies outside the root.
pub fn import_type_base_with_resource_root(
    path: &Path,
    type_name: &str,
    resource_root: &Path,
) -> Result<Option<String>, XmlFormatError> {
    let reader = SchemaReader::confined(resource_root)?;
    reader.finish(import_type_base_with_reader(path, type_name, &reader))
}

/// Base-type counterpart to [`import_root_with_byte_reader`].
pub fn import_type_base_with_byte_reader(
    path: &Path,
    type_name: &str,
    resource_root: &Path,
    reader: std::rc::Rc<dyn XmlSchemaByteReader>,
) -> Result<Option<String>, XmlFormatError> {
    let reader = SchemaReader::supplied(resource_root, reader)?;
    reader.finish(import_type_base_with_reader(path, type_name, &reader))
}

fn import_type_base_with_reader(
    path: &Path,
    type_name: &str,
    reader: &SchemaReader,
) -> Result<Option<String>, XmlFormatError> {
    let text = reader.read(path)?;
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
        reader,
    )
    .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:complexType `{type_name}`")))?;
    let external_text = reader.read(&declaration_path)?;
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

fn declaration_namespace(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    inherited_namespace: Option<&str>,
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
            .or(inherited_namespace)
            .filter(|namespace| !namespace.is_empty())
        && let Some(namespace) = XmlNamespace::qualified(namespace)
    {
        Some(namespace)
    } else {
        let explicitly_unqualified = declaration.attribute("form") == Some("unqualified");
        let schema_has_namespace = schema
            .attribute("targetNamespace")
            .or(inherited_namespace)
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

/// Finds a named top-level declaration (`xs:complexType name=..` etc.).
fn top_level<'a>(schema_el: &Node<'a, 'a>, tag: &str, name: &str) -> Option<Node<'a, 'a>> {
    schema_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == tag && n.attribute("name") == Some(name))
}

fn schema_node_count(node: &SchemaNode) -> usize {
    1 + match &node.kind {
        ir::SchemaKind::Scalar { .. } | ir::SchemaKind::ScalarUnion { .. } => 0,
        ir::SchemaKind::Group { children, .. } => children.iter().map(schema_node_count).sum(),
    }
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
    reader: &SchemaReader,
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
    visited.insert(reader.identity(schema_path));
    search_dependencies(
        schema_el,
        schema_path,
        tag,
        expanded.map_or(local_name(qname), |(_, local)| local),
        (wanted_namespace.as_deref(), effective_namespace.as_deref()),
        &mut visited,
        reader,
    )
}

fn search_dependencies(
    schema_el: &Node,
    schema_path: &Path,
    tag: &str,
    name: &str,
    namespaces: (Option<&str>, Option<&str>),
    visited: &mut BTreeSet<PathBuf>,
    reader: &SchemaReader,
) -> Option<PathBuf> {
    let (wanted_namespace, effective_namespace) = namespaces;
    let root = LinkedSchema::new(schema_el, schema_path.to_path_buf(), effective_namespace);
    schema_links::walk(root, |schema, link| {
        if !link.include {
            let Some(wanted) = wanted_namespace else {
                return Follow::Skip;
            };
            if link
                .namespace
                .as_deref()
                .is_some_and(|namespace| namespace != wanted)
            {
                return Follow::Skip;
            }
        }
        let Some(location) = &link.location else {
            return Follow::Skip;
        };
        let Some(dependency) = reader.dependency(&schema.path, location) else {
            return Follow::Skip;
        };
        let path = reader.identity(&dependency);
        if !visited.insert(path.clone()) {
            return Follow::Skip;
        }
        let Ok(text) = reader.read(&path) else {
            return Follow::Skip;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            return Follow::Skip;
        };
        let dependency_schema = doc.root_element();
        let effective_namespace = dependency_schema
            .attribute("targetNamespace")
            .or(schema.inherited_namespace(link));
        if wanted_namespace.is_none_or(|wanted| effective_namespace == Some(wanted))
            && top_level(&dependency_schema, tag, name).is_some()
        {
            return Follow::Found(path);
        }
        Follow::Descend(LinkedSchema::new(
            &dependency_schema,
            path,
            effective_namespace,
        ))
    })
}

fn normalized_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn is_single_occurrence(particle: &Node<'_, '_>) -> bool {
    particle
        .attribute("minOccurs")
        .is_none_or(|value| value == "1")
        && particle
            .attribute("maxOccurs")
            .is_none_or(|value| value == "1")
}

fn depth_limit_error() -> XmlFormatError {
    XmlFormatError::SchemaDepthLimit {
        limit: MAX_IMPORT_DEPTH,
    }
}

fn non_negative_integer_is_one(value: &str) -> bool {
    let value = value.trim();
    let digits = value.strip_prefix('+').unwrap_or(value);
    digits.trim_start_matches('0') == "1"
}

fn non_negative_integer_is_zero(value: &str) -> bool {
    let value = value.trim();
    let digits = value.strip_prefix('+').unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|digit| digit == b'0')
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
pub(crate) mod tests;
