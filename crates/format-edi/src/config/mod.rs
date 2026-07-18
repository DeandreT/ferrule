//! Bounded import of MapForce-style EDI configuration files.
//!
//! The configuration is a library of positional data, composite, and
//! segment definitions plus a message/envelope tree. Import expands that
//! library into ferrule's ordinary EDI [`SchemaNode`] representation so the
//! original configuration is not needed at execution time.

pub mod idoc;
pub mod swift;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind};
use thiserror::Error;

const MAX_FILES: usize = 32;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEFINITIONS: usize = 20_000;
const MAX_SCHEMA_NODES: usize = 40_000;
const MAX_DEPTH: usize = 128;
const MAX_MESSAGE_SCAN_FILES: usize = 512;
const MAX_MESSAGE_SCAN_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read EDI configuration `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse EDI configuration `{path}`: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("invalid EDI configuration: {0}")]
    Invalid(String),
    #[error("EDI configuration exceeds the {0} limit")]
    Limit(&'static str),
}

/// Imports one complete EDI configuration.
///
/// A message configuration is wrapped in its sibling `Envelope.Config`.
/// An envelope configuration uses `selected_messages` to resolve and embed
/// each concrete message selected by the `.mfd` component.
pub struct CompiledConfig {
    pub schema: SchemaNode,
    pub implied_decimals: Vec<EdiImpliedDecimal>,
    pub lexical_formats: Vec<EdiLexicalFormat>,
}

pub fn import_config(
    path: &Path,
    selected_messages: &[String],
) -> Result<CompiledConfig, ConfigError> {
    let mut files = Files::default();
    let mut definitions = Definitions::default();
    load_definitions(path, &mut files, &mut definitions)?;

    let main_text = files.read(path)?;
    let main_doc = parse_document(path, &main_text)?;
    let root = main_doc.root_element();
    let standard = root
        .children()
        .find(|node| node.has_tag_name("Format"))
        .and_then(|node| node.attribute("standard"))
        .ok_or_else(|| ConfigError::Invalid("configuration has no Format/@standard".into()))?;
    let mut schema_nodes = 0usize;

    if let Some(message_layout) = root.children().find(|node| node.has_tag_name("Message")) {
        let mut message = build_message(
            message_layout,
            &definitions,
            MessageName::Canonical,
            None,
            &mut schema_nodes,
        )?;
        if standard.eq_ignore_ascii_case("HL7") {
            let message_type = message_layout
                .children()
                .find(|node| node.has_tag_name("MessageType"))
                .and_then(|node| node.text())
                .ok_or_else(|| ConfigError::Invalid("HL7 Message has no MessageType".into()))?;
            message.name = message_type.to_string();
            return Ok(compiled_config(message, &definitions));
        }
        let envelope_path = resolve_sibling(path, "Envelope.Config")?;
        load_definitions(&envelope_path, &mut files, &mut definitions)?;
        let envelope_text = files.read(&envelope_path)?;
        let envelope_doc = parse_document(&envelope_path, &envelope_text)?;
        let schema = build_envelope(
            envelope_doc.root_element(),
            standard,
            vec![message],
            &definitions,
            &mut schema_nodes,
        )?;
        return Ok(compiled_config(schema, &definitions));
    }

    let envelope = root
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| {
            ConfigError::Invalid("configuration has no Message or Group layout".into())
        })?;
    if selected_messages.is_empty() {
        return Err(ConfigError::Invalid(
            "envelope configuration has no selected message types".into(),
        ));
    }

    let select = envelope
        .descendants()
        .find(|node| node.has_tag_name("Select"))
        .ok_or_else(|| ConfigError::Invalid("envelope has no message Select".into()))?;
    let discriminator = select.attribute("field").map(str::to_string);
    let mut messages = Vec::with_capacity(selected_messages.len());
    for selected in selected_messages {
        let message_path = resolve_message_config(path, selected)?;
        load_definitions(&message_path, &mut files, &mut definitions)?;
        let message_text = files.read(&message_path)?;
        let message_doc = parse_document(&message_path, &message_text)?;
        let message = message_doc
            .root_element()
            .children()
            .find(|node| node.has_tag_name("Message"))
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "selected message `{selected}` has no Message layout"
                ))
            })?;
        messages.push(build_message(
            message,
            &definitions,
            MessageName::Declared,
            discriminator
                .as_deref()
                .map(|path| (path, selected.as_str())),
            &mut schema_nodes,
        )?);
    }
    let schema = build_envelope(root, standard, messages, &definitions, &mut schema_nodes)?;
    Ok(compiled_config(schema, &definitions))
}

#[derive(Default)]
struct Files {
    paths: BTreeSet<PathBuf>,
    total_bytes: usize,
}

impl Files {
    fn read(&mut self, path: &Path) -> Result<String, ConfigError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let text = read_bounded_text(&canonical, MAX_TOTAL_BYTES, "total input size")?;
        if self.paths.insert(canonical) {
            if self.paths.len() > MAX_FILES {
                return Err(ConfigError::Limit("included file count"));
            }
            self.total_bytes = self
                .total_bytes
                .checked_add(text.len())
                .ok_or(ConfigError::Limit("total input size"))?;
            if self.total_bytes > MAX_TOTAL_BYTES {
                return Err(ConfigError::Limit("total input size"));
            }
        }
        Ok(text)
    }
}

fn read_bounded_text(
    path: &Path,
    max_bytes: usize,
    limit: &'static str,
) -> Result<String, ConfigError> {
    let file = std::fs::File::open(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut text = String::new();
    file.take((max_bytes + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if text.len() > max_bytes {
        return Err(ConfigError::Limit(limit));
    }
    Ok(text)
}

#[derive(Clone)]
struct FieldDef {
    kind: FieldKind,
    reference: String,
    node_name: Option<String>,
    merged_entries: usize,
    repeating: bool,
    disabled: bool,
    fixed: Option<String>,
    inline_fields: Option<Vec<FieldDef>>,
    inline_type: Option<ScalarType>,
    inline_implicit_decimals: Option<u8>,
    inline_lexical_kind: Option<EdiLexicalKind>,
}

#[derive(Clone, Copy)]
enum FieldKind {
    Data,
    Composite,
}

#[derive(Clone)]
struct DataDef {
    name: String,
    ty: ScalarType,
    implicit_decimals: Option<u8>,
    lexical_kind: Option<EdiLexicalKind>,
}

#[derive(Clone)]
struct CompositeDef {
    name: String,
    fields: Vec<FieldDef>,
}

#[derive(Clone)]
struct SegmentDef {
    name: String,
    fields: Vec<FieldDef>,
}

#[derive(Default)]
struct Definitions {
    data: BTreeMap<String, DataDef>,
    composites: BTreeMap<String, CompositeDef>,
    segments: BTreeMap<String, SegmentDef>,
}

impl Definitions {
    fn len(&self) -> usize {
        self.data.len() + self.composites.len() + self.segments.len()
    }

    fn implied_decimal_names(&self) -> BTreeMap<String, u8> {
        let mut names = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for data in self.data.values() {
            if let Some(places) = data.implicit_decimals {
                insert_decimal_name(&mut names, &mut ambiguous, &data.name, places);
            }
        }
        for fields in self
            .segments
            .values()
            .map(|segment| segment.fields.as_slice())
            .chain(
                self.composites
                    .values()
                    .map(|composite| composite.fields.as_slice()),
            )
        {
            collect_decimal_aliases(fields, self, &mut names, &mut ambiguous);
        }
        names
    }

    fn lexical_format_names(&self) -> BTreeMap<String, EdiLexicalKind> {
        let mut names = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for data in self.data.values() {
            if let Some(kind) = data.lexical_kind {
                insert_named_format(&mut names, &mut ambiguous, &data.name, kind);
            }
        }
        for fields in self
            .segments
            .values()
            .map(|segment| segment.fields.as_slice())
            .chain(
                self.composites
                    .values()
                    .map(|composite| composite.fields.as_slice()),
            )
        {
            collect_lexical_aliases(fields, self, &mut names, &mut ambiguous);
        }
        names
    }
}

fn load_definitions(
    path: &Path,
    files: &mut Files,
    definitions: &mut Definitions,
) -> Result<(), ConfigError> {
    let canonical = std::fs::canonicalize(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if files.paths.contains(&canonical) {
        return Ok(());
    }
    let text = files.read(&canonical)?;
    let doc = parse_document(&canonical, &text)?;
    let root = doc.root_element();
    if let Some(elements) = root.children().find(|node| node.has_tag_name("Elements")) {
        for node in elements.children().filter(roxmltree::Node::is_element) {
            let Some(name) = node.attribute("name") else {
                continue;
            };
            let key = node.attribute("id").unwrap_or(name);
            match node.tag_name().name() {
                "Data" if node.attribute("type").is_some() => {
                    definitions.data.insert(
                        key.to_string(),
                        DataDef {
                            name: name.to_string(),
                            ty: scalar_type(node.attribute("type").unwrap_or("string")),
                            implicit_decimals: read_implicit_decimals(node)?,
                            lexical_kind: read_lexical_kind(node)?,
                        },
                    );
                }
                "Composite" | "SubComposite" => {
                    definitions.composites.insert(
                        key.to_string(),
                        CompositeDef {
                            name: name.to_string(),
                            fields: read_field_defs(node)?,
                        },
                    );
                }
                "Segment" => {
                    definitions.segments.insert(
                        key.to_string(),
                        SegmentDef {
                            name: name.to_string(),
                            fields: read_field_defs(node)?,
                        },
                    );
                }
                _ => {}
            }
            if definitions.len() > MAX_DEFINITIONS {
                return Err(ConfigError::Limit("definition count"));
            }
        }
    }

    let includes = root
        .children()
        .filter(|node| node.has_tag_name("Include"))
        .filter_map(|node| node.attribute("href"))
        .filter(|href| href.to_ascii_lowercase().ends_with(".segment"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    for include in includes {
        let include_path = resolve_sibling(&canonical, &include)?;
        load_definitions(&include_path, files, definitions)?;
    }
    Ok(())
}

fn read_field_defs(node: roxmltree::Node<'_, '_>) -> Result<Vec<FieldDef>, ConfigError> {
    node.children()
        .filter(roxmltree::Node::is_element)
        .filter(|child| {
            matches!(
                child.tag_name().name(),
                "Data" | "Composite" | "SubComposite"
            )
        })
        .map(|child| {
            let kind = if child.has_tag_name("Data") {
                FieldKind::Data
            } else {
                FieldKind::Composite
            };
            let reference = child
                .attribute("ref")
                .or_else(|| child.attribute("name"))
                .ok_or_else(|| {
                    ConfigError::Invalid(format!(
                        "{} field has no ref or name",
                        child.tag_name().name()
                    ))
                })?;
            let node_name = child.attribute("nodeName").map(str::to_string);
            let merged_entries = child
                .attribute("mergedEntries")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1);
            if merged_entries == 0 || merged_entries > 256 {
                return Err(ConfigError::Invalid(format!(
                    "field `{reference}` has invalid mergedEntries"
                )));
            }
            Ok(FieldDef {
                kind,
                reference: reference.to_string(),
                node_name,
                merged_entries,
                repeating: has_multiple_occurrences(child),
                disabled: child.attribute("maxOccurs") == Some("0"),
                fixed: single_allowed_value(child),
                inline_fields: (!child.has_tag_name("Data"))
                    .then(|| read_field_defs(child))
                    .transpose()?
                    .filter(|fields| !fields.is_empty()),
                inline_type: child.attribute("type").map(scalar_type),
                inline_implicit_decimals: read_implicit_decimals(child)?,
                inline_lexical_kind: read_lexical_kind(child)?,
            })
        })
        .collect()
}

fn read_lexical_kind(node: roxmltree::Node<'_, '_>) -> Result<Option<EdiLexicalKind>, ConfigError> {
    let ty = node.attribute("type").unwrap_or_default();
    if !matches!(
        ty.to_ascii_lowercase().as_str(),
        "date" | "time" | "decimal"
    ) {
        return Ok(None);
    }
    let read_width = |attribute| {
        node.attribute(attribute)
            .map(|raw| {
                raw.parse::<u8>().map_err(|_| {
                    ConfigError::Invalid(format!("Data field has invalid {attribute} `{raw}`"))
                })
            })
            .transpose()
    };
    let min = read_width("minLength")?;
    let max = read_width("maxLength")?;
    match ty.to_ascii_lowercase().as_str() {
        "date" => match (min, max) {
            (Some(6), Some(6)) => Ok(Some(EdiLexicalKind::CompactDate6)),
            (Some(8), Some(8)) => Ok(Some(EdiLexicalKind::CompactDate8)),
            (None, None) => Ok(None),
            _ => Err(ConfigError::Invalid(
                "date Data fields must declare an exact compact length of 6 or 8".into(),
            )),
        },
        "time" => match (min, max) {
            (Some(min_digits), Some(max_digits))
                if EdiLexicalFormat::new(
                    vec!["field".into()],
                    EdiLexicalKind::CompactTime {
                        min_digits,
                        max_digits,
                    },
                )
                .is_some() =>
            {
                Ok(Some(EdiLexicalKind::CompactTime {
                    min_digits,
                    max_digits,
                }))
            }
            (None, None) => Ok(None),
            _ => Err(ConfigError::Invalid(
                "time Data fields must declare compact lengths within 4..=8".into(),
            )),
        },
        "decimal" if read_implicit_decimals(node)?.is_some() => Ok(None),
        "decimal" => match max {
            Some(max_chars) if max_chars > 0 => Ok(Some(EdiLexicalKind::Decimal { max_chars })),
            None => Ok(None),
            _ => Err(ConfigError::Invalid(
                "decimal Data fields must declare a positive maxLength".into(),
            )),
        },
        _ => Ok(None),
    }
}

fn read_implicit_decimals(node: roxmltree::Node<'_, '_>) -> Result<Option<u8>, ConfigError> {
    let Some(raw) = node.attribute("implicitDecimals") else {
        return Ok(None);
    };
    let places = raw.parse::<u8>().map_err(|_| {
        ConfigError::Invalid(format!("Data field has invalid implicitDecimals `{raw}`"))
    })?;
    if places == 0 {
        return Ok(None);
    }
    if places > 18 {
        return Err(ConfigError::Invalid(format!(
            "Data field implicitDecimals `{raw}` exceeds the 18-place runtime limit"
        )));
    }
    Ok(Some(places))
}

fn single_allowed_value(node: roxmltree::Node<'_, '_>) -> Option<String> {
    let values = node.children().find(|child| child.has_tag_name("Values"))?;
    let mut codes = values
        .children()
        .filter(|child| child.has_tag_name("Value"))
        .filter_map(|child| child.attribute("Code"));
    let only = codes.next()?;
    codes.next().is_none().then(|| only.to_string())
}

fn scalar_type(name: &str) -> ScalarType {
    match name.to_ascii_lowercase().as_str() {
        "decimal" | "float" | "double" | "number" => ScalarType::Float,
        "integer" | "int" | "long" | "short" => ScalarType::Int,
        "boolean" | "bool" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

fn compiled_config(schema: SchemaNode, definitions: &Definitions) -> CompiledConfig {
    let names = definitions.implied_decimal_names();
    let mut implied_decimals = Vec::new();
    collect_implied_decimal_paths(
        &schema,
        &names,
        &mut Vec::new(),
        true,
        &mut implied_decimals,
    );
    let lexical_names = definitions.lexical_format_names();
    let mut lexical_formats = Vec::new();
    collect_lexical_format_paths(
        &schema,
        &lexical_names,
        &mut Vec::new(),
        true,
        &mut lexical_formats,
    );
    CompiledConfig {
        schema,
        implied_decimals,
        lexical_formats,
    }
}

fn collect_lexical_format_paths(
    node: &SchemaNode,
    names: &BTreeMap<String, EdiLexicalKind>,
    path: &mut Vec<String>,
    root: bool,
    output: &mut Vec<EdiLexicalFormat>,
) {
    if !root {
        path.push(node.name.clone());
    }
    match &node.kind {
        SchemaKind::Scalar { .. } => {
            if let Some(kind) = names.get(&node.name)
                && let Some(format) = EdiLexicalFormat::new(path.clone(), *kind)
            {
                output.push(format);
            }
        }
        SchemaKind::Group { children, .. } => {
            for child in children {
                collect_lexical_format_paths(child, names, path, false, output);
            }
        }
    }
    if !root {
        path.pop();
    }
}

fn collect_implied_decimal_paths(
    node: &SchemaNode,
    names: &BTreeMap<String, u8>,
    path: &mut Vec<String>,
    root: bool,
    output: &mut Vec<EdiImpliedDecimal>,
) {
    if !root {
        path.push(node.name.clone());
    }
    match &node.kind {
        SchemaKind::Scalar { .. } => {
            if let Some(places) = names.get(&node.name)
                && let Some(format) = EdiImpliedDecimal::new(path.clone(), *places)
            {
                output.push(format);
            }
        }
        SchemaKind::Group { children, .. } => {
            for child in children {
                collect_implied_decimal_paths(child, names, path, false, output);
            }
        }
    }
    if !root {
        path.pop();
    }
}

fn collect_decimal_aliases(
    fields: &[FieldDef],
    definitions: &Definitions,
    names: &mut BTreeMap<String, u8>,
    ambiguous: &mut BTreeSet<String>,
) {
    for field in fields {
        if matches!(field.kind, FieldKind::Data) {
            let data = definitions.data.get(&field.reference);
            let places = field
                .inline_implicit_decimals
                .or_else(|| data.and_then(|data| data.implicit_decimals));
            if let Some(places) = places {
                let base_name = field
                    .node_name
                    .as_deref()
                    .or_else(|| data.map(|data| data.name.as_str()))
                    .unwrap_or(&field.reference);
                for index in 0..field.merged_entries {
                    let name = if index == 0 {
                        base_name.to_string()
                    } else {
                        format!("{base_name}_{}", index + 1)
                    };
                    insert_decimal_name(names, ambiguous, &name, places);
                }
            }
        }
        if let Some(inline) = &field.inline_fields {
            collect_decimal_aliases(inline, definitions, names, ambiguous);
        }
    }
}

fn collect_lexical_aliases(
    fields: &[FieldDef],
    definitions: &Definitions,
    names: &mut BTreeMap<String, EdiLexicalKind>,
    ambiguous: &mut BTreeSet<String>,
) {
    for field in fields {
        if matches!(field.kind, FieldKind::Data) {
            let data = definitions.data.get(&field.reference);
            let kind = field
                .inline_lexical_kind
                .or_else(|| data.and_then(|data| data.lexical_kind));
            if let Some(kind) = kind {
                let base_name = field
                    .node_name
                    .as_deref()
                    .or_else(|| data.map(|data| data.name.as_str()))
                    .unwrap_or(&field.reference);
                for index in 0..field.merged_entries {
                    let name = if index == 0 {
                        base_name.to_string()
                    } else {
                        format!("{base_name}_{}", index + 1)
                    };
                    insert_named_format(names, ambiguous, &name, kind);
                }
            }
        }
        if let Some(inline) = &field.inline_fields {
            collect_lexical_aliases(inline, definitions, names, ambiguous);
        }
    }
}

fn insert_decimal_name(
    names: &mut BTreeMap<String, u8>,
    ambiguous: &mut BTreeSet<String>,
    name: &str,
    places: u8,
) {
    if ambiguous.contains(name) {
        return;
    }
    match names.get(name) {
        Some(existing) if *existing != places => {
            names.remove(name);
            ambiguous.insert(name.to_string());
        }
        Some(_) => {}
        None => {
            names.insert(name.to_string(), places);
        }
    }
}

fn insert_named_format<T: Copy + Eq>(
    names: &mut BTreeMap<String, T>,
    ambiguous: &mut BTreeSet<String>,
    name: &str,
    format: T,
) {
    if ambiguous.contains(name) {
        return;
    }
    match names.get(name) {
        Some(existing) if *existing != format => {
            names.remove(name);
            ambiguous.insert(name.to_string());
        }
        Some(_) => {}
        None => {
            names.insert(name.to_string(), format);
        }
    }
}

#[derive(Clone, Copy)]
enum MessageName {
    Canonical,
    Declared,
}

fn build_message(
    message: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    naming: MessageName,
    discriminator: Option<(&str, &str)>,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let group = message
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("Message has no Group layout".into()))?;
    let mut built = build_group(group, definitions, &[], 0, count)?;
    if matches!(naming, MessageName::Canonical) {
        built.name = "Message".to_string();
    }
    if let Some((path, value)) = discriminator {
        if path == "@HL7" {
            set_hl7_message_fixed(&mut built, value)?;
        } else {
            set_fixed_descendant(&mut built, &path.split('/').collect::<Vec<_>>(), value)?;
        }
    }
    Ok(built)
}

fn set_fixed_descendant(
    node: &mut SchemaNode,
    path: &[&str],
    value: &str,
) -> Result<(), ConfigError> {
    if set_fixed_if_missing(node, path.iter().copied(), value).is_ok() {
        return Ok(());
    }
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path `{}` not found",
            path.join("/")
        )));
    };
    for child in children {
        if set_fixed_descendant(child, path, value).is_ok() {
            return Ok(());
        }
    }
    Err(ConfigError::Invalid(format!(
        "fixed-value path `{}` not found",
        path.join("/")
    )))
}

fn set_fixed_if_missing<'a>(
    node: &mut SchemaNode,
    mut path: impl Iterator<Item = &'a str>,
    value: &str,
) -> Result<(), ConfigError> {
    let Some(segment) = path.next() else {
        return Err(ConfigError::Invalid("empty fixed-value path".into()));
    };
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path crosses scalar `{}`",
            node.name
        )));
    };
    let child = children
        .iter_mut()
        .find(|child| child.name == segment)
        .ok_or_else(|| ConfigError::Invalid(format!("fixed-value path `{segment}` not found")))?;
    let remaining = path.collect::<Vec<_>>();
    if remaining.is_empty() {
        if child.fixed.is_none() {
            child.fixed = Some(value.to_string());
        }
        return Ok(());
    }
    set_fixed_if_missing(child, remaining.into_iter(), value)
}

fn build_envelope(
    root: roxmltree::Node<'_, '_>,
    standard: &str,
    messages: Vec<SchemaNode>,
    definitions: &Definitions,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let group = root
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("envelope has no Group layout".into()))?;
    let omitted_segments: &[&str] = if standard.eq_ignore_ascii_case("EDIFACT") {
        &["UNA", "UNG", "UNE"]
    } else if standard.eq_ignore_ascii_case("TRADACOMS") {
        &["BAT", "EOB"]
    } else {
        &[]
    };
    let mut schema =
        build_group_with_messages(group, definitions, omitted_segments, &messages, 0, count)?;
    if !standard.eq_ignore_ascii_case("HL7") {
        schema.name = "Envelope".to_string();
    }
    Ok(schema)
}

fn set_hl7_message_fixed(message: &mut SchemaNode, message_type: &str) -> Result<(), ConfigError> {
    let (code, trigger) = message_type.split_once('_').ok_or_else(|| {
        ConfigError::Invalid(format!(
            "HL7 message type `{message_type}` has no code/trigger separator"
        ))
    })?;
    for (path, value) in [
        ("MSH/MSH-9/MSG-1", code),
        ("MSH/MSH-9/MSG-2", trigger),
        ("MSH/MSH-9/MSG-3", message_type),
    ] {
        set_fixed(message, path.split('/'), value)?;
    }
    Ok(())
}

fn build_group(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    omitted_envelope_segments: &[&str],
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    build_group_with_messages(
        node,
        definitions,
        omitted_envelope_segments,
        &[],
        depth,
        count,
    )
}

fn build_group_with_messages(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    omitted_envelope_segments: &[&str],
    messages: &[SchemaNode],
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    check_depth(depth)?;
    let content = if let Some(reference) = node.attribute("ref") {
        node.document()
            .descendants()
            .find(|candidate| {
                candidate.has_tag_name("Group") && candidate.attribute("id") == Some(reference)
            })
            .ok_or_else(|| ConfigError::Invalid(format!("unknown Group ref `{reference}`")))?
    } else {
        node
    };
    let name = node
        .attribute("name")
        .or_else(|| content.attribute("name"))
        .ok_or_else(|| ConfigError::Invalid("Group has no name".into()))?;
    let mut children = Vec::new();
    for child in content.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "Group" => children.push(build_group_with_messages(
                child,
                definitions,
                omitted_envelope_segments,
                messages,
                depth + 1,
                count,
            )?),
            "Segment" => {
                let segment_name = child
                    .attribute("ref")
                    .or_else(|| child.attribute("name"))
                    .unwrap_or_default();
                if omitted_envelope_segments.contains(&segment_name) {
                    continue;
                }
                children.push(build_segment(child, definitions, depth + 1, count)?);
            }
            "Select" => {
                // Each selected message is one alternative of the configured
                // choice. An alternative can therefore be absent even when
                // the Select itself is required, and a repeated Select can
                // contain the same alternative more than once.
                children.extend(messages.iter().cloned().map(|mut message| {
                    message.repeating = true;
                    message
                }));
            }
            _ => {}
        }
    }
    bump_count(count)?;
    let mut built = SchemaNode::group(name, children);
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_segment(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    check_depth(depth)?;
    let name = node
        .attribute("ref")
        .or_else(|| node.attribute("name"))
        .ok_or_else(|| ConfigError::Invalid("Segment has no ref or name".into()))?;
    let definition = if node.attribute("ref").is_some() {
        definitions
            .segments
            .get(name)
            .ok_or_else(|| ConfigError::Invalid(format!("unknown Segment ref `{name}`")))?
    } else {
        return build_inline_segment(node, definitions, depth, count);
    };
    let mut children = build_fields(&definition.fields, definitions, depth + 1, count)?;
    apply_conditions(node, &mut children)?;
    bump_count(count)?;
    let mut built = SchemaNode::group(
        node.attribute("nodeName").unwrap_or(&definition.name),
        children,
    );
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_inline_segment(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let name = node
        .attribute("name")
        .ok_or_else(|| ConfigError::Invalid("inline Segment has no name".into()))?;
    let fields = read_field_defs(node)?;
    let mut children = build_fields(&fields, definitions, depth + 1, count)?;
    apply_conditions(node, &mut children)?;
    bump_count(count)?;
    let mut built = SchemaNode::group(node.attribute("nodeName").unwrap_or(name), children);
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_fields(
    fields: &[FieldDef],
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<Vec<SchemaNode>, ConfigError> {
    check_depth(depth)?;
    let mut built = Vec::new();
    for field in fields {
        for index in 0..field.merged_entries {
            let mut child = if field.disabled {
                match field.kind {
                    FieldKind::Data => SchemaNode::scalar(
                        field.node_name.as_deref().unwrap_or(&field.reference),
                        ScalarType::String,
                    ),
                    FieldKind::Composite => SchemaNode::group(
                        field.node_name.as_deref().unwrap_or(&field.reference),
                        Vec::new(),
                    ),
                }
            } else {
                match field.kind {
                    FieldKind::Data => {
                        if let Some(ty) = field.inline_type {
                            SchemaNode::scalar(
                                field.node_name.as_deref().unwrap_or(&field.reference),
                                ty,
                            )
                        } else {
                            let data = definitions.data.get(&field.reference).ok_or_else(|| {
                                ConfigError::Invalid(format!(
                                    "unknown Data ref `{}`",
                                    field.reference
                                ))
                            })?;
                            SchemaNode::scalar(
                                field.node_name.as_deref().unwrap_or(&data.name),
                                data.ty,
                            )
                        }
                    }
                    FieldKind::Composite => {
                        if let Some(inline) = &field.inline_fields {
                            SchemaNode::group(
                                field.node_name.as_deref().unwrap_or(&field.reference),
                                build_fields(inline, definitions, depth + 1, count)?,
                            )
                        } else {
                            let composite = definitions
                                .composites
                                .get(&field.reference)
                                .ok_or_else(|| {
                                    ConfigError::Invalid(format!(
                                        "unknown Composite ref `{}`",
                                        field.reference
                                    ))
                                })?;
                            SchemaNode::group(
                                field.node_name.as_deref().unwrap_or(&composite.name),
                                build_fields(&composite.fields, definitions, depth + 1, count)?,
                            )
                        }
                    }
                }
            };
            if index > 0 {
                child.name = format!("{}_{}", child.name, index + 1);
            }
            child.repeating = field.repeating;
            child.fixed.clone_from(&field.fixed);
            bump_count(count)?;
            built.push(child);
        }
    }
    Ok(built)
}

fn apply_conditions(
    segment: roxmltree::Node<'_, '_>,
    children: &mut [SchemaNode],
) -> Result<(), ConfigError> {
    for condition in segment
        .children()
        .filter(|node| node.has_tag_name("Condition"))
    {
        let Some(path) = condition.attribute("path") else {
            continue;
        };
        let Some(value) = condition.attribute("value") else {
            continue;
        };
        let mut wrapper = SchemaNode::group("segment", children.to_vec());
        set_fixed(&mut wrapper, path.split('/'), value)?;
        let SchemaKind::Group {
            children: updated, ..
        } = wrapper.kind
        else {
            return Err(ConfigError::Invalid(
                "condition wrapper is not a group".into(),
            ));
        };
        children.clone_from_slice(&updated);
    }
    Ok(())
}

fn set_fixed<'a>(
    node: &mut SchemaNode,
    mut path: impl Iterator<Item = &'a str>,
    value: &str,
) -> Result<(), ConfigError> {
    let Some(segment) = path.next() else {
        return Err(ConfigError::Invalid("empty fixed-value path".into()));
    };
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path crosses scalar `{}`",
            node.name
        )));
    };
    let child = children
        .iter_mut()
        .find(|child| child.name == segment)
        .ok_or_else(|| ConfigError::Invalid(format!("fixed-value path `{segment}` not found")))?;
    let remaining = path.collect::<Vec<_>>();
    if remaining.is_empty() {
        child.fixed = Some(value.to_string());
        return Ok(());
    }
    set_fixed(child, remaining.into_iter(), value)
}

fn is_optional_or_multiple(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute("minOccurs") == Some("0") || has_multiple_occurrences(node)
}

fn has_multiple_occurrences(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute("maxOccurs")
        .is_some_and(|value| value == "unbounded" || value.parse::<usize>().is_ok_and(|n| n > 1))
}

fn bump_count(count: &mut usize) -> Result<(), ConfigError> {
    *count = count
        .checked_add(1)
        .ok_or(ConfigError::Limit("materialized schema node count"))?;
    if *count > MAX_SCHEMA_NODES {
        return Err(ConfigError::Limit("materialized schema node count"));
    }
    Ok(())
}

fn check_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_DEPTH {
        Err(ConfigError::Limit("layout nesting depth"))
    } else {
        Ok(())
    }
}

fn parse_document<'a>(path: &Path, text: &'a str) -> Result<roxmltree::Document<'a>, ConfigError> {
    roxmltree::Document::parse(text).map_err(|source| ConfigError::Xml {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_sibling(path: &Path, relative: &str) -> Result<PathBuf, ConfigError> {
    let portable = relative.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(ConfigError::Invalid(format!(
            "include path `{portable}` is not a bounded relative path"
        )));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_case_insensitive(base, relative).ok_or_else(|| {
        ConfigError::Invalid(format!(
            "configuration `{portable}` was not found beside `{}`",
            path.display()
        ))
    })
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(expected) = component else {
            continue;
        };
        let direct = current.join(expected);
        if direct.exists() {
            current = direct;
            continue;
        }
        let expected = expected.to_str()?;
        let mut matches = std::fs::read_dir(&current)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected))
            })
            .map(|entry| entry.path());
        let found = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = found;
    }
    current.is_file().then_some(current)
}

fn resolve_message_config(
    envelope_path: &Path,
    message_type: &str,
) -> Result<PathBuf, ConfigError> {
    let direct = format!("{message_type}.Config");
    if let Ok(path) = resolve_sibling(envelope_path, &direct) {
        return Ok(path);
    }
    let directory = envelope_path.parent().unwrap_or_else(|| Path::new("."));
    let entries = std::fs::read_dir(directory).map_err(|source| ConfigError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut files = 0usize;
    let mut bytes = 0usize;
    let mut found = None;
    for entry in entries {
        let entry = entry.map_err(|source| ConfigError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("Config") {
            continue;
        }
        files += 1;
        if files > MAX_MESSAGE_SCAN_FILES {
            return Err(ConfigError::Limit("message configuration scan file count"));
        }
        let text = read_bounded_text(
            &path,
            MAX_MESSAGE_SCAN_BYTES,
            "message configuration scan size",
        )?;
        bytes = bytes
            .checked_add(text.len())
            .ok_or(ConfigError::Limit("message configuration scan size"))?;
        if bytes > MAX_MESSAGE_SCAN_BYTES {
            return Err(ConfigError::Limit("message configuration scan size"));
        }
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let matches = doc
            .descendants()
            .any(|node| node.has_tag_name("MessageType") && node.text() == Some(message_type));
        if matches {
            if found.is_some() {
                return Err(ConfigError::Invalid(format!(
                    "message type `{message_type}` has multiple configuration files"
                )));
            }
            found = Some(path);
        }
    }
    found.ok_or_else(|| {
        ConfigError::Invalid(format!(
            "message type `{message_type}` has no sibling configuration"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_config_expands_positions_and_wraps_envelope() {
        let directory = temp_directory("message");
        write(
            &directory.join("Defs.Segment"),
            r#"<Config><Elements>
                <Data name="F1" type="string"/>
                <Data name="F2" type="decimal" implicitDecimals="2"/>
                <Data name="FDecimal" type="decimal" minLength="1" maxLength="10"/>
                <Data name="FDate" type="date" minLength="8" maxLength="8"/>
                <Data name="FTime" type="time" minLength="4" maxLength="8"/>
                <Composite name="C1" id="C1-GUIDE"><Data ref="F1"/><Data ref="F2"/></Composite>
                <Segment name="ISA"><Data ref="F1"/></Segment>
                <Segment name="GS"><Data ref="F1"/></Segment>
                <Segment name="ST"><Data ref="F1"/></Segment>
                <Segment name="N1" id="N1-GUIDE">
                  <Composite ref="C1-GUIDE"/>
                  <Data ref="F1" mergedEntries="2"><Values><Value Code="AA"/></Values></Data>
                  <Data ref="FDate"/><Data ref="FTime" nodeName="Clock"/><Data ref="FDecimal"/>
                  <Composite name="C-MISSING" minOccurs="0" maxOccurs="0"/>
                </Segment>
                <Segment name="SE"><Data ref="F2"/></Segment>
                <Segment name="GE"><Data ref="F1"/></Segment>
                <Segment name="IEA"><Data ref="F1"/></Segment>
            </Elements></Config>"#,
        );
        write(
            &directory.join("Envelope.Config"),
            r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
              <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
                <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
                  <Segment ref="GS"/><Select field="ST/F1"/><Segment ref="GE" minOccurs="0"/>
                </Group><Segment ref="IEA" minOccurs="0"/>
              </Group></Group></Config>"#,
        );
        let message_path = directory.join("850.Config");
        write(
            &message_path,
            r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
              <Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded">
                <Segment ref="ST"/><Segment ref="N1-GUIDE" maxOccurs="3"/><Segment ref="SE"/>
              </Group></Message></Config>"#,
        );

        let compiled = import_config(&message_path, &[]).unwrap();
        assert!(compiled.implied_decimals.iter().any(|format| {
            format.places() == 2 && format.path().last().is_some_and(|segment| segment == "F2")
        }));
        assert!(compiled.lexical_formats.iter().any(|format| {
            format.kind() == EdiLexicalKind::CompactDate8
                && format
                    .path()
                    .last()
                    .is_some_and(|segment| segment == "FDate")
        }));
        assert!(compiled.lexical_formats.iter().any(|format| {
            format.kind() == EdiLexicalKind::Decimal { max_chars: 10 }
                && format
                    .path()
                    .last()
                    .is_some_and(|segment| segment == "FDecimal")
        }));
        assert!(compiled.lexical_formats.iter().any(|format| {
            format.kind()
                == EdiLexicalKind::CompactTime {
                    min_digits: 4,
                    max_digits: 8,
                }
                && format
                    .path()
                    .last()
                    .is_some_and(|segment| segment == "Clock")
        }));
        let schema = compiled.schema;
        assert_eq!(schema.name, "Envelope");
        let message = at(&schema, &["Interchange", "Group", "Message"]);
        assert!(message.repeating);
        let amount = at(message, &["N1", "C1", "F2"]);
        assert!(matches!(
            amount.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Float
            }
        ));
        assert_eq!(at(message, &["N1", "F1"]).fixed.as_deref(), Some("AA"));
        assert!(at(message, &["N1", "F1_2"]).name == "F1_2");
        assert!(matches!(
            at(message, &["N1", "C-MISSING"]).kind,
            SchemaKind::Group { ref children, .. } if children.is_empty()
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn envelope_selection_finds_message_type_and_sets_trigger_qualifier() {
        let directory = temp_directory("selection");
        write(
            &directory.join("Defs.Segment"),
            r#"<Config><Elements>
              <Data name="F143" type="string"/><Data name="F1705" type="string"/>
              <Data name="F2" type="string"/>
              <Segment name="ISA"><Data ref="F2"/></Segment>
              <Segment name="GS"><Data ref="F2"/></Segment>
              <Segment name="ST"><Data ref="F143"/></Segment>
              <Segment name="SE"><Data ref="F2"/></Segment>
            </Elements></Config>"#,
        );
        let envelope = directory.join("Envelope.Config");
        write(
            &envelope,
            r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
              <Group name="Envelope"><Group name="Interchange"><Segment ref="ISA"/>
                <Group name="Group"><Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/></Group>
              </Group></Group></Config>"#,
        );
        write(
            &directory.join("dental.Config"),
            r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
              <Message><MessageType>837-Q2</MessageType><Group name="Message_837-Q2">
                <Segment name="ST"><Condition path="F1705" value="005010X224A2"/>
                  <Data ref="F143"><Values><Value Code="837"/></Values></Data>
                  <Data ref="F1705"/>
                </Segment><Segment ref="SE"/>
              </Group></Message></Config>"#,
        );

        let schema = import_config(&envelope, &["837-Q2".into()]).unwrap().schema;
        let message = at(&schema, &["Interchange", "Group", "Message_837-Q2"]);
        assert_eq!(at(message, &["ST", "F143"]).fixed.as_deref(), Some("837"));
        assert_eq!(
            at(message, &["ST", "F1705"]).fixed.as_deref(),
            Some("005010X224A2")
        );
        assert!(message.repeating);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn at<'a>(node: &'a SchemaNode, path: &[&str]) -> &'a SchemaNode {
        path.iter().fold(node, |current, segment| {
            let SchemaKind::Group { children, .. } = &current.kind else {
                panic!("{} is scalar", current.name);
            };
            children
                .iter()
                .find(|child| child.name == *segment)
                .unwrap()
        })
    }

    fn temp_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ferrule_edi_config_{label}_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }
}
