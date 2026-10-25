use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Binding, DynamicBinding, DynamicChild, IterationOutput, JoinId, JoinPlan, NodeId,
    OwnedJoinPlan, Scope, ScopeConstruction, ScopeIteration, ScopeSequence, SequenceExpr,
    SequenceWindow, SortFilterOrder, SortKey, SourceSelector, SourceSelectorRef,
    is_constructed_scope, is_false, is_repeated_output,
};

#[derive(Serialize)]
#[serde(untagged)]
enum JoinPlanRef<'a> {
    Legacy(&'a JoinPlan),
    Owned(&'a OwnedJoinPlan),
}

#[derive(Serialize)]
struct JoinRef<'a> {
    id: JoinId,
    plan: JoinPlanRef<'a>,
}

#[derive(Deserialize)]
struct JoinOwned {
    id: JoinId,
    plan: AnyJoinPlan,
}

/// Both plan forms share the `join` scope key, so readers that only know
/// collection arrays refuse owned plans instead of dropping them. Owned plans
/// are recognized by an object first collection and are never mixed with
/// legacy collection arrays.
enum AnyJoinPlan {
    Legacy(JoinPlan),
    Owned(OwnedJoinPlan),
}

impl<'de> Deserialize<'de> for AnyJoinPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let owned = value
            .get("first")
            .and_then(|first| first.get("collection"))
            .is_some_and(serde_json::Value::is_object);
        if owned {
            serde_json::from_value(value)
                .map(Self::Owned)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Self::Legacy)
                .map_err(serde::de::Error::custom)
        }
    }
}

/// Optional presence shares the `sequence` scope key. Readers that only know
/// generated sequences reject its unknown `kind` instead of ignoring an unknown
/// key and constructing exactly one group.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PresenceWire {
    OptionalPresence { condition: NodeId },
}

const OPTIONAL_PRESENCE_KIND: &str = "optional_presence";

#[derive(Serialize)]
#[serde(untagged)]
enum SequenceRef<'a> {
    Generated(&'a SequenceExpr),
    Presence(PresenceWire),
}

enum AnySequence {
    Generated(SequenceExpr),
    Presence(NodeId),
}

impl<'de> Deserialize<'de> for AnySequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("kind").and_then(serde_json::Value::as_str) == Some(OPTIONAL_PRESENCE_KIND) {
            serde_json::from_value(value)
                .map(|PresenceWire::OptionalPresence { condition }| Self::Presence(condition))
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Self::Generated)
                .map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Serialize)]
struct ScopeRef<'a> {
    target_field: &'a str,
    #[serde(skip_serializing_if = "is_constructed_scope")]
    construction: ScopeConstruction,
    source: Option<SourceSelectorRef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<SequenceRef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    join: Option<JoinRef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    segments: Option<&'a ScopeSequence>,
    filter: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_group_filter: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_by: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_adjacent_by: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_starting_with: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_ending_with: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_into_blocks: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_by: Option<NodeId>,
    #[serde(skip_serializing_if = "is_false")]
    sort_descending: bool,
    #[serde(skip_serializing_if = "<[SortKey]>::is_empty")]
    sort_then_by: &'a [SortKey],
    #[serde(skip_serializing_if = "is_default_sort_filter_order")]
    sort_filter_order: SortFilterOrder,
    #[serde(skip_serializing_if = "<[SequenceWindow]>::is_empty")]
    windows: &'a [SequenceWindow],
    #[serde(skip_serializing_if = "Option::is_none")]
    output_path: Option<NodeId>,
    #[serde(skip_serializing_if = "is_repeated_output")]
    iteration_output: IterationOutput,
    bindings: &'a [Binding],
    #[serde(skip_serializing_if = "<[DynamicBinding]>::is_empty")]
    dynamic_bindings: &'a [DynamicBinding],
    children: &'a [Scope],
    #[serde(skip_serializing_if = "<[DynamicChild]>::is_empty")]
    dynamic_children: &'a [DynamicChild],
    #[serde(skip_serializing_if = "is_false")]
    merge_dynamic_fields: bool,
}

#[derive(Deserialize)]
struct ScopeOwned {
    #[serde(default)]
    target_field: String,
    #[serde(default)]
    construction: ScopeConstruction,
    #[serde(default)]
    source: Option<SourceSelector>,
    #[serde(default)]
    sequence: Option<AnySequence>,
    #[serde(default)]
    join: Option<JoinOwned>,
    #[serde(default)]
    segments: Option<ScopeSequence>,
    #[serde(default)]
    filter: Option<NodeId>,
    #[serde(default)]
    post_group_filter: Option<NodeId>,
    #[serde(default)]
    group_by: Option<NodeId>,
    #[serde(default)]
    group_adjacent_by: Option<NodeId>,
    #[serde(default)]
    group_starting_with: Option<NodeId>,
    #[serde(default)]
    group_ending_with: Option<NodeId>,
    #[serde(default)]
    group_into_blocks: Option<NodeId>,
    #[serde(default)]
    sort_by: Option<NodeId>,
    #[serde(default)]
    sort_descending: bool,
    #[serde(default)]
    sort_then_by: Vec<SortKey>,
    #[serde(default)]
    sort_filter_order: SortFilterOrder,
    #[serde(default)]
    windows: Vec<SequenceWindow>,
    #[serde(default)]
    output_path: Option<NodeId>,
    #[serde(default)]
    iteration_output: IterationOutput,
    #[serde(default)]
    bindings: Vec<Binding>,
    #[serde(default)]
    dynamic_bindings: Vec<DynamicBinding>,
    #[serde(default)]
    children: Vec<Scope>,
    #[serde(default)]
    dynamic_children: Vec<DynamicChild>,
    #[serde(default)]
    merge_dynamic_fields: bool,
}

impl Serialize for Scope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (source, sequence, join, segments) = match &self.iteration {
            ScopeIteration::None => (None, None, None, None),
            ScopeIteration::Source(path) => {
                (Some(SourceSelectorRef::Legacy(path)), None, None, None)
            }
            ScopeIteration::DynamicDocuments { source, .. } => {
                (Some(SourceSelectorRef::Legacy(source)), None, None, None)
            }
            ScopeIteration::OwnedSource { source, .. } => {
                (Some(SourceSelectorRef::Owned(source)), None, None, None)
            }
            ScopeIteration::Sequence(sequence) => {
                (None, Some(SequenceRef::Generated(sequence)), None, None)
            }
            ScopeIteration::OptionalPresence { condition } => (
                None,
                Some(SequenceRef::Presence(PresenceWire::OptionalPresence {
                    condition: *condition,
                })),
                None,
                None,
            ),
            ScopeIteration::InnerJoin { id, plan } => (
                None,
                None,
                Some(JoinRef {
                    id: *id,
                    plan: JoinPlanRef::Legacy(plan),
                }),
                None,
            ),
            ScopeIteration::OwnedInnerJoin { id, plan } => (
                None,
                None,
                Some(JoinRef {
                    id: *id,
                    plan: JoinPlanRef::Owned(plan),
                }),
                None,
            ),
            ScopeIteration::Concatenate(segments) => (None, None, None, Some(segments)),
        };
        ScopeRef {
            target_field: &self.target_field,
            construction: self.construction.clone(),
            source,
            sequence,
            join,
            segments,
            filter: self.filter,
            post_group_filter: self.post_group_filter,
            group_by: self.group_by,
            group_adjacent_by: self.group_adjacent_by,
            group_starting_with: self.group_starting_with,
            group_ending_with: self.group_ending_with,
            group_into_blocks: self.group_into_blocks,
            sort_by: self.sort_by,
            sort_descending: self.sort_descending,
            sort_then_by: &self.sort_then_by,
            sort_filter_order: self.sort_filter_order,
            windows: &self.windows,
            output_path: self.output_path(),
            iteration_output: self.iteration_output,
            bindings: &self.bindings,
            dynamic_bindings: &self.dynamic_bindings,
            children: &self.children,
            dynamic_children: &self.dynamic_children,
            merge_dynamic_fields: self.merge_dynamic_fields,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ScopeOwned::deserialize(deserializer)?;
        let iteration_count = usize::from(wire.source.is_some())
            + usize::from(wire.sequence.is_some())
            + usize::from(wire.join.is_some())
            + usize::from(wire.segments.is_some());
        if iteration_count > 1 {
            return Err(serde::de::Error::custom(
                "scope source, sequence, join, and concatenated iteration forms are mutually exclusive",
            ));
        }
        let iteration = match (
            wire.source,
            wire.sequence,
            wire.join,
            wire.segments,
            wire.output_path,
        ) {
            (Some(SourceSelector::Legacy(source)), None, None, None, Some(output_path)) => {
                ScopeIteration::DynamicDocuments {
                    source,
                    output_path,
                }
            }
            (Some(SourceSelector::Legacy(path)), None, None, None, None) => {
                ScopeIteration::Source(path)
            }
            (Some(SourceSelector::Owned(source)), None, None, None, output_path) => {
                ScopeIteration::OwnedSource {
                    source,
                    output_path,
                }
            }
            (None, Some(AnySequence::Generated(sequence)), None, None, None) => {
                ScopeIteration::Sequence(sequence)
            }
            (None, Some(AnySequence::Presence(condition)), None, None, None) => {
                ScopeIteration::OptionalPresence { condition }
            }
            (None, None, Some(join), None, None) => match join.plan {
                AnyJoinPlan::Legacy(plan) => ScopeIteration::InnerJoin { id: join.id, plan },
                AnyJoinPlan::Owned(plan) => ScopeIteration::OwnedInnerJoin { id: join.id, plan },
            },
            (None, None, None, Some(segments), None) => ScopeIteration::Concatenate(segments),
            (None, None, None, None, None) => ScopeIteration::None,
            _ => {
                return Err(serde::de::Error::custom(
                    "scope output_path requires source iteration; source, sequence, join, and concatenated iteration forms are mutually exclusive",
                ));
            }
        };
        Ok(Scope {
            target_field: wire.target_field,
            iteration,
            construction: wire.construction,
            filter: wire.filter,
            post_group_filter: wire.post_group_filter,
            group_by: wire.group_by,
            group_adjacent_by: wire.group_adjacent_by,
            group_starting_with: wire.group_starting_with,
            group_ending_with: wire.group_ending_with,
            group_into_blocks: wire.group_into_blocks,
            sort_by: wire.sort_by,
            sort_descending: wire.sort_descending,
            sort_then_by: wire.sort_then_by,
            sort_filter_order: wire.sort_filter_order,
            windows: wire.windows,
            iteration_output: wire.iteration_output,
            bindings: wire.bindings,
            dynamic_bindings: wire.dynamic_bindings,
            children: wire.children,
            dynamic_children: wire.dynamic_children,
            merge_dynamic_fields: wire.merge_dynamic_fields,
        })
    }
}

fn is_default_sort_filter_order(order: &SortFilterOrder) -> bool {
    *order == SortFilterOrder::SortThenFilter
}
