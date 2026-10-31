//! Independently editable settings of one target scope, as typed values a
//! graph patch can compare, replace, and restore.

use std::collections::BTreeMap;

use mapping::{
    Binding, DynamicBinding, IterationOutput, JoinId, JoinPlan, NodeId, OwnedSourceRef, Scope,
    ScopeIteration, SequenceExpr, SequenceWindow, SortFilterOrder, SortKey,
};

/// One setting of a scope. Bindings, nested scopes, and the construction
/// mode are not settings. Joined and generated iterations and computed
/// properties are settings that only their own graph actions change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeSetting {
    /// The iteration form and its per-item output path.
    Iteration,
    Filter,
    /// Every grouping expression and the filter applied after grouping.
    Grouping,
    /// The primary sort key, its tie-breakers, and the filter order.
    Sort,
    Windows,
    IterationOutput,
    /// The ordered computed properties of an open target object.
    ComputedProperties,
}

impl ScopeSetting {
    pub const ALL: [Self; 7] = [
        Self::Iteration,
        Self::Filter,
        Self::Grouping,
        Self::Sort,
        Self::Windows,
        Self::IterationOutput,
        Self::ComputedProperties,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Iteration => "Change scope iteration",
            Self::Filter => "Change scope filter",
            Self::Grouping => "Change scope grouping",
            Self::Sort => "Change scope sort",
            Self::Windows => "Change scope windows",
            Self::IterationOutput => "Change scope output",
            Self::ComputedProperties => "Change computed properties",
        }
    }
}

/// The iteration forms editors author: source-backed iterations, generated
/// sequences, optional presence, and joins over static collections. Owned joins
/// and concatenated segments have no setting value, so edits that involve them
/// are not expressed as scope settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Iteration {
    None,
    Source(Vec<String>),
    Owned {
        source: OwnedSourceRef,
        output_path: Option<NodeId>,
    },
    DynamicDocuments {
        source: Vec<String>,
        output_path: NodeId,
    },
    Generated(SequenceExpr),
    OptionalPresence {
        condition: NodeId,
    },
    Joined {
        id: JoinId,
        plan: JoinPlan,
    },
}

impl Iteration {
    /// Whether this is one of the source-backed forms the scope editor's
    /// iteration controls choose between.
    pub fn is_source_backed(&self) -> bool {
        matches!(
            self,
            Self::None | Self::Source(_) | Self::Owned { .. } | Self::DynamicDocuments { .. }
        )
    }

    fn of(iteration: &ScopeIteration) -> Option<Self> {
        Some(match iteration {
            ScopeIteration::None => Self::None,
            ScopeIteration::Source(path) => Self::Source(path.clone()),
            ScopeIteration::OwnedSource {
                source,
                output_path,
            } => Self::Owned {
                source: source.clone(),
                output_path: *output_path,
            },
            ScopeIteration::DynamicDocuments {
                source,
                output_path,
            } => Self::DynamicDocuments {
                source: source.clone(),
                output_path: *output_path,
            },
            ScopeIteration::Sequence(sequence) => Self::Generated(sequence.clone()),
            ScopeIteration::OptionalPresence { condition } => Self::OptionalPresence {
                condition: *condition,
            },
            ScopeIteration::InnerJoin { id, plan } => Self::Joined {
                id: *id,
                plan: plan.clone(),
            },
            ScopeIteration::OwnedInnerJoin { .. } | ScopeIteration::Concatenate(_) => return None,
        })
    }

    fn iteration(&self) -> ScopeIteration {
        match self {
            Self::None => ScopeIteration::None,
            Self::Source(path) => ScopeIteration::Source(path.clone()),
            Self::Owned {
                source,
                output_path,
            } => ScopeIteration::OwnedSource {
                source: source.clone(),
                output_path: *output_path,
            },
            Self::DynamicDocuments {
                source,
                output_path,
            } => ScopeIteration::DynamicDocuments {
                source: source.clone(),
                output_path: *output_path,
            },
            Self::Generated(sequence) => ScopeIteration::Sequence(sequence.clone()),
            Self::OptionalPresence { condition } => ScopeIteration::OptionalPresence {
                condition: *condition,
            },
            Self::Joined { id, plan } => ScopeIteration::InnerJoin {
                id: *id,
                plan: plan.clone(),
            },
        }
    }
}

/// Grouping expressions of one scope; at most one mode is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grouping {
    pub by: Option<NodeId>,
    pub adjacent_by: Option<NodeId>,
    pub starting_with: Option<NodeId>,
    pub ending_with: Option<NodeId>,
    pub into_blocks: Option<NodeId>,
    pub post_group_filter: Option<NodeId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sorting {
    pub by: Option<NodeId>,
    pub descending: bool,
    pub then_by: Vec<SortKey>,
    pub filter_order: SortFilterOrder,
}

/// The value of one scope setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeSettingValue {
    Iteration(Iteration),
    Filter(Option<NodeId>),
    Grouping(Grouping),
    Sort(Sorting),
    Windows(Vec<SequenceWindow>),
    IterationOutput(IterationOutput),
    ComputedProperties(Vec<DynamicBinding>),
}

impl ScopeSettingValue {
    /// Reads one setting of `scope`, or `None` when its current form is not
    /// a setting value.
    pub fn read(setting: ScopeSetting, scope: &Scope) -> Option<Self> {
        Some(match setting {
            ScopeSetting::Iteration => Self::Iteration(Iteration::of(&scope.iteration)?),
            ScopeSetting::Filter => Self::Filter(scope.filter),
            ScopeSetting::Grouping => Self::Grouping(Grouping {
                by: scope.group_by,
                adjacent_by: scope.group_adjacent_by,
                starting_with: scope.group_starting_with,
                ending_with: scope.group_ending_with,
                into_blocks: scope.group_into_blocks,
                post_group_filter: scope.post_group_filter,
            }),
            ScopeSetting::Sort => Self::Sort(Sorting {
                by: scope.sort_by,
                descending: scope.sort_descending,
                then_by: scope.sort_then_by.clone(),
                filter_order: scope.sort_filter_order,
            }),
            ScopeSetting::Windows => Self::Windows(scope.windows.clone()),
            ScopeSetting::IterationOutput => Self::IterationOutput(scope.iteration_output),
            ScopeSetting::ComputedProperties => {
                Self::ComputedProperties(scope.dynamic_bindings.clone())
            }
        })
    }

    pub fn setting(&self) -> ScopeSetting {
        match self {
            Self::Iteration(_) => ScopeSetting::Iteration,
            Self::Filter(_) => ScopeSetting::Filter,
            Self::Grouping(_) => ScopeSetting::Grouping,
            Self::Sort(_) => ScopeSetting::Sort,
            Self::Windows(_) => ScopeSetting::Windows,
            Self::IterationOutput(_) => ScopeSetting::IterationOutput,
            Self::ComputedProperties(_) => ScopeSetting::ComputedProperties,
        }
    }

    /// Replaces this setting of `scope`, leaving every other field intact.
    pub(crate) fn write(&self, scope: &mut Scope) {
        match self {
            Self::Iteration(iteration) => scope.iteration = iteration.iteration(),
            Self::Filter(filter) => scope.filter = *filter,
            Self::Grouping(grouping) => {
                scope.group_by = grouping.by;
                scope.group_adjacent_by = grouping.adjacent_by;
                scope.group_starting_with = grouping.starting_with;
                scope.group_ending_with = grouping.ending_with;
                scope.group_into_blocks = grouping.into_blocks;
                scope.post_group_filter = grouping.post_group_filter;
            }
            Self::Sort(sorting) => {
                scope.sort_by = sorting.by;
                scope.sort_descending = sorting.descending;
                scope.sort_then_by = sorting.then_by.clone();
                scope.sort_filter_order = sorting.filter_order;
            }
            Self::Windows(windows) => scope.windows = windows.clone(),
            Self::IterationOutput(output) => scope.iteration_output = *output,
            Self::ComputedProperties(properties) => scope.dynamic_bindings = properties.clone(),
        }
    }
}

/// The settings and bindings of one scope, captured so an editor pass over
/// that scope alone can be described as typed changes.
#[derive(Clone, Debug)]
pub struct ScopeFields {
    settings: BTreeMap<ScopeSetting, ScopeSettingValue>,
    bindings: Vec<Binding>,
}

impl ScopeFields {
    pub fn capture(scope: &Scope) -> Self {
        Self {
            settings: ScopeSetting::ALL
                .into_iter()
                .filter_map(|setting| {
                    ScopeSettingValue::read(setting, scope).map(|value| (setting, value))
                })
                .collect(),
            bindings: scope.bindings.clone(),
        }
    }

    pub fn setting(&self, setting: ScopeSetting) -> Option<&ScopeSettingValue> {
        self.settings.get(&setting)
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Settings with a value on both sides that differ between `self` and
    /// `after`. A setting whose form has no value on either side is omitted,
    /// so a host must still verify that the captured changes explain an edit.
    pub fn changed_settings<'a>(
        &'a self,
        after: &'a Self,
    ) -> impl Iterator<Item = (&'a ScopeSettingValue, &'a ScopeSettingValue)> {
        self.settings.iter().filter_map(|(setting, before)| {
            after
                .settings
                .get(setting)
                .filter(|after| *after != before)
                .map(|after| (before, after))
        })
    }
}
