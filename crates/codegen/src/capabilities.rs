//! Owned generation admission as one explicit capability set.
//!
//! Every explicit owned generation entry point names an [`OwnedPreset`]; its
//! [`OwnedCapabilities`] are read by the single owned-source validator, the
//! named-input planner, lowering and both emitters. Capabilities are
//! independent permissions: each admits one shape or operator, and a closed
//! capability keeps that construct's existing typed refusal. The presets are
//! the one declarative table below; see `docs/code-generation.md` for how a
//! new capability or preset is added.

use crate::{Expression, GeneratedSequence, GroupingPlan};

/// Static Basic source controls. Each admits one scope shape over an ordinary
/// or owned source selector with a `Group` or `Scalar` target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceControls {
    /// A filtered repeated scope without sort, grouping or windows.
    pub filter: bool,
    /// A sorted repeated scope without grouping or windows; it may filter.
    pub sort: bool,
    /// An ungrouped scope with ordered windows or an implicit `First` group
    /// output; it may filter and sort.
    pub windows: bool,
}

/// Grouping modes a static Basic scope may partition by. A grouped scope may
/// also filter, sort, window and retain members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupingModes {
    pub by: bool,
    pub adjacent_by: bool,
    pub starting_with: bool,
    pub ending_with: bool,
    pub into_blocks: bool,
}

impl GroupingModes {
    /// Whether any grouping mode is admitted.
    pub const fn any(self) -> bool {
        self.by || self.adjacent_by || self.starting_with || self.ending_with || self.into_blocks
    }

    /// Whether `grouping` is an admitted mode; no grouping is never a mode.
    pub const fn permits(self, grouping: Option<GroupingPlan>) -> bool {
        match grouping {
            Some(GroupingPlan::By { .. }) => self.by,
            Some(GroupingPlan::AdjacentBy { .. }) => self.adjacent_by,
            Some(GroupingPlan::StartingWith { .. }) => self.starting_with,
            Some(GroupingPlan::EndingWith { .. }) => self.ending_with,
            Some(GroupingPlan::IntoBlocks { .. }) => self.into_blocks,
            None => false,
        }
    }
}

/// Failure-rule permissions, independent of target controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FailureSources {
    /// Global rules over authenticated primary rows whose closed scalar
    /// predicate and message read owned fields.
    pub primary_rows: bool,
    /// Rules that walk one exact declared endpoint with a closed scalar
    /// predicate and message.
    pub owned_sources: bool,
}

/// Generated-sequence consumers in a receiving Basic context. Producers are
/// the four scalar producers unless stated otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SequenceConsumers {
    /// `SequenceItemAt`; producer inputs and the index keep the receiving context.
    pub item_at: bool,
    /// `SequenceExists`; its predicate adds only the private generated item.
    pub exists: bool,
    /// `SequenceAggregate`; its predicate and expression add only the private
    /// item and its argument keeps the receiving context.
    pub aggregate: bool,
    /// `SequenceItemAt` also over an owned recursive collection walk.
    pub owned_recursive_item_at: bool,
}

impl SequenceConsumers {
    /// Whether any consumer is admitted.
    pub const fn any(self) -> bool {
        self.item_at || self.exists || self.aggregate
    }

    /// Whether `expression` is an admitted consumer of an admitted producer.
    pub fn permits(self, expression: &Expression) -> bool {
        let (sequence, item_at) = match expression {
            Expression::SequenceItemAt { sequence, .. } if self.item_at => (sequence, true),
            Expression::SequenceExists { sequence, .. } if self.exists => (sequence, false),
            Expression::SequenceAggregate { sequence, .. } if self.aggregate => (sequence, false),
            _ => return false,
        };
        matches!(
            sequence,
            GeneratedSequence::Range { .. }
                | GeneratedSequence::Tokenize { .. }
                | GeneratedSequence::TokenizeByLength { .. }
                | GeneratedSequence::TokenizeRegex { .. }
        ) || item_at
            && self.owned_recursive_item_at
            && matches!(sequence, GeneratedSequence::OwnedRecursiveCollect { .. })
    }
}

/// Owned tree constructions. Legacy trees keep the ordinary emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OwnedConstructions {
    /// An owned path-hierarchy input collection.
    pub path_hierarchy: bool,
    /// Owned adjacency-tree rows; the optional root keeps the receiving context.
    pub adjacency_tree: bool,
    /// Recursive filtering of a scope's own owned selector.
    pub recursive_filter: bool,
}

/// Owned collection searches and reductions over one exact endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OwnedSearch {
    /// Owned `Lookup` and `CollectionFind`.
    pub lookup_find: bool,
    /// Owned collection aggregates.
    pub collection_aggregates: bool,
}

/// Owned XML operators. Each level admits every operator of the levels below
/// it; legacy XML operators keep their own gate at every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwnedXml {
    Closed,
    /// Owned XML serialization.
    Serialize,
    /// Serialization plus owned mixed-content atomization.
    MixedContent,
    /// Atomization plus mixed-content construction by an ungrouped scope's own
    /// owned selector.
    MixedConstruction,
}

/// One complete owned generation admission. Construct it from an
/// [`OwnedPreset`]; every field is an independent permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OwnedCapabilities {
    pub controls: SourceControls,
    pub grouping: GroupingModes,
    pub failures: FailureSources,
    pub sequences: SequenceConsumers,
    pub constructions: OwnedConstructions,
    pub search: OwnedSearch,
    /// Owned dynamic source-field reads; the key keeps the receiving context.
    pub dynamic_fields: bool,
    /// Owned dynamic driver loading: a loading scope walks its endpoint's exact
    /// driver collection, evaluates the path in each driver item, and exact
    /// field and position reads see the loaded document.
    pub dynamic_loading: bool,
    pub xml: OwnedXml,
    /// Owned inner join scopes, their tuple reads and owned joined aggregates.
    pub joins: bool,
}

impl OwnedCapabilities {
    /// Whether every permission of `self` is also granted by `other`.
    pub const fn is_within(self, other: Self) -> bool {
        const fn within(this: bool, other: bool) -> bool {
            !this || other
        }
        let (c, g, f, s) = (self.controls, self.grouping, self.failures, self.sequences);
        let (t, q) = (self.constructions, self.search);
        within(c.filter, other.controls.filter)
            && within(c.sort, other.controls.sort)
            && within(c.windows, other.controls.windows)
            && within(g.by, other.grouping.by)
            && within(g.adjacent_by, other.grouping.adjacent_by)
            && within(g.starting_with, other.grouping.starting_with)
            && within(g.ending_with, other.grouping.ending_with)
            && within(g.into_blocks, other.grouping.into_blocks)
            && within(f.primary_rows, other.failures.primary_rows)
            && within(f.owned_sources, other.failures.owned_sources)
            && within(s.item_at, other.sequences.item_at)
            && within(s.exists, other.sequences.exists)
            && within(s.aggregate, other.sequences.aggregate)
            && within(
                s.owned_recursive_item_at,
                other.sequences.owned_recursive_item_at,
            )
            && within(t.path_hierarchy, other.constructions.path_hierarchy)
            && within(t.adjacency_tree, other.constructions.adjacency_tree)
            && within(t.recursive_filter, other.constructions.recursive_filter)
            && within(q.lookup_find, other.search.lookup_find)
            && within(q.collection_aggregates, other.search.collection_aggregates)
            && within(self.dynamic_fields, other.dynamic_fields)
            && within(self.dynamic_loading, other.dynamic_loading)
            && self.xml as u8 <= other.xml as u8
            && within(self.joins, other.joins)
    }
}

/// A named owned generation admission. Each preset is one row of the table in
/// [`OwnedPreset::capabilities`] and backs one family of public entry points
/// (`lower_with_basic_owned_*`, `validate_program_with_basic_owned_*`,
/// `validate_basic_owned_*_emission`, `legacy_named_input_admission_plan_with_basic_owned_*`
/// and both emitters' `emit_with_basic_owned_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedPreset {
    /// The common control-free Basic source context (`*_basic_owned_sources`).
    ControlFree,
    Filters,
    Sort,
    Windows,
    RowFailures,
    GroupBy,
    SequenceItemAt,
    GroupAdjacentBy,
    GroupStartingWith,
    GroupEndingWith,
    GroupIntoBlocks,
    FailureSources,
    PathHierarchy,
    AdjacencyTree,
    RecursiveCollect,
    RecursiveFilter,
    LookupFind,
    DynamicField,
    DynamicLoading,
    CollectionAggregates,
    Joins,
    SequenceExists,
    SequenceAggregate,
    XmlSerialize,
    XmlMixedContent,
    XmlMixedConstruction,
}

const NO_CONTROLS: SourceControls = SourceControls {
    filter: false,
    sort: false,
    windows: false,
};
const FILTER: SourceControls = SourceControls {
    filter: true,
    ..NO_CONTROLS
};
const SORT: SourceControls = SourceControls {
    sort: true,
    ..FILTER
};
const WINDOWS: SourceControls = SourceControls {
    windows: true,
    ..SORT
};

const UNGROUPED: GroupingModes = GroupingModes {
    by: false,
    adjacent_by: false,
    starting_with: false,
    ending_with: false,
    into_blocks: false,
};
const BY: GroupingModes = GroupingModes {
    by: true,
    ..UNGROUPED
};
const TO_ADJACENT: GroupingModes = GroupingModes {
    adjacent_by: true,
    ..BY
};
const TO_STARTING: GroupingModes = GroupingModes {
    starting_with: true,
    ..TO_ADJACENT
};
const TO_ENDING: GroupingModes = GroupingModes {
    ending_with: true,
    ..TO_STARTING
};
const ALL_MODES: GroupingModes = GroupingModes {
    into_blocks: true,
    ..TO_ENDING
};

const NO_FAILURES: FailureSources = FailureSources {
    primary_rows: false,
    owned_sources: false,
};
const ROWS: FailureSources = FailureSources {
    primary_rows: true,
    owned_sources: false,
};
const OWNED: FailureSources = FailureSources {
    primary_rows: true,
    owned_sources: true,
};

const NO_SEQUENCES: SequenceConsumers = SequenceConsumers {
    item_at: false,
    exists: false,
    aggregate: false,
    owned_recursive_item_at: false,
};
const ITEM_AT: SequenceConsumers = SequenceConsumers {
    item_at: true,
    ..NO_SEQUENCES
};
const RECURSIVE: SequenceConsumers = SequenceConsumers {
    owned_recursive_item_at: true,
    ..ITEM_AT
};
const EXISTS: SequenceConsumers = SequenceConsumers {
    exists: true,
    ..ITEM_AT
};
const AGGREGATE: SequenceConsumers = SequenceConsumers {
    aggregate: true,
    ..EXISTS
};

const NO_TREES: OwnedConstructions = OwnedConstructions {
    path_hierarchy: false,
    adjacency_tree: false,
    recursive_filter: false,
};
const HIERARCHY: OwnedConstructions = OwnedConstructions {
    path_hierarchy: true,
    ..NO_TREES
};
const TREE: OwnedConstructions = OwnedConstructions {
    adjacency_tree: true,
    ..HIERARCHY
};
const FILTERED: OwnedConstructions = OwnedConstructions {
    recursive_filter: true,
    ..TREE
};

const NO_SEARCH: OwnedSearch = OwnedSearch {
    lookup_find: false,
    collection_aggregates: false,
};
const SEARCH: OwnedSearch = OwnedSearch {
    lookup_find: true,
    collection_aggregates: false,
};
const REDUCE: OwnedSearch = OwnedSearch {
    lookup_find: true,
    collection_aggregates: true,
};

const NO_XML: OwnedXml = OwnedXml::Closed;
const SERIALIZE: OwnedXml = OwnedXml::Serialize;
const MIXED: OwnedXml = OwnedXml::MixedContent;
const CONSTRUCT: OwnedXml = OwnedXml::MixedConstruction;

// Positional so each table row reads as one complete capability set.
#[allow(clippy::too_many_arguments)]
const fn set(
    controls: SourceControls,
    grouping: GroupingModes,
    failures: FailureSources,
    sequences: SequenceConsumers,
    constructions: OwnedConstructions,
    search: OwnedSearch,
    dynamic_fields: bool,
    dynamic_loading: bool,
    xml: OwnedXml,
    joins: bool,
) -> OwnedCapabilities {
    OwnedCapabilities {
        controls,
        grouping,
        failures,
        sequences,
        constructions,
        search,
        dynamic_fields,
        dynamic_loading,
        xml,
        joins,
    }
}

impl OwnedPreset {
    /// Every preset, in table order.
    pub const ALL: [Self; 26] = [
        Self::ControlFree,
        Self::Filters,
        Self::Sort,
        Self::Windows,
        Self::RowFailures,
        Self::GroupBy,
        Self::SequenceItemAt,
        Self::GroupAdjacentBy,
        Self::GroupStartingWith,
        Self::GroupEndingWith,
        Self::GroupIntoBlocks,
        Self::FailureSources,
        Self::PathHierarchy,
        Self::AdjacencyTree,
        Self::RecursiveCollect,
        Self::RecursiveFilter,
        Self::LookupFind,
        Self::DynamicField,
        Self::DynamicLoading,
        Self::CollectionAggregates,
        Self::Joins,
        Self::SequenceExists,
        Self::SequenceAggregate,
        Self::XmlSerialize,
        Self::XmlMixedContent,
        Self::XmlMixedConstruction,
    ];

    /// The preset table: every row lists its complete capability set.
    #[rustfmt::skip]
    pub const fn capabilities(self) -> OwnedCapabilities {
        //                                    controls     grouping     failures     sequences     trees      search     dyn    load   xml        joins
        match self {
            Self::ControlFree          => set(NO_CONTROLS, UNGROUPED,   NO_FAILURES, NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::Filters              => set(FILTER,      UNGROUPED,   NO_FAILURES, NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::Sort                 => set(SORT,        UNGROUPED,   NO_FAILURES, NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::Windows              => set(WINDOWS,     UNGROUPED,   NO_FAILURES, NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::RowFailures          => set(WINDOWS,     UNGROUPED,   ROWS,        NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::GroupBy              => set(WINDOWS,     BY,          ROWS,        NO_SEQUENCES, NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::SequenceItemAt       => set(WINDOWS,     BY,          ROWS,        ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::GroupAdjacentBy      => set(WINDOWS,     TO_ADJACENT, ROWS,        ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::GroupStartingWith    => set(WINDOWS,     TO_STARTING, ROWS,        ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::GroupEndingWith      => set(WINDOWS,     TO_ENDING,   ROWS,        ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::GroupIntoBlocks      => set(WINDOWS,     ALL_MODES,   ROWS,        ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::FailureSources       => set(WINDOWS,     TO_ENDING,   OWNED,       ITEM_AT,      NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::PathHierarchy        => set(WINDOWS,     TO_ENDING,   OWNED,       ITEM_AT,      HIERARCHY, NO_SEARCH, false, false, NO_XML,    false),
            Self::AdjacencyTree        => set(WINDOWS,     TO_ENDING,   OWNED,       ITEM_AT,      TREE,      NO_SEARCH, false, false, NO_XML,    false),
            Self::RecursiveCollect     => set(WINDOWS,     TO_ENDING,   OWNED,       RECURSIVE,    TREE,      NO_SEARCH, false, false, NO_XML,    false),
            Self::RecursiveFilter      => set(WINDOWS,     TO_ENDING,   OWNED,       RECURSIVE,    FILTERED,  NO_SEARCH, false, false, NO_XML,    false),
            Self::LookupFind           => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    false, false, NO_XML,    false),
            Self::DynamicField         => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    true,  false, NO_XML,    false),
            Self::DynamicLoading       => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    true,  true,  NO_XML,    false),
            Self::CollectionAggregates => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  REDUCE,    false, false, NO_XML,    false),
            Self::Joins                => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    false, false, NO_XML,    true),
            Self::SequenceExists       => set(WINDOWS,     TO_ENDING,   ROWS,        EXISTS,       NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::SequenceAggregate    => set(WINDOWS,     TO_ENDING,   ROWS,        AGGREGATE,    NO_TREES,  NO_SEARCH, false, false, NO_XML,    false),
            Self::XmlSerialize         => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    true,  false, SERIALIZE, false),
            Self::XmlMixedContent      => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    true,  false, MIXED,     false),
            Self::XmlMixedConstruction => set(WINDOWS,     ALL_MODES,   OWNED,       RECURSIVE,    FILTERED,  SEARCH,    true,  false, CONSTRUCT, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_distinct_capability_sets() {
        for (index, preset) in OwnedPreset::ALL.iter().enumerate() {
            for other in &OwnedPreset::ALL[index + 1..] {
                assert_ne!(
                    preset.capabilities(),
                    other.capabilities(),
                    "{preset:?} {other:?}"
                );
            }
        }
    }

    #[test]
    fn control_free_is_within_every_preset() {
        for preset in OwnedPreset::ALL {
            let capabilities = preset.capabilities();
            assert!(
                OwnedPreset::ControlFree
                    .capabilities()
                    .is_within(capabilities)
            );
            assert!(capabilities.is_within(capabilities));
        }
        assert!(
            !OwnedPreset::Sort
                .capabilities()
                .is_within(OwnedPreset::Filters.capabilities())
        );
    }
}
