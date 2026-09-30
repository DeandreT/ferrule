//! Certified, bounded source metadata for the supported native IDoc grammar.
//!
//! This is separate from [`crate::IdocLayout`]: the latter is the executable
//! fixed-record contract, while this descriptor retains configuration details
//! that the runtime schema and layout cannot reconstruct.

use std::collections::HashSet;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::{
    IdocFieldLayout, IdocLayout, IdocSegmentLayout, MAX_IDOC_FIELDS, MAX_IDOC_RECORD_BYTES,
    MAX_IDOC_SEGMENTS,
};
use ir::{ScalarType, SchemaNode};

pub const MAX_IDOC_NATIVE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IDOC_NATIVE_DEPTH: usize = 128;
pub const MAX_IDOC_NATIVE_NODES: usize = 4_096;
pub const MAX_IDOC_NATIVE_CODES: usize = 131_072;
pub const MAX_IDOC_NATIVE_TEXT_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdocNativeConfigError {
    Invalid(String),
    Limit(&'static str),
}

impl std::fmt::Display for IdocNativeConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => {
                write!(formatter, "invalid native IDoc descriptor: {message}")
            }
            Self::Limit(limit) => write!(
                formatter,
                "native IDoc descriptor exceeds the {limit} limit"
            ),
        }
    }
}

impl std::error::Error for IdocNativeConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdocNativeStatus {
    Mandatory,
    Optional,
}

impl IdocNativeStatus {
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Mandatory => "MANDATORY",
            Self::Optional => "OPTIONAL",
        }
    }

    pub const fn is_optional(self) -> bool {
        matches!(self, Self::Optional)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdocNativeFieldType {
    Character,
}

impl IdocNativeFieldType {
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Character => "CHARACTER",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeCode {
    value: String,
    text: String,
}

impl IdocNativeCode {
    pub fn new(
        value: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<Self, IdocNativeConfigError> {
        let value = value.into();
        let text = text.into();
        validate_text(&value, "code value")?;
        validate_text(&text, "code text")?;
        if value.contains('\'') {
            return Err(IdocNativeConfigError::Invalid(
                "code values containing apostrophes are outside the supported grammar".into(),
            ));
        }
        Ok(Self { value, text })
    }

    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeField {
    name: String,
    text: String,
    field_type: IdocNativeFieldType,
    length: NonZeroU32,
    position: NonZeroU32,
    first_byte: NonZeroU32,
    last_byte: NonZeroU32,
    codes: Vec<IdocNativeCode>,
}

impl IdocNativeField {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        text: impl Into<String>,
        field_type: IdocNativeFieldType,
        length: NonZeroU32,
        position: NonZeroU32,
        first_byte: NonZeroU32,
        last_byte: NonZeroU32,
        codes: Vec<IdocNativeCode>,
    ) -> Result<Self, IdocNativeConfigError> {
        let name = name.into();
        let text = text.into();
        validate_token(&name, "field name")?;
        validate_text(&text, "field text")?;
        if last_byte < first_byte || last_byte.get() > MAX_IDOC_RECORD_BYTES {
            return Err(IdocNativeConfigError::Invalid(format!(
                "field `{name}` has an invalid byte range"
            )));
        }
        if length.get() != last_byte.get() - first_byte.get() + 1 {
            return Err(IdocNativeConfigError::Invalid(format!(
                "field `{name}` LENGTH differs from its byte range"
            )));
        }
        if codes.len() > MAX_IDOC_NATIVE_CODES {
            return Err(IdocNativeConfigError::Limit("code count"));
        }
        let mut values = HashSet::new();
        for code in &codes {
            if !values.insert(code.value()) {
                return Err(IdocNativeConfigError::Invalid(format!(
                    "field `{name}` has duplicate code `{}`",
                    code.value()
                )));
            }
        }
        Ok(Self {
            name,
            text,
            field_type,
            length,
            position,
            first_byte,
            last_byte,
            codes,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub const fn field_type(&self) -> IdocNativeFieldType {
        self.field_type
    }
    pub const fn length(&self) -> NonZeroU32 {
        self.length
    }
    pub const fn position(&self) -> NonZeroU32 {
        self.position
    }
    pub const fn first_byte(&self) -> NonZeroU32 {
        self.first_byte
    }
    pub const fn last_byte(&self) -> NonZeroU32 {
        self.last_byte
    }
    pub fn codes(&self) -> &[IdocNativeCode] {
        &self.codes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeGroup {
    number: NonZeroU32,
    level: NonZeroU32,
    status: IdocNativeStatus,
    loop_min: u64,
    loop_max: u64,
    children: Vec<IdocNativeNode>,
}

impl IdocNativeGroup {
    pub fn new(
        number: NonZeroU32,
        level: NonZeroU32,
        status: IdocNativeStatus,
        loop_min: u64,
        loop_max: u64,
        children: Vec<IdocNativeNode>,
    ) -> Result<Self, IdocNativeConfigError> {
        validate_occurrence(loop_min, loop_max)?;
        validate_level(level)?;
        if children.is_empty() {
            return Err(IdocNativeConfigError::Invalid(format!(
                "group `{number}` has no children"
            )));
        }
        Ok(Self {
            number,
            level,
            status,
            loop_min,
            loop_max,
            children,
        })
    }

    pub const fn number(&self) -> NonZeroU32 {
        self.number
    }
    pub const fn level(&self) -> NonZeroU32 {
        self.level
    }
    pub const fn status(&self) -> IdocNativeStatus {
        self.status
    }
    pub const fn loop_min(&self) -> u64 {
        self.loop_min
    }
    pub const fn loop_max(&self) -> u64 {
        self.loop_max
    }
    pub fn children(&self) -> &[IdocNativeNode] {
        &self.children
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeSegment {
    record_name: String,
    segment_type: String,
    qualified: bool,
    level: NonZeroU32,
    status: IdocNativeStatus,
    loop_min: u64,
    loop_max: u64,
    fields: Vec<IdocNativeField>,
}

impl IdocNativeSegment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        record_name: impl Into<String>,
        segment_type: impl Into<String>,
        qualified: bool,
        level: NonZeroU32,
        status: IdocNativeStatus,
        loop_min: u64,
        loop_max: u64,
        fields: Vec<IdocNativeField>,
    ) -> Result<Self, IdocNativeConfigError> {
        let record_name = record_name.into();
        let segment_type = segment_type.into();
        validate_token(&record_name, "record name")?;
        validate_token(&segment_type, "segment type")?;
        validate_occurrence(loop_min, loop_max)?;
        validate_level(level)?;
        if fields.is_empty() {
            return Err(IdocNativeConfigError::Invalid(format!(
                "segment `{record_name}` has no fields"
            )));
        }
        if fields.len() > MAX_IDOC_FIELDS {
            return Err(IdocNativeConfigError::Limit("field count"));
        }
        let mut names = HashSet::new();
        let mut positions = HashSet::new();
        let mut last_byte = 0;
        for field in &fields {
            if !names.insert(field.name()) || !positions.insert(field.position().get()) {
                return Err(IdocNativeConfigError::Invalid(format!(
                    "segment `{record_name}` has duplicate field name or position"
                )));
            }
            if field.first_byte().get() <= last_byte {
                return Err(IdocNativeConfigError::Invalid(format!(
                    "segment `{record_name}` has overlapping or unordered field ranges"
                )));
            }
            last_byte = field.last_byte().get();
        }
        Ok(Self {
            record_name,
            segment_type,
            qualified,
            level,
            status,
            loop_min,
            loop_max,
            fields,
        })
    }

    pub fn record_name(&self) -> &str {
        &self.record_name
    }
    pub fn segment_type(&self) -> &str {
        &self.segment_type
    }
    pub const fn qualified(&self) -> bool {
        self.qualified
    }
    pub const fn level(&self) -> NonZeroU32 {
        self.level
    }
    pub const fn status(&self) -> IdocNativeStatus {
        self.status
    }
    pub const fn loop_min(&self) -> u64 {
        self.loop_min
    }
    pub const fn loop_max(&self) -> u64 {
        self.loop_max
    }
    pub fn fields(&self) -> &[IdocNativeField] {
        &self.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum IdocNativeNode {
    Group(IdocNativeGroup),
    Segment(IdocNativeSegment),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeConfig {
    name: String,
    nodes: Vec<IdocNativeNode>,
}

impl IdocNativeConfig {
    pub fn new(
        name: impl Into<String>,
        nodes: Vec<IdocNativeNode>,
    ) -> Result<Self, IdocNativeConfigError> {
        let name = name.into();
        validate_token(&name, "IDoc name")?;
        if nodes.is_empty() {
            return Err(IdocNativeConfigError::Invalid("IDoc has no nodes".into()));
        }
        let mut budget = Budget::default();
        budget.add_bytes(name.len())?;
        let mut segment_names = HashSet::new();
        let mut group_numbers = HashSet::new();
        walk_nodes(
            &nodes,
            0,
            &mut budget,
            &mut segment_names,
            &mut group_numbers,
        )?;
        if budget.segments == 0 {
            return Err(IdocNativeConfigError::Invalid(
                "IDoc has no segments".into(),
            ));
        }
        // The executable layout enforces the same aggregate segment/field bounds.
        let descriptor = Self { name, nodes };
        descriptor.project()?;
        Ok(descriptor)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn nodes(&self) -> &[IdocNativeNode] {
        &self.nodes
    }

    /// Projects the exact runtime shape represented by this descriptor.
    pub fn project(&self) -> Result<(SchemaNode, IdocLayout), IdocNativeConfigError> {
        let mut layouts = Vec::new();
        let children = self
            .nodes
            .iter()
            .map(|node| project_node(node, &mut layouts))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = IdocLayout::new(layouts)
            .map_err(|error| IdocNativeConfigError::Invalid(error.to_string()))?;
        Ok((SchemaNode::group("IDOC", children), layout))
    }
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    segments: usize,
    fields: usize,
    codes: usize,
    bytes: usize,
}

impl Budget {
    fn add_bytes(&mut self, bytes: usize) -> Result<(), IdocNativeConfigError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(IdocNativeConfigError::Limit("metadata byte count"))?;
        if self.bytes > MAX_IDOC_NATIVE_BYTES {
            return Err(IdocNativeConfigError::Limit("metadata byte count"));
        }
        Ok(())
    }
}

fn walk_nodes(
    nodes: &[IdocNativeNode],
    depth: usize,
    budget: &mut Budget,
    segment_names: &mut HashSet<String>,
    group_numbers: &mut HashSet<u32>,
) -> Result<(), IdocNativeConfigError> {
    if depth > MAX_IDOC_NATIVE_DEPTH {
        return Err(IdocNativeConfigError::Limit("nesting depth"));
    }
    for node in nodes {
        budget.nodes += 1;
        if budget.nodes > MAX_IDOC_NATIVE_NODES {
            return Err(IdocNativeConfigError::Limit("node count"));
        }
        match node {
            IdocNativeNode::Group(group) => {
                if !group_numbers.insert(group.number.get()) {
                    return Err(IdocNativeConfigError::Invalid(format!(
                        "duplicate group number `{}`",
                        group.number
                    )));
                }
                budget.add_bytes(16)?;
                walk_nodes(
                    &group.children,
                    depth + 1,
                    budget,
                    segment_names,
                    group_numbers,
                )?;
            }
            IdocNativeNode::Segment(segment) => {
                budget.segments += 1;
                if budget.segments > MAX_IDOC_SEGMENTS {
                    return Err(IdocNativeConfigError::Limit("segment count"));
                }
                if !segment_names.insert(segment.record_name.clone()) {
                    return Err(IdocNativeConfigError::Invalid(format!(
                        "duplicate segment `{}`",
                        segment.record_name
                    )));
                }
                budget.add_bytes(segment.record_name.len() + segment.segment_type.len())?;
                for field in &segment.fields {
                    budget.fields += 1;
                    if budget.fields > MAX_IDOC_FIELDS {
                        return Err(IdocNativeConfigError::Limit("field count"));
                    }
                    budget.add_bytes(field.name.len() + field.text.len())?;
                    for code in &field.codes {
                        budget.codes += 1;
                        if budget.codes > MAX_IDOC_NATIVE_CODES {
                            return Err(IdocNativeConfigError::Limit("code count"));
                        }
                        budget.add_bytes(code.value.len() + code.text.len())?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn project_node(
    node: &IdocNativeNode,
    layouts: &mut Vec<IdocSegmentLayout>,
) -> Result<SchemaNode, IdocNativeConfigError> {
    match node {
        IdocNativeNode::Group(group) => {
            let children = group
                .children
                .iter()
                .map(|node| project_node(node, layouts))
                .collect::<Result<Vec<_>, _>>()?;
            let mut schema = SchemaNode::group(format!("SG{}", group.number), children);
            schema.repeating = group.status.is_optional() || group.loop_max > 1;
            Ok(schema)
        }
        IdocNativeNode::Segment(segment) => {
            let fields = segment
                .fields
                .iter()
                .map(|field| SchemaNode::scalar(field.name(), ScalarType::String))
                .collect();
            let layout_fields = segment
                .fields
                .iter()
                .map(|field| {
                    IdocFieldLayout::new(field.name(), field.first_byte, field.last_byte)
                        .map_err(|error| IdocNativeConfigError::Invalid(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            layouts.push(
                IdocSegmentLayout::new(&segment.record_name, layout_fields)
                    .map_err(|error| IdocNativeConfigError::Invalid(error.to_string()))?,
            );
            let mut schema = SchemaNode::group(&segment.record_name, fields);
            schema.repeating = segment.status.is_optional() || segment.loop_max > 1;
            Ok(schema)
        }
    }
}

fn validate_token(value: &str, what: &str) -> Result<(), IdocNativeConfigError> {
    if value.is_empty()
        || value.len() > MAX_IDOC_NATIVE_TEXT_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(IdocNativeConfigError::Invalid(format!(
            "{what} is not a bounded ASCII token"
        )));
    }
    Ok(())
}

fn validate_text(value: &str, what: &str) -> Result<(), IdocNativeConfigError> {
    if value.len() > MAX_IDOC_NATIVE_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(IdocNativeConfigError::Invalid(format!(
            "{what} is not bounded single-line text"
        )));
    }
    Ok(())
}

fn validate_occurrence(min: u64, max: u64) -> Result<(), IdocNativeConfigError> {
    if max == 0 || min > max {
        return Err(IdocNativeConfigError::Invalid(
            "invalid LOOPMIN/LOOPMAX range".into(),
        ));
    }
    Ok(())
}

fn validate_level(level: NonZeroU32) -> Result<(), IdocNativeConfigError> {
    if level.get() as usize > MAX_IDOC_NATIVE_DEPTH {
        return Err(IdocNativeConfigError::Limit("LEVEL"));
    }
    Ok(())
}

// Only the top-level descriptor is deserializable. Wire values are rebuilt
// through the validated constructors before any instance becomes usable.
impl<'de> Deserialize<'de> for IdocNativeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireConfig {
            name: String,
            nodes: Vec<WireNode>,
        }
        #[derive(Deserialize)]
        enum WireNode {
            Group(WireGroup),
            Segment(WireSegment),
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireGroup {
            number: NonZeroU32,
            level: NonZeroU32,
            status: IdocNativeStatus,
            loop_min: u64,
            loop_max: u64,
            children: Vec<WireNode>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSegment {
            record_name: String,
            segment_type: String,
            qualified: bool,
            level: NonZeroU32,
            status: IdocNativeStatus,
            loop_min: u64,
            loop_max: u64,
            fields: Vec<WireField>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireField {
            name: String,
            text: String,
            field_type: IdocNativeFieldType,
            length: NonZeroU32,
            position: NonZeroU32,
            first_byte: NonZeroU32,
            last_byte: NonZeroU32,
            codes: Vec<WireCode>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireCode {
            value: String,
            text: String,
        }

        fn convert(node: WireNode, depth: usize) -> Result<IdocNativeNode, IdocNativeConfigError> {
            if depth > MAX_IDOC_NATIVE_DEPTH {
                return Err(IdocNativeConfigError::Limit("nesting depth"));
            }
            match node {
                WireNode::Group(group) => {
                    let children = group
                        .children
                        .into_iter()
                        .map(|child| convert(child, depth + 1))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(IdocNativeNode::Group(IdocNativeGroup::new(
                        group.number,
                        group.level,
                        group.status,
                        group.loop_min,
                        group.loop_max,
                        children,
                    )?))
                }
                WireNode::Segment(segment) => {
                    let fields = segment
                        .fields
                        .into_iter()
                        .map(|field| {
                            let codes = field
                                .codes
                                .into_iter()
                                .map(|code| IdocNativeCode::new(code.value, code.text))
                                .collect::<Result<Vec<_>, _>>()?;
                            IdocNativeField::new(
                                field.name,
                                field.text,
                                field.field_type,
                                field.length,
                                field.position,
                                field.first_byte,
                                field.last_byte,
                                codes,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(IdocNativeNode::Segment(IdocNativeSegment::new(
                        segment.record_name,
                        segment.segment_type,
                        segment.qualified,
                        segment.level,
                        segment.status,
                        segment.loop_min,
                        segment.loop_max,
                        fields,
                    )?))
                }
            }
        }

        let wire = WireConfig::deserialize(deserializer)?;
        let nodes = wire
            .nodes
            .into_iter()
            .map(|node| convert(node, 0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        Self::new(wire.name, nodes).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_segment() -> IdocNativeConfig {
        let field = IdocNativeField::new(
            "CODE",
            "Código",
            IdocNativeFieldType::Character,
            NonZeroU32::new(3).unwrap(),
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(64).unwrap(),
            NonZeroU32::new(66).unwrap(),
            vec![IdocNativeCode::new("é", "Été").unwrap()],
        )
        .unwrap();
        IdocNativeConfig::new(
            "TEST01",
            vec![IdocNativeNode::Segment(
                IdocNativeSegment::new(
                    "E2ITEM",
                    "E1ITEM",
                    true,
                    NonZeroU32::new(1).unwrap(),
                    IdocNativeStatus::Optional,
                    0,
                    9_999_999_999,
                    vec![field],
                )
                .unwrap(),
            )],
        )
        .unwrap()
    }

    #[test]
    fn descriptor_json_roundtrip_revalidates_private_metadata() {
        let valid = one_segment();
        let json = serde_json::to_value(&valid).unwrap();
        assert_eq!(
            serde_json::from_value::<IdocNativeConfig>(json.clone()).unwrap(),
            valid
        );

        let mut invalid_length = json.clone();
        invalid_length["nodes"][0]["Segment"]["fields"][0]["length"] = json!(4);
        assert!(serde_json::from_value::<IdocNativeConfig>(invalid_length).is_err());

        let mut invalid_loop = json.clone();
        invalid_loop["nodes"][0]["Segment"]["loop_max"] = json!(0);
        assert!(serde_json::from_value::<IdocNativeConfig>(invalid_loop).is_err());

        let mut duplicate_code = json.clone();
        let code = duplicate_code["nodes"][0]["Segment"]["fields"][0]["codes"][0].clone();
        duplicate_code["nodes"][0]["Segment"]["fields"][0]["codes"] = json!([code.clone(), code]);
        assert!(serde_json::from_value::<IdocNativeConfig>(duplicate_code).is_err());

        let mut unknown = json;
        unknown["nodes"][0]["Segment"]["untracked"] = json!(true);
        assert!(serde_json::from_value::<IdocNativeConfig>(unknown).is_err());
    }

    #[test]
    fn constructors_enforce_text_record_and_depth_budgets() {
        assert!(IdocNativeCode::new("X", "x".repeat(MAX_IDOC_NATIVE_TEXT_BYTES + 1)).is_err());
        assert!(IdocNativeCode::new("'", "quoted").is_err());
        let too_high = IdocNativeSegment::new(
            "E2ITEM",
            "E1ITEM",
            false,
            NonZeroU32::new(MAX_IDOC_NATIVE_DEPTH as u32 + 1).unwrap(),
            IdocNativeStatus::Mandatory,
            1,
            1,
            match one_segment().nodes.pop().unwrap() {
                IdocNativeNode::Segment(segment) => segment.fields,
                _ => unreachable!(),
            },
        );
        assert!(matches!(
            too_high,
            Err(IdocNativeConfigError::Limit("LEVEL"))
        ));
        let child = one_segment().nodes[0].clone();
        let duplicate_groups = (0..2)
            .map(|_| {
                IdocNativeNode::Group(
                    IdocNativeGroup::new(
                        NonZeroU32::new(1).unwrap(),
                        NonZeroU32::new(1).unwrap(),
                        IdocNativeStatus::Mandatory,
                        1,
                        1,
                        vec![child.clone()],
                    )
                    .unwrap(),
                )
            })
            .collect();
        assert!(IdocNativeConfig::new("TEST01", duplicate_groups).is_err());
        let mut nodes = one_segment().nodes.clone();
        for number in 1..=MAX_IDOC_NATIVE_DEPTH + 1 {
            nodes = vec![IdocNativeNode::Group(
                IdocNativeGroup::new(
                    NonZeroU32::new(number as u32).unwrap(),
                    NonZeroU32::new(1).unwrap(),
                    IdocNativeStatus::Mandatory,
                    1,
                    1,
                    nodes,
                )
                .unwrap(),
            )];
        }
        assert!(matches!(
            IdocNativeConfig::new("TEST01", nodes),
            Err(IdocNativeConfigError::Limit("nesting depth"))
        ));
    }
}
