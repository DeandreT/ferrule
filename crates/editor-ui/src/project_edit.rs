//! Project parts outside any one mapping document's graph and scope tree:
//! failure rules, named sources and targets, the primary source's format
//! options, and user-function definitions.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mapping::{
    FailureRule, FormatOptions, FunctionId, NamedSource, NamedTarget, Node, Project, UserFunction,
};

use crate::GraphEditError;
use crate::graph_edit::Side;

/// Decides whether two nodes have the same content.
pub(crate) type SameNode<'a> = &'a dyn Fn(&Node, &Node) -> bool;

/// One project part a patch can replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectPart {
    FailureRules,
    NamedSources,
    NamedTargets,
    SourceOptions,
    Function(FunctionId),
}

impl fmt::Display for ProjectPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FailureRules => f.write_str("the failure rules"),
            Self::NamedSources => f.write_str("the named sources"),
            Self::NamedTargets => f.write_str("the named targets"),
            Self::SourceOptions => f.write_str("the source format options"),
            Self::Function(id) => write!(f, "user function {}", id.get()),
        }
    }
}

/// One project part on both sides of an edit.
#[derive(Clone, Debug)]
pub struct PartChange<T> {
    before: T,
    after: T,
}

impl<T> PartChange<T> {
    pub fn before(&self) -> &T {
        &self.before
    }

    pub fn after(&self) -> &T {
        &self.after
    }

    fn side(&self, side: Side) -> &T {
        match side {
            Side::Before => &self.before,
            Side::After => &self.after,
        }
    }
}

impl<T: Clone + PartialEq> PartChange<T> {
    fn of(before: &T, after: &T) -> Option<Self> {
        (before != after).then(|| Self {
            before: before.clone(),
            after: after.clone(),
        })
    }

    /// The change equivalent to this one followed by `next`; `Some(None)`
    /// when together they change nothing.
    fn then(&self, next: &Self) -> Option<Option<Self>> {
        (self.after == next.before).then(|| Self::of(&self.before, &next.after))
    }
}

/// Whether two definitions match, with body nodes compared by `same_node`.
pub(crate) fn same_function(
    left: &UserFunction,
    right: &UserFunction,
    same_node: &dyn Fn(&Node, &Node) -> bool,
) -> bool {
    left.library == right.library
        && left.name == right.name
        && left.description == right.description
        && left.parameters == right.parameters
        && left.output_name == right.output_name
        && left.output_type == right.output_type
        && left.output == right.output
        && left.body.nodes.len() == right.body.nodes.len()
        && left.body.nodes.iter().zip(&right.body.nodes).all(
            |((left_id, left), (right_id, right))| left_id == right_id && same_node(left, right),
        )
}

fn same_presence(
    left: Option<&UserFunction>,
    right: Option<&UserFunction>,
    same_node: &dyn Fn(&Node, &Node) -> bool,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => same_function(left, right, same_node),
        (left, right) => left.is_none() && right.is_none(),
    }
}

/// The project parts one edit replaces. Parts it leaves unchanged are not
/// captured.
#[derive(Clone, Debug, Default)]
pub struct ProjectParts {
    failure_rules: Option<PartChange<Vec<FailureRule>>>,
    named_sources: Option<PartChange<Vec<NamedSource>>>,
    named_targets: Option<PartChange<Vec<NamedTarget>>>,
    source_options: Option<PartChange<FormatOptions>>,
    functions: BTreeMap<FunctionId, PartChange<Option<UserFunction>>>,
}

impl ProjectParts {
    /// Captures every part that differs between two project states.
    pub fn between(
        before: &Project,
        after: &Project,
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Self {
        let ids = before
            .user_functions
            .keys()
            .chain(after.user_functions.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let functions = ids
            .into_iter()
            .filter_map(|id| {
                let (left, right) = (
                    before.user_functions.get(&id),
                    after.user_functions.get(&id),
                );
                (!same_presence(left, right, &same_node)).then(|| {
                    (
                        id,
                        PartChange {
                            before: left.cloned(),
                            after: right.cloned(),
                        },
                    )
                })
            })
            .collect();
        Self {
            failure_rules: PartChange::of(&before.failure_rules, &after.failure_rules),
            named_sources: PartChange::of(&before.extra_sources, &after.extra_sources),
            named_targets: PartChange::of(&before.extra_targets, &after.extra_targets),
            source_options: PartChange::of(&before.source_options, &after.source_options),
            functions,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.failure_rules.is_none()
            && self.named_sources.is_none()
            && self.named_targets.is_none()
            && self.source_options.is_none()
            && self.functions.is_empty()
    }

    /// Every part the edit replaces.
    pub fn parts(&self) -> impl Iterator<Item = ProjectPart> + '_ {
        [
            self.failure_rules
                .as_ref()
                .map(|_| ProjectPart::FailureRules),
            self.named_sources
                .as_ref()
                .map(|_| ProjectPart::NamedSources),
            self.named_targets
                .as_ref()
                .map(|_| ProjectPart::NamedTargets),
            self.source_options
                .as_ref()
                .map(|_| ProjectPart::SourceOptions),
        ]
        .into_iter()
        .flatten()
        .chain(self.functions.keys().map(|&id| ProjectPart::Function(id)))
    }

    pub fn failure_rules(&self) -> Option<&PartChange<Vec<FailureRule>>> {
        self.failure_rules.as_ref()
    }

    pub fn named_sources(&self) -> Option<&PartChange<Vec<NamedSource>>> {
        self.named_sources.as_ref()
    }

    pub fn named_targets(&self) -> Option<&PartChange<Vec<NamedTarget>>> {
        self.named_targets.as_ref()
    }

    pub fn source_options(&self) -> Option<&PartChange<FormatOptions>> {
        self.source_options.as_ref()
    }

    pub fn function(&self, id: FunctionId) -> Option<&PartChange<Option<UserFunction>>> {
        self.functions.get(&id)
    }

    /// The single edit equivalent to this one followed by `next`, or `None`
    /// when `next` does not start where this one ends.
    pub(crate) fn then(
        &self,
        next: &Self,
        same_node: &dyn Fn(&Node, &Node) -> bool,
    ) -> Option<Self> {
        fn join<T: Clone + PartialEq>(
            first: &Option<PartChange<T>>,
            second: &Option<PartChange<T>>,
        ) -> Option<Option<PartChange<T>>> {
            match (first, second) {
                (Some(first), Some(second)) => first.then(second),
                (first, second) => Some(first.clone().or_else(|| second.clone())),
            }
        }
        let mut functions = self.functions.clone();
        for (&id, change) in &next.functions {
            let joined = match self.functions.get(&id) {
                Some(earlier) => {
                    if !same_presence(earlier.after.as_ref(), change.before.as_ref(), same_node) {
                        return None;
                    }
                    PartChange {
                        before: earlier.before.clone(),
                        after: change.after.clone(),
                    }
                }
                None => change.clone(),
            };
            functions.insert(id, joined);
        }
        functions.retain(|_, change| {
            !same_presence(change.before.as_ref(), change.after.as_ref(), same_node)
        });
        Some(Self {
            failure_rules: join(&self.failure_rules, &next.failure_rules)?,
            named_sources: join(&self.named_sources, &next.named_sources)?,
            named_targets: join(&self.named_targets, &next.named_targets)?,
            source_options: join(&self.source_options, &next.source_options)?,
            functions,
        })
    }

    /// Checks that every part `target` replaces still holds its recorded
    /// content. Functions are compared by presence, and also by content
    /// when `same_node` decides body equality.
    pub(crate) fn validate(
        &self,
        project: &Project,
        target: Side,
        same_node: Option<SameNode<'_>>,
    ) -> Result<(), GraphEditError> {
        let replaced = target.opposite();
        let differs = |part| Err(GraphEditError::ProjectPartDiffers { part });
        if let Some(change) = &self.failure_rules
            && project.failure_rules != *change.side(replaced)
        {
            return differs(ProjectPart::FailureRules);
        }
        if let Some(change) = &self.named_sources
            && project.extra_sources != *change.side(replaced)
        {
            return differs(ProjectPart::NamedSources);
        }
        if let Some(change) = &self.named_targets
            && project.extra_targets != *change.side(replaced)
        {
            return differs(ProjectPart::NamedTargets);
        }
        if let Some(change) = &self.source_options
            && project.source_options != *change.side(replaced)
        {
            return differs(ProjectPart::SourceOptions);
        }
        for (&id, change) in &self.functions {
            let current = project.user_functions.get(&id);
            let expected = change.side(replaced).as_ref();
            let matches = match (current, expected, same_node) {
                (Some(current), Some(expected), Some(same_node)) => {
                    same_function(current, expected, same_node)
                }
                (current, expected, _) => current.is_some() == expected.is_some(),
            };
            if !matches {
                return differs(ProjectPart::Function(id));
            }
        }
        Ok(())
    }

    /// Writes `target`'s side of every part; [`Self::validate`] runs first.
    pub(crate) fn write(&self, project: &mut Project, target: Side) {
        if let Some(change) = &self.failure_rules {
            project.failure_rules = change.side(target).clone();
        }
        if let Some(change) = &self.named_sources {
            project.extra_sources = change.side(target).clone();
        }
        if let Some(change) = &self.named_targets {
            project.extra_targets = change.side(target).clone();
        }
        if let Some(change) = &self.source_options {
            project.source_options = change.side(target).clone();
        }
        for (&id, change) in &self.functions {
            match change.side(target) {
                Some(function) => project.user_functions.insert(id, function.clone()),
                None => project.user_functions.remove(&id),
            };
        }
    }
}
