use std::collections::HashMap;

use ir::{
    MAX_JSON_UNIQUE_ITEMS, MAX_JSON_UNIQUE_KEY_BYTES, MAX_JSON_UNIQUE_KEY_NODES, SchemaKind,
    SchemaNode,
};
use serde_json::value::RawValue;

use super::unsupported_union;
use crate::JsonFormatError;

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("uniqueItems").is_some()
}

pub(super) fn selected(name: &str, schema: &serde_json::Value) -> Result<bool, JsonFormatError> {
    match schema.get("uniqueItems") {
        None => Ok(false),
        Some(serde_json::Value::Bool(value)) => Ok(*value),
        Some(_) => Err(unsupported_union(name, "uniqueItems must be a boolean")),
    }
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    ambiguous_without_array: bool,
) -> Result<(), JsonFormatError> {
    if !selected(name, schema)? {
        return Ok(());
    }
    if node.repeating {
        node.json_unique_items = true;
    } else if ambiguous_without_array {
        return Err(unsupported_union(
            name,
            "uniqueItems: true without a concrete array type also admits unconstrained non-array values",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    selected(name, schema)?;
    Ok(())
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if node.json_unique_items {
        out.insert("uniqueItems".into(), true.into());
    }
}

pub(crate) fn validate(
    schema: &SchemaNode,
    items: &[serde_json::Value],
) -> Result<(), JsonFormatError> {
    if !schema.json_unique_items {
        return Ok(());
    }
    validate_unique_json_items(items).map_err(|error| match error {
        UniqueItemsValidationError::Duplicate {
            first_index,
            duplicate_index,
        } => JsonFormatError::UniqueItemsMismatch {
            name: schema.name.clone(),
            first_index,
            duplicate_index,
        },
        UniqueItemsValidationError::Limit { resource, max } => JsonFormatError::UniqueItemsLimit {
            name: schema.name.clone(),
            resource,
            max,
        },
    })
}

/// Validates `uniqueItems` throughout one already-decoded JSON value.
///
/// This complements the lexical raw-document prepass for private predicate
/// schemas that are evaluated conditionally by `contains`.
pub(crate) fn validate_json_tree(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if schema.container_nullable && value.is_null() {
        return Ok(());
    }
    if !schema.repeating {
        return validate_json_single_node(schema, value);
    }
    let serde_json::Value::Array(items) = value else {
        return Ok(());
    };
    validate(schema, items)?;
    for item in items {
        validate_json_single_node(schema, item)?;
    }
    Ok(())
}

fn validate_json_single_node(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if schema.json_any || schema.container_nullable && value.is_null() {
        return Ok(());
    }
    let (
        SchemaKind::Group {
            children, dynamic, ..
        },
        serde_json::Value::Object(fields),
    ) = (&schema.kind, value)
    else {
        return Ok(());
    };
    for child in children {
        if let Some(value) = fields.get(&child.name) {
            validate_json_tree(child, value)?;
        }
    }
    if let Some(dynamic) = dynamic {
        for (name, value) in fields {
            if !children.iter().any(|child| child.name == *name) {
                validate_json_tree(dynamic, value)?;
            }
        }
    }
    Ok(())
}

/// Validates every exact JSON `uniqueItems` assertion in one raw document
/// before ordinary JSON number decoding can lose decimal lexical precision.
pub fn validate_raw_json_unique_items(
    schema: &SchemaNode,
    document: &str,
) -> Result<(), JsonFormatError> {
    if !tree_has_unique_items(schema) {
        return Ok(());
    }
    let document = document.strip_prefix('\u{feff}').unwrap_or(document);
    let raw = serde_json::from_str::<Box<RawValue>>(document)?;
    validate_raw_node(schema, &raw)
}

pub(crate) fn validate_raw_json_lines_unique_items(
    schema: &SchemaNode,
    lines: &[&str],
) -> Result<(), JsonFormatError> {
    if !tree_has_unique_items(schema) {
        return Ok(());
    }
    let raw = lines
        .iter()
        .map(|line| serde_json::from_str::<Box<RawValue>>(line))
        .collect::<Result<Vec<_>, _>>()?;
    if schema.json_unique_items {
        validate_raw_items(schema, &raw)?;
    }
    for item in &raw {
        validate_raw_single_node(schema, item)?;
    }
    Ok(())
}

fn tree_has_unique_items(schema: &SchemaNode) -> bool {
    schema.json_unique_items
        || match &schema.kind {
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
            SchemaKind::Group {
                children, dynamic, ..
            } => {
                children.iter().any(tree_has_unique_items)
                    || dynamic.as_deref().is_some_and(tree_has_unique_items)
            }
        }
}

fn validate_raw_node(schema: &SchemaNode, raw: &RawValue) -> Result<(), JsonFormatError> {
    if schema.container_nullable && raw.get().trim() == "null" {
        return Ok(());
    }
    if !schema.repeating {
        return validate_raw_single_node(schema, raw);
    }
    if !raw.get().trim_start().starts_with('[') {
        return Ok(());
    }
    let items = serde_json::from_str::<Vec<Box<RawValue>>>(raw.get())?;
    if schema.json_unique_items {
        validate_raw_items(schema, &items)?;
    }
    for item in &items {
        validate_raw_single_node(schema, item)?;
    }
    Ok(())
}

fn validate_raw_single_node(schema: &SchemaNode, raw: &RawValue) -> Result<(), JsonFormatError> {
    if schema.json_any
        || schema.container_nullable && raw.get().trim() == "null"
        || !matches!(schema.kind, SchemaKind::Group { .. })
        || !raw.get().trim_start().starts_with('{')
    {
        return Ok(());
    }
    let fields =
        serde_json::from_str::<std::collections::BTreeMap<String, Box<RawValue>>>(raw.get())?;
    let SchemaKind::Group {
        children, dynamic, ..
    } = &schema.kind
    else {
        return Ok(());
    };
    for child in children {
        if let Some(value) = fields.get(&child.name) {
            validate_raw_node(child, value)?;
        }
    }
    if let Some(dynamic) = dynamic {
        for (name, value) in &fields {
            if !children.iter().any(|child| child.name == *name) {
                validate_raw_node(dynamic, value)?;
            }
        }
    }
    Ok(())
}

fn validate_raw_items(schema: &SchemaNode, items: &[Box<RawValue>]) -> Result<(), JsonFormatError> {
    if items.len() > MAX_JSON_UNIQUE_ITEMS {
        return Err(JsonFormatError::UniqueItemsLimit {
            name: schema.name.clone(),
            resource: "array items",
            max: MAX_JSON_UNIQUE_ITEMS,
        });
    }
    let mut budget = KeyBudget::default();
    let mut seen = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let key = SemanticKey::from_raw(item, &mut budget)
            .map_err(|error| map_raw_key_error(schema, error))?;
        let bytes = key
            .canonical_json_len()
            .map_err(|error| map_unique_error(schema, error))?;
        budget
            .add_bytes(bytes)
            .map_err(|error| map_unique_error(schema, error))?;
        let item_index = index + 1;
        if let Some(first_index) = seen.insert(key, item_index) {
            return Err(JsonFormatError::UniqueItemsMismatch {
                name: schema.name.clone(),
                first_index,
                duplicate_index: item_index,
            });
        }
    }
    Ok(())
}

/// One exact, bounded JSON `uniqueItems` validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniqueItemsValidationError {
    Duplicate {
        first_index: usize,
        duplicate_index: usize,
    },
    Limit {
        resource: &'static str,
        max: usize,
    },
}

/// Validates one normalized JSON array using exact JSON Schema value equality.
///
/// Object member order is ignored, array order is retained, and decimal
/// numbers are compared mathematically without passing through `f64`.
pub fn validate_unique_json_items(
    items: &[serde_json::Value],
) -> Result<(), UniqueItemsValidationError> {
    if items.len() > MAX_JSON_UNIQUE_ITEMS {
        return Err(limit("array items", MAX_JSON_UNIQUE_ITEMS));
    }
    let mut budget = KeyBudget::default();
    let mut seen = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let key = SemanticKey::from_json(item, &mut budget)?;
        budget.add_bytes(key.canonical_json_len()?)?;
        let item_index = index + 1;
        if let Some(first_index) = seen.insert(key, item_index) {
            return Err(UniqueItemsValidationError::Duplicate {
                first_index,
                duplicate_index: item_index,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SemanticKey {
    Null,
    Bool(bool),
    Number(CanonicalNumber),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

impl SemanticKey {
    fn from_json(
        value: &serde_json::Value,
        budget: &mut KeyBudget,
    ) -> Result<Self, UniqueItemsValidationError> {
        budget.add_node()?;
        match value {
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Bool(value) => Ok(Self::Bool(*value)),
            serde_json::Value::Number(value) => {
                let number = CanonicalNumber::from_lexical(&value.to_string())?;
                Ok(Self::Number(number))
            }
            serde_json::Value::String(value) => Ok(Self::String(value.clone())),
            serde_json::Value::Array(values) => values
                .iter()
                .map(|value| Self::from_json(value, budget))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
            serde_json::Value::Object(fields) => {
                let mut canonical = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    canonical.push((name.clone(), Self::from_json(value, budget)?));
                }
                canonical.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
                Ok(Self::Object(canonical))
            }
        }
    }

    fn from_raw(value: &RawValue, budget: &mut KeyBudget) -> Result<Self, RawKeyError> {
        budget.add_node().map_err(RawKeyError::Unique)?;
        let source = value.get().trim();
        let Some(first) = source.as_bytes().first().copied() else {
            return Err(RawKeyError::InvalidToken);
        };
        match first {
            b'n' => Ok(Self::Null),
            b't' => Ok(Self::Bool(true)),
            b'f' => Ok(Self::Bool(false)),
            b'"' => serde_json::from_str::<String>(source)
                .map(Self::String)
                .map_err(RawKeyError::Json),
            b'[' => serde_json::from_str::<Vec<Box<RawValue>>>(source)
                .map_err(RawKeyError::Json)?
                .iter()
                .map(|item| Self::from_raw(item, budget))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
            b'{' => {
                let fields = serde_json::from_str::<
                    std::collections::BTreeMap<String, Box<RawValue>>,
                >(source)
                .map_err(RawKeyError::Json)?;
                let mut canonical = fields
                    .iter()
                    .map(|(name, value)| {
                        Self::from_raw(value, budget).map(|value| (name.clone(), value))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                canonical.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
                Ok(Self::Object(canonical))
            }
            b'-' | b'0'..=b'9' => CanonicalNumber::from_lexical(source)
                .map(Self::Number)
                .map_err(RawKeyError::Unique),
            _ => Err(RawKeyError::InvalidToken),
        }
    }

    fn canonical_json_len(&self) -> Result<usize, UniqueItemsValidationError> {
        match self {
            Self::Null => Ok(4),
            Self::Bool(true) => Ok(4),
            Self::Bool(false) => Ok(5),
            Self::Number(value) => value.canonical_json_len(),
            Self::String(value) => json_string_len(value),
            Self::Array(values) => collection_len(2, values.iter().map(Self::canonical_json_len)),
            Self::Object(fields) => {
                let mut length = 2usize;
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        length = checked_len_add(length, 1)?;
                    }
                    length = checked_len_add(length, json_string_len(name)?)?;
                    length = checked_len_add(length, 1)?;
                    length = checked_len_add(length, value.canonical_json_len()?)?;
                }
                Ok(length)
            }
        }
    }
}

enum RawKeyError {
    Json(serde_json::Error),
    Unique(UniqueItemsValidationError),
    InvalidToken,
}

fn map_raw_key_error(schema: &SchemaNode, error: RawKeyError) -> JsonFormatError {
    match error {
        RawKeyError::Json(error) => JsonFormatError::Json(error),
        RawKeyError::Unique(error) => map_unique_error(schema, error),
        RawKeyError::InvalidToken => JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "valid JSON value",
            got: "invalid raw JSON",
        },
    }
}

fn map_unique_error(schema: &SchemaNode, error: UniqueItemsValidationError) -> JsonFormatError {
    match error {
        UniqueItemsValidationError::Duplicate {
            first_index,
            duplicate_index,
        } => JsonFormatError::UniqueItemsMismatch {
            name: schema.name.clone(),
            first_index,
            duplicate_index,
        },
        UniqueItemsValidationError::Limit { resource, max } => JsonFormatError::UniqueItemsLimit {
            name: schema.name.clone(),
            resource,
            max,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CanonicalNumber {
    negative: bool,
    coefficient: String,
    exponent: i64,
}

impl CanonicalNumber {
    fn from_lexical(lexical: &str) -> Result<Self, UniqueItemsValidationError> {
        let (mantissa, exponent) = lexical
            .split_once(['e', 'E'])
            .map_or((lexical, None), |(mantissa, exponent)| {
                (mantissa, Some(exponent))
            });
        let (negative, mantissa) = mantissa
            .strip_prefix('-')
            .map_or((false, mantissa), |mantissa| (true, mantissa));
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let mut coefficient = String::with_capacity(whole.len() + fraction.len());
        coefficient.push_str(whole);
        coefficient.push_str(fraction);
        let first_nonzero = coefficient
            .bytes()
            .position(|byte| byte != b'0')
            .unwrap_or(coefficient.len());
        coefficient.drain(..first_nonzero);
        if coefficient.is_empty() {
            return Ok(Self {
                negative: false,
                coefficient: "0".into(),
                exponent: 0,
            });
        }
        let exponent = exponent.map_or(Ok(0), |exponent| {
            exponent
                .parse::<i64>()
                .map_err(|_| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))
        })?;
        let trailing_zeroes = coefficient
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'0')
            .count();
        coefficient.truncate(coefficient.len() - trailing_zeroes);
        let fraction_len = i64::try_from(fraction.len())
            .map_err(|_| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))?;
        let trailing_zeroes = i64::try_from(trailing_zeroes)
            .map_err(|_| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))?;
        let exponent = exponent
            .checked_sub(fraction_len)
            .and_then(|value| value.checked_add(trailing_zeroes))
            .ok_or_else(|| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))?;
        Ok(Self {
            negative,
            coefficient,
            exponent,
        })
    }

    fn canonical_json_len(&self) -> Result<usize, UniqueItemsValidationError> {
        let mut length = self.coefficient.len();
        if self.negative {
            length = checked_len_add(length, 1)?;
        }
        if self.exponent != 0 {
            length = checked_len_add(length, 1)?;
            length = checked_len_add(length, decimal_i64_len(self.exponent))?;
        }
        Ok(length)
    }
}

#[derive(Debug, Default)]
struct KeyBudget {
    nodes: usize,
    bytes: usize,
}

impl KeyBudget {
    fn add_node(&mut self) -> Result<(), UniqueItemsValidationError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| limit("canonical nodes", MAX_JSON_UNIQUE_KEY_NODES))?;
        if self.nodes > MAX_JSON_UNIQUE_KEY_NODES {
            return Err(limit("canonical nodes", MAX_JSON_UNIQUE_KEY_NODES));
        }
        Ok(())
    }

    fn add_bytes(&mut self, bytes: usize) -> Result<(), UniqueItemsValidationError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))?;
        if self.bytes > MAX_JSON_UNIQUE_KEY_BYTES {
            return Err(limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES));
        }
        Ok(())
    }
}

fn limit(resource: &'static str, max: usize) -> UniqueItemsValidationError {
    UniqueItemsValidationError::Limit { resource, max }
}

fn collection_len(
    mut length: usize,
    values: impl IntoIterator<Item = Result<usize, UniqueItemsValidationError>>,
) -> Result<usize, UniqueItemsValidationError> {
    for (index, value) in values.into_iter().enumerate() {
        if index != 0 {
            length = checked_len_add(length, 1)?;
        }
        length = checked_len_add(length, value?)?;
    }
    Ok(length)
}

fn json_string_len(value: &str) -> Result<usize, UniqueItemsValidationError> {
    let mut length = 2usize;
    for character in value.chars() {
        let encoded = match character {
            '"' | '\\' | '\u{0008}' | '\u{000c}' | '\n' | '\r' | '\t' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            other => other.len_utf8(),
        };
        length = checked_len_add(length, encoded)?;
    }
    Ok(length)
}

fn decimal_i64_len(value: i64) -> usize {
    if value == 0 {
        return 1;
    }
    let mut magnitude = value.unsigned_abs();
    let mut length = usize::from(value.is_negative());
    while magnitude != 0 {
        length += 1;
        magnitude /= 10;
    }
    length
}

fn checked_len_add(left: usize, right: usize) -> Result<usize, UniqueItemsValidationError> {
    left.checked_add(right)
        .ok_or_else(|| limit("canonical bytes", MAX_JSON_UNIQUE_KEY_BYTES))
}
