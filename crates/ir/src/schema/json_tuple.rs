//! The exact frame invariant of a JSON Schema tuple array.
//!
//! A tuple frame is a group whose [`SchemaNode::json_tuple`] layout gives
//! each array position its own schema: children `item1` through `itemN` hold
//! the positional members in schema order and, when the tail is open, one
//! more repeating `rest` child holds the tail item schema. No member and no
//! tail item is itself a JSON array. The layout owns the complete array
//! length; a repeating frame (an array of tuples) keeps its outer array
//! bounds in `item_count_range`.
//!
//! Only canonical frames are representable: a closed frame's maximum length
//! is exactly its member count, an open frame admits more items than it has
//! members, and a frame that would lower to an ordinary homogeneous array is
//! refused. Saved-schema readers, engine validation and the generated
//! descriptor codec all consult this one check.

use serde::{Deserialize, Serialize};

use crate::{
    GroupAlternativeMode, ItemCountRange, SchemaKind, SchemaNode, XmlAlternativeKind,
    XmlWildcardProcessContents,
};

/// The declared tail role of an open tuple frame.
pub const JSON_TUPLE_REST_FIELD: &str = "rest";
/// The name prefix of a tuple member; member `k` is named `item{k}`.
pub const JSON_TUPLE_MEMBER_PREFIX: &str = "item";
/// Maximum number of positional members in one tuple frame.
pub const MAX_JSON_TUPLE_MEMBERS: usize = 4096;

/// The array layout of one tuple frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonTupleLayout {
    /// Bounds on the complete array length: positions plus tail items.
    /// Absence is the unconstrained length, as for
    /// [`SchemaNode::item_count_range`]; a closed frame always has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<ItemCountRange>,
    pub tail: JsonTupleTail,
}

/// Whether array items beyond the positional members are admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonTupleTail {
    /// No item follows the members.
    Closed,
    /// Further items follow the members under the `rest` item schema.
    Open,
}

/// The name of the one-based positional member `position`.
pub fn json_tuple_member_name(position: usize) -> String {
    format!("{JSON_TUPLE_MEMBER_PREFIX}{position}")
}

/// The roles of one valid tuple frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JsonTupleFrame<'a> {
    pub layout: JsonTupleLayout,
    /// Positional members in schema order; `members[k - 1]` is `item{k}`.
    pub members: &'a [SchemaNode],
    /// The repeating tail item schema of an open frame.
    pub rest: Option<&'a SchemaNode>,
}

/// The first way a node departs from the exact tuple frame or places a
/// frame where it cannot be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonTupleFrameError {
    /// The frame is a scalar rather than a group.
    NotGroup,
    /// The frame is also a nested-array wrapper.
    ArrayWrapper,
    /// The frame carries a recursive schema reference.
    RecursiveReference,
    /// The frame carries object metadata: required, alternative or dynamic
    /// properties, or property constraints.
    ObjectMetadata,
    /// The frame carries `contains` or `uniqueItems` assertions.
    ArrayMetadata,
    /// The frame carries scalar value metadata: null, arbitrary JSON, fixed,
    /// allowed, default or generated values, or value constraints.
    ScalarMetadata,
    /// The frame carries XML naming, occurrence, nil, type or compositor
    /// metadata.
    XmlMetadata,
    /// The frame carries a database relation.
    DatabaseRelation,
    /// A repeating frame does not state whether each tuple may be `null`.
    ItemNullability,
    /// An open frame's last child is not its `rest` tail.
    MissingRest,
    /// The frame has no members or more than [`MAX_JSON_TUPLE_MEMBERS`].
    MemberCount { count: usize },
    /// The member at this one-based position is not named `item{position}`.
    MemberName { position: usize },
    /// The member at this one-based position is itself a JSON array.
    MemberArray { position: usize },
    /// The `rest` tail does not repeat.
    RestNotRepeating,
    /// The `rest` tail items are themselves JSON arrays.
    RestArray,
    /// The `rest` tail carries its own count, `contains`, `uniqueItems` or
    /// container nullability; the frame owns the array.
    RestArrayMetadata,
    /// The length bounds are not canonical for the tail: a closed frame's
    /// maximum is not its member count, or an open frame's maximum admits no
    /// tail item.
    Length,
    /// The members and tail describe an ordinary homogeneous array.
    Homogeneous,
    /// A dynamic property value schema is a frame.
    DynamicValue,
    /// A `contains` or dependent-schema predicate contains a frame.
    PredicateSchema,
}

impl core::fmt::Display for JsonTupleFrameError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotGroup => formatter.write_str("the frame is not a group"),
            Self::ArrayWrapper => formatter.write_str("the frame is also a nested-array wrapper"),
            Self::RecursiveReference => {
                formatter.write_str("the frame declares a recursive reference")
            }
            Self::ObjectMetadata => formatter.write_str("the frame declares object metadata"),
            Self::ArrayMetadata => {
                formatter.write_str("the frame declares `contains` or `uniqueItems` assertions")
            }
            Self::ScalarMetadata => formatter.write_str("the frame declares scalar value metadata"),
            Self::XmlMetadata => formatter.write_str("the frame declares XML metadata"),
            Self::DatabaseRelation => formatter.write_str("the frame declares a database relation"),
            Self::ItemNullability => formatter
                .write_str("the repeating frame does not declare whether each tuple may be null"),
            Self::MissingRest => write!(
                formatter,
                "the open frame's last child is not `{JSON_TUPLE_REST_FIELD}`"
            ),
            Self::MemberCount { count } => write!(
                formatter,
                "the frame has {count} members; it requires 1 to {MAX_JSON_TUPLE_MEMBERS}"
            ),
            Self::MemberName { position } => write!(
                formatter,
                "member {position} is not named `{}`",
                json_tuple_member_name(*position)
            ),
            Self::MemberArray { position } => {
                write!(formatter, "member {position} is a JSON array")
            }
            Self::RestNotRepeating => write!(
                formatter,
                "the `{JSON_TUPLE_REST_FIELD}` tail does not repeat"
            ),
            Self::RestArray => write!(
                formatter,
                "the `{JSON_TUPLE_REST_FIELD}` tail items are JSON arrays"
            ),
            Self::RestArrayMetadata => write!(
                formatter,
                "the `{JSON_TUPLE_REST_FIELD}` tail declares its own array metadata"
            ),
            Self::Length => {
                formatter.write_str("the length bounds are not canonical for the frame's tail")
            }
            Self::Homogeneous => formatter.write_str("the frame describes a homogeneous array"),
            Self::DynamicValue => {
                formatter.write_str("a dynamic property value schema is a tuple frame")
            }
            Self::PredicateSchema => {
                formatter.write_str("a predicate schema contains a tuple frame")
            }
        }
    }
}

impl std::error::Error for JsonTupleFrameError {}

impl SchemaNode {
    /// Checks this node's tuple frame, and that no dynamic property value or
    /// predicate schema it owns is a frame, without copying its child tree.
    ///
    /// A node without frames passes.
    pub fn check_json_tuple_frame(&self) -> Result<(), JsonTupleFrameError> {
        if let Some(layout) = self.json_tuple {
            self.check_frame(layout)?;
        }
        if self
            .dynamic_fields()
            .is_some_and(|dynamic| dynamic.json_tuple.is_some())
        {
            return Err(JsonTupleFrameError::DynamicValue);
        }
        let predicates = self
            .json_contains
            .iter()
            .flat_map(|constraints| constraints.as_slice())
            .filter_map(|constraint| constraint.predicate().as_schema())
            .chain(
                self.json_dependent_schemas
                    .iter()
                    .flat_map(|constraints| constraints.as_slice())
                    .filter_map(|constraint| constraint.predicate().as_schema()),
            );
        for predicate in predicates {
            if predicate.contains_json_tuple() {
                return Err(JsonTupleFrameError::PredicateSchema);
            }
        }
        Ok(())
    }

    pub fn json_tuple_frame_is_valid(&self) -> bool {
        self.check_json_tuple_frame().is_ok()
    }

    pub fn json_tuple_frames_tree_is_valid(&self) -> bool {
        self.walk_json_schema_nodes(SchemaNode::json_tuple_frame_is_valid)
    }

    /// Whether any node in this schema, including private predicate schemas,
    /// is a tuple frame. Non-JSON boundaries refuse such schemas.
    pub fn contains_json_tuple(&self) -> bool {
        !self.walk_json_schema_nodes(|node| node.json_tuple.is_none())
    }

    /// The roles of a validated tuple frame.
    pub fn json_tuple_frame(&self) -> Option<JsonTupleFrame<'_>> {
        let layout = self.json_tuple?;
        self.check_frame(layout).ok()?;
        let SchemaKind::Group { children, .. } = &self.kind else {
            return None;
        };
        Some(match layout.tail {
            JsonTupleTail::Closed => JsonTupleFrame {
                layout,
                members: children,
                rest: None,
            },
            JsonTupleTail::Open => {
                let (rest, members) = children.split_last()?;
                JsonTupleFrame {
                    layout,
                    members,
                    rest: Some(rest),
                }
            }
        })
    }

    /// Makes this group a tuple frame when its children and metadata form
    /// exactly that frame.
    pub fn with_json_tuple(mut self, layout: JsonTupleLayout) -> Option<Self> {
        self.json_tuple = Some(layout);
        (self.json_tuple_frame_is_valid() && self.metadata_is_valid()).then_some(self)
    }

    fn check_frame(&self, layout: JsonTupleLayout) -> Result<(), JsonTupleFrameError> {
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            xml_restricted_alternatives,
            dynamic,
        } = &self.kind
        else {
            return Err(JsonTupleFrameError::NotGroup);
        };
        if self.json_array_wrapper {
            return Err(JsonTupleFrameError::ArrayWrapper);
        }
        if self.recursive_ref.is_some() {
            return Err(JsonTupleFrameError::RecursiveReference);
        }
        if !alternatives.is_empty()
            || !required.is_empty()
            || dynamic.is_some()
            || self.alternative_mode != GroupAlternativeMode::Exclusive
            || self.json_dependent_schemas.is_some()
            || self.property_count_range.is_some()
            || self.json_property_dependencies.is_some()
            || self.json_pattern_property_names.is_some()
            || self.json_property_names.is_some()
        {
            return Err(JsonTupleFrameError::ObjectMetadata);
        }
        if self.json_contains.is_some() || self.json_unique_items {
            return Err(JsonTupleFrameError::ArrayMetadata);
        }
        if self.nullable
            || self.json_any
            || self.fixed.is_some()
            || self.json_allowed_values.is_some()
            || self.numeric_range.is_some()
            || self.json_multiple_of.is_some()
            || self.string_length_range.is_some()
            || self.json_patterns.is_some()
            || !self.json_formats.is_empty()
            || self.default.is_some()
            || self.value_generation.is_some()
        {
            return Err(JsonTupleFrameError::ScalarMetadata);
        }
        if !xml_restricted_alternatives.is_empty()
            || self.xml_namespace.is_some()
            || !self.xml_name_alternatives.is_empty()
            || self.xml_wildcard_namespace.is_some()
            || self.xml_wildcard_process_contents != XmlWildcardProcessContents::Skip
            || self.xml_wildcard_excluded_names.is_some()
            || self.attribute
            || self.text
            || self.nillable
            || self.xml_optional
            || self.xml_attribute_required
            || self.xml_alternative_kind != XmlAlternativeKind::XsiType
            || self.xml_type_alternatives
            || self.xml_default_type.is_some()
            || !self.xml_repeating_sequences.is_empty()
            || !self.xml_repeating_choices.is_empty()
        {
            return Err(JsonTupleFrameError::XmlMetadata);
        }
        if self.database_relation.is_some() {
            return Err(JsonTupleFrameError::DatabaseRelation);
        }
        // `json_single_container_nullable` would otherwise read the outer
        // array's nullability as each tuple's.
        if self.repeating && self.json_object_item_nullable.is_none() {
            return Err(JsonTupleFrameError::ItemNullability);
        }
        let (members, rest) = match layout.tail {
            JsonTupleTail::Closed => (children.as_slice(), None),
            JsonTupleTail::Open => match children.split_last() {
                Some((rest, members)) if rest.name == JSON_TUPLE_REST_FIELD => {
                    (members, Some(rest))
                }
                _ => return Err(JsonTupleFrameError::MissingRest),
            },
        };
        if members.is_empty() || members.len() > MAX_JSON_TUPLE_MEMBERS {
            return Err(JsonTupleFrameError::MemberCount {
                count: members.len(),
            });
        }
        for (index, member) in members.iter().enumerate() {
            let position = index + 1;
            if member.name != json_tuple_member_name(position) {
                return Err(JsonTupleFrameError::MemberName { position });
            }
            if is_json_array(member) {
                return Err(JsonTupleFrameError::MemberArray { position });
            }
        }
        if let Some(rest) = rest {
            if !rest.repeating {
                return Err(JsonTupleFrameError::RestNotRepeating);
            }
            if rest.json_array_wrapper || rest.json_tuple.is_some() {
                return Err(JsonTupleFrameError::RestArray);
            }
            if rest.item_count_range.is_some()
                || rest.json_contains.is_some()
                || rest.json_unique_items
                || rest.container_nullable
            {
                return Err(JsonTupleFrameError::RestArrayMetadata);
            }
        }
        let count = u64::try_from(members.len()).unwrap_or(u64::MAX);
        let maximum = layout.length.and_then(ItemCountRange::maximum);
        let canonical = match layout.tail {
            JsonTupleTail::Closed => maximum == Some(count),
            JsonTupleTail::Open => maximum.is_none_or(|maximum| maximum > count),
        };
        if !canonical {
            return Err(JsonTupleFrameError::Length);
        }
        if homogeneous(members, rest) {
            return Err(JsonTupleFrameError::Homogeneous);
        }
        Ok(())
    }
}

/// A repeating node, nested-array wrapper or nested frame is a JSON array.
fn is_json_array(node: &SchemaNode) -> bool {
    node.repeating || node.json_array_wrapper || node.json_tuple.is_some()
}

/// Whether selection would lower these members and tail to an ordinary
/// homogeneous array: every member has one JSON shape and the tail is closed
/// or has that same item shape. See [`json_shape`].
fn homogeneous(members: &[SchemaNode], rest: Option<&SchemaNode>) -> bool {
    let Some((first, others)) = members.split_first() else {
        return false;
    };
    let first = json_shape(first);
    if others.iter().any(|member| json_shape(member) != first) {
        return false;
    }
    rest.is_none_or(|rest| {
        // A tail item's object null lives in `json_object_item_nullable`,
        // where a member's lives in `container_nullable`.
        let mut item = rest.clone();
        item.repeating = false;
        if matches!(item.kind, SchemaKind::Group { .. }) {
            item.container_nullable = item.json_object_item_nullable.take().unwrap_or(false);
        }
        json_shape(&item) == first
    })
}

/// The schema as JSON boundaries see it, for comparing positions: the
/// position's own name is dropped, and XML, database and value-generation
/// metadata is cleared throughout its tree because JSON reading, writing and
/// JSON Schema export ignore it. Two positions with equal shapes accept and
/// produce the same JSON values. Predicate schemas are compared as stored.
fn json_shape(node: &SchemaNode) -> SchemaNode {
    let mut shape = node.clone();
    shape.name.clear();
    let mut pending = vec![&mut shape];
    while let Some(node) = pending.pop() {
        node.xml_namespace = None;
        node.xml_name_alternatives.clear();
        node.xml_wildcard_namespace = None;
        node.xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
        node.xml_wildcard_excluded_names = None;
        node.attribute = false;
        node.text = false;
        node.nillable = false;
        node.xml_optional = false;
        node.xml_attribute_required = false;
        node.default = None;
        node.value_generation = None;
        node.xml_alternative_kind = XmlAlternativeKind::XsiType;
        node.xml_default_type = None;
        node.xml_repeating_sequences.clear();
        node.xml_repeating_choices.clear();
        node.database_relation = None;
        if let SchemaKind::Group {
            children,
            xml_restricted_alternatives,
            dynamic,
            ..
        } = &mut node.kind
        {
            xml_restricted_alternatives.clear();
            pending.extend(children.iter_mut());
            if let Some(dynamic) = dynamic {
                pending.push(dynamic);
            }
        }
    }
    shape
}
