//! A bounded DTD subset importer for ordinary element/attribute schemas.
//!
//! The importer intentionally does not expand entities or external subsets.
//! It supports `ELEMENT` and `ATTLIST` declarations, child sequences and
//! choices, the standard occurrence suffixes, `EMPTY`, exact `(#PCDATA)`,
//! and required CDATA/enumeration attributes.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use ir::{ScalarType, SchemaNode, XML_TEXT_FIELD};
use thiserror::Error;

const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_DECLARATIONS: usize = 4096;
const MAX_ATTRIBUTES_PER_ELEMENT: usize = 1024;
const MAX_NAME_BYTES: usize = 1024;
const MAX_NESTING_DEPTH: usize = 64;
const MAX_PARTICLES: usize = 65_536;
const MAX_SCHEMA_NODES: usize = 100_000;

#[derive(Debug, Error)]
pub enum DtdError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("DTD is not UTF-8 at byte {offset}")]
    InvalidUtf8 { offset: usize },
    #[error("DTD input exceeds the {limit}-byte limit")]
    InputTooLarge { limit: usize },
    #[error("DTD syntax error at byte {offset}: {message}")]
    Syntax { offset: usize, message: String },
    #[error("unsupported DTD feature at byte {offset}: {feature}")]
    Unsupported { offset: usize, feature: String },
    #[error("DTD {kind} limit of {limit} exceeded")]
    LimitExceeded { kind: &'static str, limit: usize },
    #[error("DTD contains no element declarations")]
    NoElementDeclarations,
    #[error("DTD does not declare root element `{0}`")]
    MissingRoot(String),
    #[error("DTD declares element `{0}` more than once")]
    DuplicateElement(String),
    #[error("DTD declares attribute `{attribute}` on element `{element}` more than once")]
    DuplicateAttribute { element: String, attribute: String },
    #[error("DTD ATTLIST owner `{0}` has no ELEMENT declaration")]
    UndeclaredAttributeOwner(String),
    #[error("DTD element `{parent}` references undeclared element `{child}`")]
    UnresolvedElement { parent: String, child: String },
    #[error("DTD element `{element}` uses `{name}` as both a child element and an attribute")]
    AttributeElementNameCollision { element: String, name: String },
    #[error("DTD element `{0}` is recursively defined and cannot become a finite schema")]
    RecursiveElement(String),
    #[error(
        "DTD element `{element}` repeats a particle with {member_count} distinct element members; tuple order cannot be preserved"
    )]
    UnsupportedRepeatingParticle {
        element: String,
        member_count: usize,
    },
}

/// Imports the first declared element from a DTD file.
pub fn import(path: &Path) -> Result<SchemaNode, DtdError> {
    import_root(path, None)
}

/// Imports a named root element from a DTD file. When `root` is `None`, the
/// first `ELEMENT` declaration is used.
pub fn import_root(path: &Path, root: Option<&str>) -> Result<SchemaNode, DtdError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_INPUT_BYTES.min(8192));
    file.take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(DtdError::InputTooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
    let text = String::from_utf8(bytes).map_err(|error| DtdError::InvalidUtf8 {
        offset: error.utf8_error().valid_up_to(),
    })?;
    import_root_str(&text, root)
}

/// Imports a named root element from in-memory DTD text.
pub fn import_root_str(text: &str, root: Option<&str>) -> Result<SchemaNode, DtdError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(DtdError::InputTooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
    let document = Parser::new(text).parse()?;
    let root_name = match root {
        Some(name) if document.elements.contains_key(name) => name,
        Some(name) => return Err(DtdError::MissingRoot(name.to_string())),
        None => document
            .order
            .first()
            .map(String::as_str)
            .ok_or(DtdError::NoElementDeclarations)?,
    };
    Expander::new(&document).expand(root_name)
}

#[derive(Debug)]
struct Document {
    elements: BTreeMap<String, ElementDecl>,
    order: Vec<String>,
}

#[derive(Debug)]
struct ElementDecl {
    content: Content,
    attributes: Vec<AttributeDecl>,
}

#[derive(Debug)]
struct AttributeDecl {
    name: String,
}

#[derive(Debug)]
enum Content {
    Empty,
    Text,
    Children(Particle),
}

#[derive(Debug)]
struct Particle {
    kind: ParticleKind,
    occurs: Occurs,
}

#[derive(Debug)]
enum ParticleKind {
    Element(String),
    Sequence(Vec<Particle>),
    Choice(Vec<Particle>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Occurs {
    Once,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

impl Occurs {
    fn is_repeating(self) -> bool {
        matches!(self, Self::ZeroOrMore | Self::OneOrMore)
    }
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
    declaration_count: usize,
    particle_count: usize,
    elements: BTreeMap<String, ElementDecl>,
    order: Vec<String>,
    attributes: BTreeMap<String, Vec<AttributeDecl>>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            position: if text.starts_with('\u{feff}') {
                '\u{feff}'.len_utf8()
            } else {
                0
            },
            declaration_count: 0,
            particle_count: 0,
            elements: BTreeMap::new(),
            order: Vec::new(),
            attributes: BTreeMap::new(),
        }
    }

    fn parse(mut self) -> Result<Document, DtdError> {
        while !self.is_eof() {
            self.skip_whitespace();
            if self.is_eof() {
                break;
            }
            if self.consume("<!--") {
                self.skip_until("-->", "unterminated DTD comment")?;
            } else if self.consume("<?") {
                self.skip_until("?>", "unterminated processing instruction")?;
            } else if self.consume_declaration("ELEMENT") {
                self.bump_declaration_count()?;
                self.parse_element_declaration()?;
            } else if self.consume_declaration("ATTLIST") {
                self.bump_declaration_count()?;
                self.parse_attribute_list()?;
            } else if self.starts_with("<!ENTITY") || self.starts_with("%") {
                return self.unsupported("entity declarations and references");
            } else if self.starts_with("<!NOTATION") {
                return self.unsupported("notation declarations");
            } else if self.starts_with("<![") {
                return self.unsupported("conditional include/ignore sections");
            } else if self.starts_with("<!") {
                return self.unsupported("declaration other than ELEMENT or ATTLIST");
            } else {
                return self.syntax("expected an ELEMENT or ATTLIST declaration");
            }
        }

        for owner in self.attributes.keys() {
            if !self.elements.contains_key(owner) {
                return Err(DtdError::UndeclaredAttributeOwner(owner.clone()));
            }
        }
        for (owner, attributes) in self.attributes {
            if let Some(element) = self.elements.get_mut(&owner) {
                element.attributes = attributes;
            }
        }
        Ok(Document {
            elements: self.elements,
            order: self.order,
        })
    }

    fn parse_element_declaration(&mut self) -> Result<(), DtdError> {
        self.require_whitespace("ELEMENT name")?;
        let name = self.parse_name()?;
        self.require_whitespace("ELEMENT content")?;
        let content = self.parse_content()?;
        self.skip_whitespace();
        self.expect('>')?;
        if self.elements.contains_key(&name) {
            return Err(DtdError::DuplicateElement(name));
        }
        self.order.push(name.clone());
        self.elements.insert(
            name,
            ElementDecl {
                content,
                attributes: Vec::new(),
            },
        );
        Ok(())
    }

    fn parse_content(&mut self) -> Result<Content, DtdError> {
        if self.consume_keyword("EMPTY") {
            return Ok(Content::Empty);
        }
        if self.consume_keyword("ANY") {
            return self.unsupported("ANY element content");
        }
        if !self.consume("(") {
            return self.syntax("expected EMPTY, (#PCDATA), or a child particle");
        }
        self.skip_whitespace();
        if self.consume("#PCDATA") {
            self.skip_whitespace();
            if !self.consume(")") {
                return self.unsupported("mixed PCDATA and child-element content");
            }
            if self
                .peek_byte()
                .is_some_and(|byte| matches!(byte, b'?' | b'*' | b'+'))
            {
                return self.unsupported("an occurrence suffix on #PCDATA content");
            }
            return Ok(Content::Text);
        }
        let mut particle = self.parse_particle_group(1)?;
        particle.occurs = self.parse_occurrence();
        Ok(Content::Children(particle))
    }

    fn parse_particle_group(&mut self, depth: usize) -> Result<Particle, DtdError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(DtdError::LimitExceeded {
                kind: "particle nesting depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        self.skip_whitespace();
        let first = self.parse_particle(depth)?;
        let mut particles = vec![first];
        let mut separator = None;
        loop {
            self.skip_whitespace();
            if self.consume(")") {
                break;
            }
            let next_separator = match self.peek_byte() {
                Some(b',') => b',',
                Some(b'|') => b'|',
                Some(b'#') => return self.unsupported("mixed PCDATA and child-element content"),
                Some(_) => return self.syntax("expected `,`, `|`, or `)` in child particle"),
                None => return self.syntax("unterminated child particle"),
            };
            if separator.is_some_and(|current| current != next_separator) {
                return self.syntax("cannot mix `,` and `|` at one particle level");
            }
            separator = Some(next_separator);
            self.position += 1;
            self.skip_whitespace();
            particles.push(self.parse_particle(depth)?);
        }
        self.bump_particle_count()?;
        let kind = if separator == Some(b'|') {
            ParticleKind::Choice(particles)
        } else {
            ParticleKind::Sequence(particles)
        };
        Ok(Particle {
            kind,
            occurs: Occurs::Once,
        })
    }

    fn parse_particle(&mut self, depth: usize) -> Result<Particle, DtdError> {
        let kind = if self.consume("(") {
            let mut group = self.parse_particle_group(depth + 1)?;
            group.occurs = self.parse_occurrence();
            return Ok(group);
        } else if self.starts_with("#PCDATA") {
            return self.unsupported("mixed PCDATA and child-element content");
        } else {
            ParticleKind::Element(self.parse_name()?)
        };
        self.bump_particle_count()?;
        Ok(Particle {
            kind,
            occurs: self.parse_occurrence(),
        })
    }

    fn parse_occurrence(&mut self) -> Occurs {
        match self.peek_byte() {
            Some(b'?') => {
                self.position += 1;
                Occurs::Optional
            }
            Some(b'*') => {
                self.position += 1;
                Occurs::ZeroOrMore
            }
            Some(b'+') => {
                self.position += 1;
                Occurs::OneOrMore
            }
            _ => Occurs::Once,
        }
    }

    fn parse_attribute_list(&mut self) -> Result<(), DtdError> {
        self.require_whitespace("ATTLIST owner")?;
        let owner = self.parse_name()?;
        let mut parsed = Vec::new();
        loop {
            self.skip_whitespace();
            if self.consume(">") {
                break;
            }
            let name = self.parse_name()?;
            self.require_whitespace("attribute type")?;
            self.parse_attribute_type()?;
            self.require_whitespace("attribute default declaration")?;
            if !self.consume("#REQUIRED") {
                return self.unsupported("attribute defaults other than #REQUIRED");
            }
            if self
                .peek_byte()
                .is_some_and(|byte| !byte.is_ascii_whitespace() && byte != b'>')
            {
                return self.syntax("expected whitespace or `>` after #REQUIRED");
            }
            if parsed
                .iter()
                .any(|attribute: &AttributeDecl| attribute.name == name)
                || self.attributes.get(&owner).is_some_and(|attributes| {
                    attributes.iter().any(|attribute| attribute.name == name)
                })
            {
                return Err(DtdError::DuplicateAttribute {
                    element: owner,
                    attribute: name,
                });
            }
            parsed.push(AttributeDecl { name });
            let existing = self.attributes.get(&owner).map_or(0, Vec::len);
            if existing + parsed.len() > MAX_ATTRIBUTES_PER_ELEMENT {
                return Err(DtdError::LimitExceeded {
                    kind: "attributes per element",
                    limit: MAX_ATTRIBUTES_PER_ELEMENT,
                });
            }
        }
        self.attributes.entry(owner).or_default().extend(parsed);
        Ok(())
    }

    fn parse_attribute_type(&mut self) -> Result<(), DtdError> {
        if self.consume_keyword("CDATA") {
            return Ok(());
        }
        if self.consume("(") {
            self.skip_whitespace();
            self.parse_nmtoken()?;
            loop {
                self.skip_whitespace();
                if self.consume(")") {
                    return Ok(());
                }
                self.expect('|')?;
                self.skip_whitespace();
                self.parse_nmtoken()?;
            }
        }
        let offset = self.position;
        let ty = self.parse_name()?;
        Err(DtdError::Unsupported {
            offset,
            feature: format!("attribute type `{ty}`"),
        })
    }

    fn parse_name(&mut self) -> Result<String, DtdError> {
        let start = self.position;
        let Some(first) = self.peek_byte() else {
            return self.syntax("expected a name");
        };
        if !is_name_start(first) {
            return self.syntax("expected an ASCII XML name");
        }
        self.position += 1;
        while self.peek_byte().is_some_and(is_name_byte) {
            self.position += 1;
        }
        if self.position - start > MAX_NAME_BYTES {
            return Err(DtdError::LimitExceeded {
                kind: "name length",
                limit: MAX_NAME_BYTES,
            });
        }
        let name = &self.text[start..self.position];
        if name.contains(':') {
            return Err(DtdError::Unsupported {
                offset: start,
                feature: "namespace-qualified DTD names".to_string(),
            });
        }
        Ok(name.to_string())
    }

    fn parse_nmtoken(&mut self) -> Result<(), DtdError> {
        let start = self.position;
        while self.peek_byte().is_some_and(is_name_byte) {
            self.position += 1;
        }
        if self.position == start {
            return self.syntax("expected an enumeration token");
        }
        if self.position - start > MAX_NAME_BYTES {
            return Err(DtdError::LimitExceeded {
                kind: "enumeration token length",
                limit: MAX_NAME_BYTES,
            });
        }
        Ok(())
    }

    fn bump_declaration_count(&mut self) -> Result<(), DtdError> {
        self.declaration_count += 1;
        if self.declaration_count > MAX_DECLARATIONS {
            return Err(DtdError::LimitExceeded {
                kind: "declaration count",
                limit: MAX_DECLARATIONS,
            });
        }
        Ok(())
    }

    fn bump_particle_count(&mut self) -> Result<(), DtdError> {
        self.particle_count += 1;
        if self.particle_count > MAX_PARTICLES {
            return Err(DtdError::LimitExceeded {
                kind: "particle count",
                limit: MAX_PARTICLES,
            });
        }
        Ok(())
    }

    fn consume_declaration(&mut self, keyword: &str) -> bool {
        let prefix = format!("<!{keyword}");
        if !self.starts_with(&prefix) {
            return false;
        }
        let after = self.position + prefix.len();
        if self
            .text
            .as_bytes()
            .get(after)
            .is_none_or(|byte| !byte.is_ascii_whitespace())
        {
            return false;
        }
        self.position = after;
        true
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if !self.starts_with(keyword) {
            return false;
        }
        let after = self.position + keyword.len();
        if self
            .text
            .as_bytes()
            .get(after)
            .is_some_and(|byte| is_name_byte(*byte))
        {
            return false;
        }
        self.position = after;
        true
    }

    fn require_whitespace(&mut self, context: &str) -> Result<(), DtdError> {
        let start = self.position;
        self.skip_whitespace();
        if self.position == start {
            return self.syntax(&format!("expected whitespace before {context}"));
        }
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.remaining().chars().next()
            && character.is_whitespace()
        {
            self.position += character.len_utf8();
        }
    }

    fn skip_until(&mut self, delimiter: &str, message: &str) -> Result<(), DtdError> {
        let Some(offset) = self.remaining().find(delimiter) else {
            return self.syntax(message);
        };
        self.position += offset + delimiter.len();
        Ok(())
    }

    fn expect(&mut self, expected: char) -> Result<(), DtdError> {
        if self.peek_byte() == Some(expected as u8) {
            self.position += 1;
            Ok(())
        } else {
            self.syntax(&format!("expected `{expected}`"))
        }
    }

    fn consume(&mut self, text: &str) -> bool {
        if self.starts_with(text) {
            self.position += text.len();
            true
        } else {
            false
        }
    }

    fn starts_with(&self, text: &str) -> bool {
        self.remaining().starts_with(text)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }

    fn remaining(&self) -> &'a str {
        &self.text[self.position..]
    }

    fn is_eof(&self) -> bool {
        self.position == self.text.len()
    }

    fn syntax<T>(&self, message: &str) -> Result<T, DtdError> {
        Err(DtdError::Syntax {
            offset: self.position,
            message: message.to_string(),
        })
    }

    fn unsupported<T>(&self, feature: &str) -> Result<T, DtdError> {
        Err(DtdError::Unsupported {
            offset: self.position,
            feature: feature.to_string(),
        })
    }
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':')
}

fn is_name_byte(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
}

struct Expander<'a> {
    document: &'a Document,
    active: Vec<&'a str>,
    node_count: usize,
}

impl<'a> Expander<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            active: Vec::new(),
            node_count: 0,
        }
    }

    fn expand(mut self, root: &str) -> Result<SchemaNode, DtdError> {
        let (root, _) = self
            .document
            .elements
            .get_key_value(root)
            .ok_or_else(|| DtdError::MissingRoot(root.to_string()))?;
        self.expand_element(root)
    }

    fn expand_element(&mut self, name: &'a str) -> Result<SchemaNode, DtdError> {
        if self.active.contains(&name) {
            return Err(DtdError::RecursiveElement(name.to_string()));
        }
        if self.active.len() >= MAX_NESTING_DEPTH {
            return Err(DtdError::LimitExceeded {
                kind: "schema expansion depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        let declaration =
            self.document
                .elements
                .get(name)
                .ok_or_else(|| DtdError::UnresolvedElement {
                    parent: self.active.last().copied().unwrap_or(name).to_string(),
                    child: name.to_string(),
                })?;
        self.bump_node_count()?;
        self.active.push(name);
        let result = match &declaration.content {
            Content::Text if declaration.attributes.is_empty() => {
                Ok(SchemaNode::scalar(name, ScalarType::String))
            }
            Content::Text => {
                self.bump_node_count()?;
                let mut children =
                    vec![SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text()];
                children.extend(self.expand_attributes(&declaration.attributes)?);
                Ok(SchemaNode::group(name, children))
            }
            Content::Empty => Ok(SchemaNode::group(
                name,
                self.expand_attributes(&declaration.attributes)?,
            )),
            Content::Children(particle) => {
                let uses = child_uses(name, particle)?;
                let mut children = Vec::with_capacity(uses.len() + declaration.attributes.len());
                for child_use in uses {
                    let (child_name, _) = self
                        .document
                        .elements
                        .get_key_value(child_use.name)
                        .ok_or_else(|| DtdError::UnresolvedElement {
                            parent: name.to_string(),
                            child: child_use.name.to_string(),
                        })?;
                    let mut child = self.expand_element(child_name)?;
                    child.repeating = child_use.repeating;
                    children.push(child);
                }
                if let Some(attribute) = declaration
                    .attributes
                    .iter()
                    .find(|attribute| children.iter().any(|child| child.name == attribute.name))
                {
                    return Err(DtdError::AttributeElementNameCollision {
                        element: name.to_string(),
                        name: attribute.name.clone(),
                    });
                }
                children.extend(self.expand_attributes(&declaration.attributes)?);
                Ok(SchemaNode::group(name, children))
            }
        };
        self.active.pop();
        result
    }

    fn expand_attributes(
        &mut self,
        attributes: &[AttributeDecl],
    ) -> Result<Vec<SchemaNode>, DtdError> {
        let mut result = Vec::with_capacity(attributes.len());
        for attribute in attributes {
            self.bump_node_count()?;
            result.push(SchemaNode::scalar(&attribute.name, ScalarType::String).attribute());
        }
        Ok(result)
    }

    fn bump_node_count(&mut self) -> Result<(), DtdError> {
        self.node_count += 1;
        if self.node_count > MAX_SCHEMA_NODES {
            return Err(DtdError::LimitExceeded {
                kind: "expanded schema node count",
                limit: MAX_SCHEMA_NODES,
            });
        }
        Ok(())
    }
}

struct ChildUse<'a> {
    name: &'a str,
    repeating: bool,
}

fn child_uses<'a>(element: &str, particle: &'a Particle) -> Result<Vec<ChildUse<'a>>, DtdError> {
    validate_repeating_particles(element, particle)?;
    let mut result = Vec::new();
    collect_child_uses(particle, false, &mut result);
    Ok(result)
}

fn validate_repeating_particles(element: &str, particle: &Particle) -> Result<(), DtdError> {
    if particle.occurs.is_repeating() {
        let mut names = BTreeSet::new();
        collect_distinct_names(particle, &mut names);
        if names.len() > 1 {
            return Err(DtdError::UnsupportedRepeatingParticle {
                element: element.to_string(),
                member_count: names.len(),
            });
        }
    }
    match &particle.kind {
        ParticleKind::Sequence(children) | ParticleKind::Choice(children) => {
            for child in children {
                validate_repeating_particles(element, child)?;
            }
        }
        ParticleKind::Element(_) => {}
    }
    Ok(())
}

fn collect_distinct_names<'a>(particle: &'a Particle, names: &mut BTreeSet<&'a str>) {
    match &particle.kind {
        ParticleKind::Element(name) => {
            names.insert(name);
        }
        ParticleKind::Sequence(children) | ParticleKind::Choice(children) => {
            for child in children {
                collect_distinct_names(child, names);
            }
        }
    }
}

fn collect_child_uses<'a>(
    particle: &'a Particle,
    inherited_repeating: bool,
    result: &mut Vec<ChildUse<'a>>,
) {
    let repeating = inherited_repeating || particle.occurs.is_repeating();
    match &particle.kind {
        ParticleKind::Element(name) => {
            if let Some(existing) = result.iter_mut().find(|child| child.name == name) {
                existing.repeating = true;
            } else {
                result.push(ChildUse { name, repeating });
            }
        }
        ParticleKind::Sequence(children) | ParticleKind::Choice(children) => {
            for child in children {
                collect_child_uses(child, repeating, result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ir::{Instance, SchemaKind, Value};

    use super::*;
    use crate::{from_str, to_string};

    const SUPPORTED_DTD: &str = r#"
        <!-- self-authored DTD exercising the supported subset -->
        <!ELEMENT Header (#PCDATA)>
        <!ELEMENT Item (#PCDATA)>
        <!ELEMENT Note (#PCDATA)>
        <!ELEMENT Point EMPTY>
        <!ATTLIST Point Lat CDATA #REQUIRED Direction (E|W) #REQUIRED>
        <!ELEMENT Footer (#PCDATA)>
        <!ELEMENT Report (Header,(Item|Note),Point,Point+,Footer?)>
    "#;

    #[test]
    fn imports_forward_references_choices_attributes_and_repetitions() {
        let schema = import_root_str(SUPPORTED_DTD, Some("Report")).unwrap();
        let SchemaKind::Group { children, .. } = &schema.kind else {
            panic!("Report should be a group");
        };
        assert_eq!(
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["Header", "Item", "Note", "Point", "Footer"]
        );
        let point = schema.child("Point").unwrap();
        assert!(point.repeating);
        assert!(
            point
                .child("Lat")
                .is_some_and(|attribute| attribute.attribute)
        );
        assert!(
            point
                .child("Direction")
                .is_some_and(|attribute| attribute.attribute)
        );
        assert!(!schema.child("Footer").unwrap().repeating);
    }

    #[test]
    fn named_root_and_first_declared_root_are_distinct() {
        let first = import_root_str(&format!("\u{feff}{SUPPORTED_DTD}"), None).unwrap();
        let report = import_root_str(SUPPORTED_DTD, Some("Report")).unwrap();
        assert_eq!(first.name, "Header");
        assert_eq!(report.name, "Report");
        assert!(matches!(
            import_root_str(SUPPORTED_DTD, Some("Missing")),
            Err(DtdError::MissingRoot(name)) if name == "Missing"
        ));
    }

    #[test]
    fn absent_groups_remain_absent_while_present_empty_groups_round_trip() {
        let schema = import_root_str(
            r#"
                <!ELEMENT Root ((Left|Right),Flag?)>
                <!ELEMENT Left (Value)>
                <!ELEMENT Right (Value)>
                <!ELEMENT Value (#PCDATA)>
                <!ELEMENT Flag EMPTY>
            "#,
            Some("Root"),
        )
        .unwrap();
        let instance = from_str(
            "<Root><Left><Value>selected</Value></Left><Flag/></Root>",
            &schema,
        )
        .unwrap();
        assert!(instance.field("Left").is_some());
        assert!(instance.field("Right").is_none());
        assert_eq!(instance.field("Flag"), Some(&Instance::Group(Vec::new())));

        let xml = to_string(&schema, &instance).unwrap();
        assert!(xml.contains("<Left>"), "{xml}");
        assert!(!xml.contains("<Right>"), "{xml}");
        assert!(xml.contains("<Flag>"), "{xml}");
        assert!(from_str(&xml, &schema).unwrap().field("Flag").is_some());
    }

    #[test]
    fn pcdata_with_attributes_becomes_simple_content_group() {
        let schema = import_root_str(
            "<!ELEMENT Label (#PCDATA)><!ATTLIST Label lang CDATA #REQUIRED>",
            Some("Label"),
        )
        .unwrap();
        let instance = from_str("<Label lang=\"en\">hello</Label>", &schema).unwrap();
        assert_eq!(
            instance.field(XML_TEXT_FIELD).and_then(Instance::as_scalar),
            Some(&Value::String("hello".to_string()))
        );
        assert_eq!(
            instance.field("lang").and_then(Instance::as_scalar),
            Some(&Value::String("en".to_string()))
        );
    }

    #[test]
    fn file_api_imports_self_authored_dtd() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_dtd_import_test_{}.dtd",
            std::process::id()
        ));
        std::fs::write(&path, SUPPORTED_DTD).unwrap();
        let schema = import_root(&path, Some("Report")).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(schema.name, "Report");
    }

    #[test]
    fn rejects_unsupported_or_unrepresentable_content_precisely() {
        let cases = [
            ("<!ELEMENT Root ANY>", "ANY element content"),
            (
                "<!ELEMENT Root (#PCDATA|Child)*><!ELEMENT Child EMPTY>",
                "mixed PCDATA and child-element content",
            ),
            ("<!ENTITY item \"value\"><!ELEMENT Root EMPTY>", "entity"),
            ("<!NOTATION image SYSTEM \"image/png\">", "notation"),
            ("<![INCLUDE[<!ELEMENT Root EMPTY>]]>", "conditional"),
            (
                "<!ELEMENT Root EMPTY><!ATTLIST Root optional CDATA #IMPLIED>",
                "attribute defaults other than #REQUIRED",
            ),
        ];
        for (text, expected) in cases {
            let error = import_root_str(text, Some("Root")).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }

        let tuple = import_root_str(
            "<!ELEMENT Root (A,B)*><!ELEMENT A EMPTY><!ELEMENT B EMPTY>",
            Some("Root"),
        )
        .unwrap_err();
        assert!(matches!(
            tuple,
            DtdError::UnsupportedRepeatingParticle {
                member_count: 2,
                ..
            }
        ));
    }

    #[test]
    fn rejects_unresolved_cycles_duplicates_and_orphan_attributes() {
        assert!(matches!(
            import_root_str("<!ELEMENT Root (Missing)>", Some("Root")),
            Err(DtdError::UnresolvedElement { child, .. }) if child == "Missing"
        ));
        assert!(matches!(
            import_root_str(
                "<!ELEMENT Root (Child)><!ELEMENT Child (Root)>",
                Some("Root")
            ),
            Err(DtdError::RecursiveElement(name)) if name == "Root"
        ));
        assert!(matches!(
            import_root_str("<!ELEMENT Root EMPTY><!ELEMENT Root EMPTY>", Some("Root")),
            Err(DtdError::DuplicateElement(name)) if name == "Root"
        ));
        assert!(matches!(
            import_root_str(
                "<!ATTLIST Missing id CDATA #REQUIRED><!ELEMENT Root EMPTY>",
                Some("Root")
            ),
            Err(DtdError::UndeclaredAttributeOwner(name)) if name == "Missing"
        ));
        assert!(matches!(
            import_root_str(
                "<!ELEMENT Root (Code)><!ELEMENT Code (#PCDATA)><!ATTLIST Root Code CDATA #REQUIRED>",
                Some("Root")
            ),
            Err(DtdError::AttributeElementNameCollision { name, .. }) if name == "Code"
        ));
    }

    #[test]
    fn enforces_input_and_nesting_limits() {
        let oversized = " ".repeat(MAX_INPUT_BYTES + 1);
        assert!(matches!(
            import_root_str(&oversized, None),
            Err(DtdError::InputTooLarge { .. })
        ));

        let nested = format!(
            "<!ELEMENT Root ({}Leaf{})><!ELEMENT Leaf EMPTY>",
            "(".repeat(MAX_NESTING_DEPTH + 1),
            ")".repeat(MAX_NESTING_DEPTH + 1)
        );
        assert!(matches!(
            import_root_str(&nested, Some("Root")),
            Err(DtdError::LimitExceeded {
                kind: "particle nesting depth",
                ..
            })
        ));

        let mut chain = String::new();
        for index in 0..=MAX_NESTING_DEPTH {
            let next = index + 1;
            chain.push_str(&format!("<!ELEMENT N{index} (N{next})>"));
        }
        chain.push_str(&format!("<!ELEMENT N{} EMPTY>", MAX_NESTING_DEPTH + 1));
        assert!(matches!(
            import_root_str(&chain, Some("N0")),
            Err(DtdError::LimitExceeded {
                kind: "schema expansion depth",
                ..
            })
        ));
    }
}
