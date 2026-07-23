//! Definition catalog parsing and derived lexical metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind, EdiValueConstraint};

use super::files::{Files, parse_document, resolve_sibling};
use super::{CompiledConfig, ConfigError};

const MAX_DEFINITIONS: usize = 20_000;

#[derive(Clone)]
pub(super) struct FieldDef {
    pub(super) kind: FieldKind,
    pub(super) reference: String,
    pub(super) node_name: Option<String>,
    pub(super) merged_entries: usize,
    pub(super) repeating: bool,
    pub(super) disabled: bool,
    pub(super) fixed: Option<String>,
    pub(super) inline_fields: Option<Vec<FieldDef>>,
    pub(super) inline_type: Option<ScalarType>,
    pub(super) inline_implicit_decimals: Option<u8>,
    pub(super) inline_lexical_kind: Option<EdiLexicalKind>,
    inline_value_constraint: ValueConstraintSpec,
}

#[derive(Clone, Copy)]
pub(super) enum FieldKind {
    Data,
    Composite,
}

#[derive(Clone)]
pub(super) struct DataDef {
    pub(super) name: String,
    pub(super) ty: ScalarType,
    implicit_decimals: Option<u8>,
    lexical_kind: Option<EdiLexicalKind>,
    value_constraint: ValueConstraintSpec,
}

#[derive(Clone, Default)]
struct ValueConstraintSpec {
    min_chars: Option<u16>,
    max_chars: Option<u16>,
    allowed_values: Option<Vec<String>>,
}

#[derive(Clone, PartialEq, Eq)]
struct ValueConstraintDef {
    min_chars: u16,
    max_chars: u16,
    allowed_values: Vec<String>,
}

impl ValueConstraintSpec {
    fn merged_with(&self, inherited: Option<&Self>) -> Option<ValueConstraintDef> {
        let has_constraint = self.min_chars.is_some()
            || self.max_chars.is_some()
            || self.allowed_values.is_some()
            || inherited.is_some_and(|constraint| {
                constraint.min_chars.is_some()
                    || constraint.max_chars.is_some()
                    || constraint.allowed_values.is_some()
            });
        has_constraint.then(|| ValueConstraintDef {
            min_chars: self
                .min_chars
                .or_else(|| inherited.and_then(|constraint| constraint.min_chars))
                .unwrap_or(0),
            max_chars: self
                .max_chars
                .or_else(|| inherited.and_then(|constraint| constraint.max_chars))
                .unwrap_or(u16::MAX),
            allowed_values: self
                .allowed_values
                .clone()
                .or_else(|| inherited.and_then(|constraint| constraint.allowed_values.clone()))
                .unwrap_or_default(),
        })
    }
}

#[derive(Clone)]
pub(super) struct CompositeDef {
    pub(super) name: String,
    pub(super) fields: Vec<FieldDef>,
}

#[derive(Clone)]
pub(super) struct SegmentDef {
    pub(super) name: String,
    pub(super) fields: Vec<FieldDef>,
}

#[derive(Default)]
pub(super) struct Definitions {
    pub(super) data: BTreeMap<String, DataDef>,
    pub(super) composites: BTreeMap<String, CompositeDef>,
    pub(super) segments: BTreeMap<String, SegmentDef>,
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
                insert_unambiguous_name(&mut names, &mut ambiguous, &data.name, kind);
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

    fn value_constraint_names(&self) -> BTreeMap<String, ValueConstraintDef> {
        let mut names = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for data in self.data.values() {
            if let Some(constraint) = data.value_constraint.merged_with(None) {
                insert_unambiguous_name(&mut names, &mut ambiguous, &data.name, constraint);
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
            collect_constraint_aliases(fields, self, &mut names, &mut ambiguous);
        }
        names
    }
}

pub(super) fn load_definitions(
    path: &Path,
    files: &mut Files,
    definitions: &mut Definitions,
) -> Result<(), ConfigError> {
    let canonical = std::fs::canonicalize(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if files.contains(&canonical) {
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
                            value_constraint: read_value_constraint(node)?,
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

pub(super) fn read_field_defs(node: roxmltree::Node<'_, '_>) -> Result<Vec<FieldDef>, ConfigError> {
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
                inline_value_constraint: read_value_constraint(child)?,
            })
        })
        .collect()
}

pub(super) fn has_multiple_occurrences(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute("maxOccurs")
        .is_some_and(|value| value == "unbounded" || value.parse::<usize>().is_ok_and(|n| n > 1))
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

fn read_value_constraint(
    node: roxmltree::Node<'_, '_>,
) -> Result<ValueConstraintSpec, ConfigError> {
    let read_width = |attribute| {
        node.attribute(attribute)
            .map(|raw| {
                raw.parse::<u16>().map_err(|_| {
                    ConfigError::Invalid(format!("Data field has invalid {attribute} `{raw}`"))
                })
            })
            .transpose()
    };
    let min_chars = read_width("minLength")?;
    let max_chars = read_width("maxLength")?;
    if max_chars == Some(0) || min_chars.zip(max_chars).is_some_and(|(min, max)| min > max) {
        return Err(ConfigError::Invalid(
            "Data field requires 0 <= minLength <= maxLength and a positive maxLength".into(),
        ));
    }
    let allowed_values = node
        .children()
        .find(|child| child.has_tag_name("Values"))
        .map(|values| {
            let mut codes = values
                .children()
                .filter(|child| child.has_tag_name("Value"))
                .filter_map(|child| child.attribute("Code"))
                .map(str::to_string)
                .collect::<Vec<_>>();
            codes.sort();
            codes.dedup();
            codes
        })
        .filter(|codes| !codes.is_empty());
    if allowed_values
        .as_ref()
        .is_some_and(|codes| codes.len() > 4096)
    {
        return Err(ConfigError::Limit("allowed values per Data field"));
    }
    Ok(ValueConstraintSpec {
        min_chars,
        max_chars,
        allowed_values,
    })
}

fn scalar_type(name: &str) -> ScalarType {
    match name.to_ascii_lowercase().as_str() {
        "decimal" | "float" | "double" | "number" => ScalarType::Float,
        "integer" | "int" | "long" | "short" => ScalarType::Int,
        "boolean" | "bool" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

pub(super) fn compiled_config(schema: SchemaNode, definitions: &Definitions) -> CompiledConfig {
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
    let constraint_names = definitions.value_constraint_names();
    let mut value_constraints = Vec::new();
    collect_value_constraint_paths(
        &schema,
        &constraint_names,
        &mut Vec::new(),
        true,
        &mut value_constraints,
    );
    CompiledConfig {
        schema,
        implied_decimals,
        lexical_formats,
        value_constraints,
    }
}

fn collect_value_constraint_paths(
    node: &SchemaNode,
    names: &BTreeMap<String, ValueConstraintDef>,
    path: &mut Vec<String>,
    root: bool,
    output: &mut Vec<EdiValueConstraint>,
) {
    if !root {
        path.push(node.name.clone());
    }
    match &node.kind {
        SchemaKind::Scalar { .. } => {
            if let Some(constraint) = names.get(&node.name)
                && let Some(constraint) = EdiValueConstraint::new(
                    path.clone(),
                    constraint.min_chars,
                    constraint.max_chars,
                    constraint.allowed_values.clone(),
                )
            {
                output.push(constraint);
            }
        }
        SchemaKind::Group { children, .. } => {
            for child in children {
                collect_value_constraint_paths(child, names, path, false, output);
            }
        }
    }
    if !root {
        path.pop();
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
                    insert_unambiguous_name(names, ambiguous, &name, kind);
                }
            }
        }
        if let Some(inline) = &field.inline_fields {
            collect_lexical_aliases(inline, definitions, names, ambiguous);
        }
    }
}

fn collect_constraint_aliases(
    fields: &[FieldDef],
    definitions: &Definitions,
    names: &mut BTreeMap<String, ValueConstraintDef>,
    ambiguous: &mut BTreeSet<String>,
) {
    for field in fields {
        if matches!(field.kind, FieldKind::Data) {
            let data = definitions.data.get(&field.reference);
            let constraint = field
                .inline_value_constraint
                .merged_with(data.map(|data| &data.value_constraint));
            if let Some(constraint) = constraint {
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
                    insert_unambiguous_name(names, ambiguous, &name, constraint.clone());
                }
            }
        }
        if let Some(inline) = &field.inline_fields {
            collect_constraint_aliases(inline, definitions, names, ambiguous);
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

fn insert_unambiguous_name<T: Clone + Eq>(
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
