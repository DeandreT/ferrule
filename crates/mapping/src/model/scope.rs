use serde::{Deserialize, Serialize};

use crate::{
    AdjacencyTreePlan, Binding, DynamicBinding, DynamicChild, JoinId, JoinPlan, NodeId,
    PathHierarchyPlan, RecursiveFilterPlan, ScopeIteration, ScopeSequence, SequenceExpr,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IterationOutput {
    /// Preserve every produced item as a repeating target value.
    #[default]
    Repeated,
    /// Produce the first surviving item as a non-repeating target group.
    First,
    /// Preserve mapping-produced XML occurrences independently of the
    /// target schema's declared repetition.
    MappedSequence,
}

/// How a scope produces each target group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeConstruction {
    /// Build a new group from the scope's bindings and child scopes.
    #[default]
    Constructed,
    /// Clone the current source item as one complete group.
    CopyCurrentSource,
    /// Evaluate one scalar expression as the scope's complete output item.
    Scalar { value: NodeId },
    /// Build a group while retaining the current source group's interleaved
    /// XML text and mapped child-element order.
    XmlMixedContent {
        elements: Vec<XmlMixedContentElement>,
    },
    /// Clone one group shape while recursively filtering one item collection.
    RecursiveFilter { plan: RecursiveFilterPlan },
    /// Group scalar path strings into one bounded recursive directory tree.
    PathHierarchy { plan: PathHierarchyPlan },
    /// Build a bounded recursive target group from flat adjacency rows.
    AdjacencyTree { plan: AdjacencyTreePlan },
}

/// One direct-child rename in an XML mixed-content construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlMixedContentElement {
    pub source: String,
    pub target: String,
}

/// Relative order of the two ordinary per-item sequence controls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortFilterOrder {
    /// Sort candidates before evaluating the filter predicate.
    #[default]
    SortThenFilter,
    /// Filter candidates in source order, then sort the surviving items.
    FilterThenSort,
}

/// One ordered window applied to an iterated sequence after ordinary
/// sorting, filtering, and grouping controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SequenceWindow {
    /// Drop the first `count` items.
    SkipFirst { count: NodeId },
    /// Retain at most the first `count` items.
    First { count: NodeId },
    /// Retain items beginning at the one-based `position`.
    From { position: NodeId },
    /// Retain the inclusive one-based range from `first` through `last`.
    FromTo { first: NodeId, last: NodeId },
    /// Retain at most the final `count` items.
    Last { count: NodeId },
}

impl SequenceWindow {
    pub fn nodes(self) -> impl Iterator<Item = NodeId> {
        let nodes = match self {
            Self::SkipFirst { count } | Self::First { count } | Self::Last { count } => {
                [Some(count), None]
            }
            Self::From { position } => [Some(position), None],
            Self::FromTo { first, last } => [Some(first), Some(last)],
        };
        nodes.into_iter().flatten()
    }
}

/// Populates one target group.
///
/// [`ScopeIteration::Source`] follows a path from the parent scope's current
/// item, branching whenever it crosses repetition. A generated sequence or
/// inner join supplies items instead. [`ScopeIteration::None`] runs exactly
/// once and produces a non-repeating group.
///
/// Iterating scopes use [`Scope::iteration_output`] to retain every produced
/// group or return only the first surviving group. First-item output returns
/// an empty group when no item survives. Mapped-sequence output retains zero
/// or more XML element occurrences without changing schema cardinality.
/// Sorting, filtering, grouping, and ordered windows are applied before output
/// cardinality is selected.
///
/// If `filter` is set, it is evaluated in the same per-item context as
/// `bindings`; items for which it returns `false` are dropped. `sort_by`
/// stably orders candidates before filtering/grouping. `windows` then applies
/// zero or more ordered prefix, suffix, or range selections. These controls
/// apply to both source-backed and generated iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub node: NodeId,
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub descending: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Name of the field this scope populates in its parent scope; ignored
    /// for a project's root scope.
    pub target_field: String,
    /// Exactly one iteration form, or `None` for a non-iterating scope.
    pub iteration: ScopeIteration,
    /// Whether this scope constructs fields or preserves the complete current
    /// source group. Copy construction is deliberately incompatible with
    /// bindings, child scopes, generated sequences, joins, and grouping.
    pub construction: ScopeConstruction,
    pub filter: Option<NodeId>,
    /// Groups the iterated items by this key expression (evaluated once
    /// per item): the scope then produces one target group per distinct
    /// key, in first-seen order, and the iteration frame becomes the
    /// group's members -- so bindings read the first member, aggregates
    /// reduce the members, and nested scopes iterate them. Only meaningful
    /// for a source-backed or generated iteration.
    pub group_by: Option<NodeId>,
    /// Partitions iterated items into contiguous runs with equal key values.
    /// Unlike `group_by`, a later run with a previously seen key remains a
    /// separate group. Mutually exclusive with the other grouping modes.
    pub group_adjacent_by: Option<NodeId>,
    /// Partitions items into contiguous groups whenever this per-item
    /// predicate is true. Items before its first true result form an initial
    /// group. Mutually exclusive with the other grouping modes.
    pub group_starting_with: Option<NodeId>,
    /// Partitions items into contiguous groups that end whenever this
    /// per-item predicate is true. Items after the final true result form a
    /// trailing group. Mutually exclusive with the other grouping modes.
    pub group_ending_with: Option<NodeId>,
    /// Partitions iterated items into contiguous groups of this many members.
    /// The expression is evaluated once in the parent context and must produce
    /// a positive item count. Mutually exclusive with the other grouping
    /// modes.
    pub group_into_blocks: Option<NodeId>,
    /// Sort key evaluated once per candidate item. Incomparable values keep
    /// their source order.
    pub sort_by: Option<NodeId>,
    pub sort_descending: bool,
    /// Successive tie-breakers, evaluated in declaration order after the
    /// primary `sort_by` key.
    pub sort_then_by: Vec<SortKey>,
    pub sort_filter_order: SortFilterOrder,
    /// Ordered sequence windows whose bounds are evaluated once in the parent
    /// context. Window order is semantically significant.
    pub windows: Vec<SequenceWindow>,
    /// Cardinality of an iterating scope's target value. Older projects omit
    /// this field and retain the original repeated behavior.
    pub iteration_output: IterationOutput,
    pub bindings: Vec<Binding>,
    /// Computed scalar fields of an open target group.
    pub dynamic_bindings: Vec<DynamicBinding>,
    pub children: Vec<Scope>,
    /// Computed fields whose values are complete child scopes (objects or
    /// arrays). Kept separate from `children` so static and computed names
    /// cannot form an invalid mixed target descriptor.
    pub dynamic_children: Vec<DynamicChild>,
    /// An iterating scope normally produces an array. For an open object,
    /// each iteration may instead produce one property fragment; this flag
    /// merges those fragments into one object and rejects duplicate names.
    pub merge_dynamic_fields: bool,
}

impl Scope {
    /// Graph nodes owned by grouping controls, in stable mode order.
    pub fn grouping_nodes(&self) -> impl Iterator<Item = NodeId> {
        [
            self.group_by,
            self.group_adjacent_by,
            self.group_starting_with,
            self.group_ending_with,
            self.group_into_blocks,
        ]
        .into_iter()
        .flatten()
    }

    pub fn has_grouping(&self) -> bool {
        self.grouping_nodes().next().is_some()
    }

    pub fn has_conflicting_grouping(&self) -> bool {
        self.grouping_nodes().nth(1).is_some()
    }

    pub fn sort_keys(&self) -> impl Iterator<Item = SortKey> + '_ {
        self.sort_by
            .map(|node| SortKey {
                node,
                descending: self.sort_descending,
            })
            .into_iter()
            .chain(self.sort_then_by.iter().copied())
    }

    pub fn has_sort(&self) -> bool {
        self.sort_by.is_some() || !self.sort_then_by.is_empty()
    }

    pub fn source(&self) -> Option<&[String]> {
        self.iteration.source()
    }

    pub fn source_mut(&mut self) -> Option<&mut Vec<String>> {
        match &mut self.iteration {
            ScopeIteration::Source(path)
            | ScopeIteration::DynamicDocuments { source: path, .. } => Some(path),
            ScopeIteration::None
            | ScopeIteration::Sequence(_)
            | ScopeIteration::InnerJoin { .. }
            | ScopeIteration::Concatenate(_) => None,
        }
    }

    pub fn set_source(&mut self, source: Option<Vec<String>>) {
        match (source, &mut self.iteration) {
            (Some(path), ScopeIteration::DynamicDocuments { source, .. }) => *source = path,
            (Some(path), _) => self.iteration = ScopeIteration::Source(path),
            (None, ScopeIteration::Source(_) | ScopeIteration::DynamicDocuments { .. }) => {
                self.iteration = ScopeIteration::None;
            }
            (None, _) => {}
        }
    }

    /// Graph expression paired with every complete document produced by this
    /// scope. The typed iteration variant keeps the path expression and its
    /// source item context inseparable.
    pub fn output_path(&self) -> Option<NodeId> {
        match self.iteration {
            ScopeIteration::DynamicDocuments { output_path, .. } => Some(output_path),
            _ => None,
        }
    }

    /// Adds or removes per-item output path ownership. Adding it requires an
    /// existing source iteration and cannot silently replace another form.
    pub fn set_output_path(&mut self, output_path: Option<NodeId>) -> bool {
        match (output_path, &mut self.iteration) {
            (Some(output_path), ScopeIteration::Source(source)) => {
                self.iteration = ScopeIteration::DynamicDocuments {
                    source: std::mem::take(source),
                    output_path,
                };
                true
            }
            (
                Some(output_path),
                ScopeIteration::DynamicDocuments {
                    output_path: current,
                    ..
                },
            ) => {
                *current = output_path;
                true
            }
            (Some(_), _) => false,
            (None, ScopeIteration::DynamicDocuments { source, .. }) => {
                self.iteration = ScopeIteration::Source(std::mem::take(source));
                true
            }
            (None, _) => true,
        }
    }

    pub fn sequence(&self) -> Option<&SequenceExpr> {
        self.iteration.sequence()
    }

    pub fn sequence_mut(&mut self) -> Option<&mut SequenceExpr> {
        match &mut self.iteration {
            ScopeIteration::Sequence(sequence) => Some(sequence),
            ScopeIteration::None
            | ScopeIteration::Source(_)
            | ScopeIteration::DynamicDocuments { .. }
            | ScopeIteration::InnerJoin { .. }
            | ScopeIteration::Concatenate(_) => None,
        }
    }

    pub fn set_sequence(&mut self, sequence: Option<SequenceExpr>) {
        match sequence {
            Some(sequence) => self.iteration = ScopeIteration::Sequence(sequence),
            None if matches!(self.iteration, ScopeIteration::Sequence(_)) => {
                self.iteration = ScopeIteration::None;
            }
            None => {}
        }
    }

    pub fn join(&self) -> Option<(JoinId, &JoinPlan)> {
        self.iteration.join()
    }

    pub fn concatenated(&self) -> Option<&ScopeSequence> {
        self.iteration.concatenated()
    }

    pub fn concatenated_mut(&mut self) -> Option<&mut ScopeSequence> {
        self.iteration.concatenated_mut()
    }

    pub fn iterates(&self) -> bool {
        self.iteration.iterates()
    }
}

pub(crate) fn is_repeated_output(output: &IterationOutput) -> bool {
    *output == IterationOutput::Repeated
}

pub(crate) fn is_constructed_scope(construction: &ScopeConstruction) -> bool {
    matches!(construction, ScopeConstruction::Constructed)
}
