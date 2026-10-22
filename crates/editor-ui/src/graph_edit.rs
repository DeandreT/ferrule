//! Typed, invertible graph edits keyed by node and scope identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mapping::{Binding, FunctionId, Graph, Node, NodeId, Scope};

use crate::{ScopeFields, ScopeSetting, ScopeSettingValue, TargetId, WireSink};

/// The graph and target scope tree that one mapping canvas edits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphDocument {
    /// The shared project graph with one target's root scope.
    Target(TargetId),
    /// One user-function body, which has no target scope.
    Function(FunctionId),
}

/// One step from a scope to a nested scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeStep {
    Child(usize),
    /// One segment of a concatenated iteration.
    Segment(usize),
    DynamicChild(usize),
}

/// A scope addressed structurally from its root scope.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeLocation(Vec<ScopeStep>);

impl ScopeLocation {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn steps(&self) -> &[ScopeStep] {
        &self.0
    }

    /// The scope one step above this one, and that step; `None` at the root.
    pub fn parent(&self) -> Option<(Self, ScopeStep)> {
        let (&last, rest) = self.0.split_last()?;
        Some((Self(rest.to_vec()), last))
    }

    /// Whether this scope lies strictly inside the scope at `ancestor`.
    pub fn is_below(&self, ancestor: &Self) -> bool {
        self.0.len() > ancestor.0.len() && self.0.starts_with(&ancestor.0)
    }

    /// Locates a scope by the indices of the static children leading to it.
    pub fn of_children(path: &[usize]) -> Self {
        Self(path.iter().copied().map(ScopeStep::Child).collect())
    }

    /// Locates the static child scope chain that canvases bind target fields
    /// through: at each step, the first child whose target field matches.
    pub fn of_child_chain(root: &Scope, chain: &[String]) -> Option<Self> {
        let mut scope = root;
        let mut steps = Vec::with_capacity(chain.len());
        for field in chain {
            let index = scope
                .children
                .iter()
                .position(|child| child.target_field == *field)?;
            scope = &scope.children[index];
            steps.push(ScopeStep::Child(index));
        }
        Some(Self(steps))
    }

    pub fn join(&self, step: ScopeStep) -> Self {
        let mut steps = self.0.clone();
        steps.push(step);
        Self(steps)
    }

    pub(crate) fn resolve<'s>(&self, root: &'s Scope) -> Option<&'s Scope> {
        self.0.iter().try_fold(root, |scope, step| match *step {
            ScopeStep::Child(index) => scope.children.get(index),
            ScopeStep::Segment(index) => scope.concatenated()?.iter().nth(index),
            ScopeStep::DynamicChild(index) => {
                scope.dynamic_children.get(index).map(|child| &child.scope)
            }
        })
    }

    fn resolve_mut<'s>(&self, root: &'s mut Scope) -> Option<&'s mut Scope> {
        self.0.iter().try_fold(root, |scope, step| match *step {
            ScopeStep::Child(index) => scope.children.get_mut(index),
            ScopeStep::Segment(index) => scope.concatenated_mut()?.iter_mut().nth(index),
            ScopeStep::DynamicChild(index) => scope
                .dynamic_children
                .get_mut(index)
                .map(|child| &mut child.scope),
        })
    }
}

/// One node identity on both sides of an edit; `None` means absent.
#[derive(Clone, Debug)]
pub struct NodeChange {
    before: Option<Node>,
    after: Option<Node>,
}

impl NodeChange {
    pub fn before(&self) -> Option<&Node> {
        self.before.as_ref()
    }

    pub fn after(&self) -> Option<&Node> {
        self.after.as_ref()
    }
}

/// The ordered binding list of one scope on both sides of an edit.
#[derive(Clone, Debug)]
pub struct BindingsChange {
    before: Vec<Binding>,
    after: Vec<Binding>,
}

impl BindingsChange {
    pub fn before(&self) -> &[Binding] {
        &self.before
    }

    pub fn after(&self) -> &[Binding] {
        &self.after
    }
}

/// The ordered static child scopes of one scope on both sides of an edit
/// that adds, removes, or restructures them.
#[derive(Clone, Debug)]
pub struct ChildrenChange {
    before: Vec<Scope>,
    after: Vec<Scope>,
}

impl ChildrenChange {
    pub fn before(&self) -> &[Scope] {
        &self.before
    }

    pub fn after(&self) -> &[Scope] {
        &self.after
    }
}

/// One setting of one scope on both sides of an edit.
#[derive(Clone, Debug)]
pub struct SettingChange {
    before: ScopeSettingValue,
    after: ScopeSettingValue,
}

impl SettingChange {
    pub fn before(&self) -> &ScopeSettingValue {
        &self.before
    }

    pub fn after(&self) -> &ScopeSettingValue {
        &self.after
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphEditError {
    /// The scope trees differ in shape, which a binding patch cannot express.
    ScopeShapeChanged {
        location: ScopeLocation,
    },
    /// The edited target or function no longer exists.
    MissingDocument(GraphDocument),
    /// A node's presence differs from the state the edit replaces.
    NodePresence {
        node: NodeId,
        expected: bool,
    },
    /// A node's content differs from the state the edit replaces.
    NodeDiffers {
        node: NodeId,
    },
    MissingScope {
        location: ScopeLocation,
    },
    /// A scope's bindings differ from the state the edit replaces.
    BindingsDiffer {
        location: ScopeLocation,
    },
    /// Binding changes were addressed to a document without a target scope.
    NoTargetScope,
    /// A requested node is not removed by the edit.
    NotRemoved {
        node: NodeId,
    },
    EmptyRemoval,
    /// The edit does not connect `source` into `sink`.
    NotConnected {
        source: NodeId,
        sink: WireSink,
    },
    /// The edit does not disconnect `sink`.
    NotDisconnected {
        sink: WireSink,
    },
    /// The edit does not add `node`.
    NotInserted {
        node: NodeId,
    },
    /// A node insertion also changes or removes existing identities.
    InsertionNotAdditive,
    /// A scope setting differs from the state the edit replaces.
    SettingDiffers {
        location: ScopeLocation,
        setting: ScopeSetting,
    },
    /// The edit does not change the properties of `node`.
    NodeNotEdited {
        node: NodeId,
    },
    /// The edit does not change the bindings of one scope as its action
    /// describes.
    BindingNotChanged {
        location: ScopeLocation,
    },
    /// The edit does not change `setting` of one scope.
    SettingNotChanged {
        location: ScopeLocation,
        setting: ScopeSetting,
    },
    /// The edit also changes identities its action neither names nor
    /// depends on.
    UnrelatedChanges,
    /// A scope's child scopes differ from the state the edit replaces.
    ChildrenDiffer {
        location: ScopeLocation,
    },
    /// A scope setting change lies inside child scopes the edit replaces.
    SettingInsideReplacedScopes {
        location: ScopeLocation,
    },
    /// The edit does not add, expand, or remove child scopes at one scope as
    /// its action describes.
    ScopeTreeNotChanged {
        location: ScopeLocation,
    },
    /// The edit does not change the iteration of one scope as its action
    /// describes.
    IterationNotChanged {
        location: ScopeLocation,
    },
    /// The edit does not change the computed properties of one scope as its
    /// action describes.
    ComputedPropertiesNotChanged {
        location: ScopeLocation,
    },
    /// The edit does not set or clear the XML type of one scope as its
    /// action describes.
    XmlTypeNotChanged {
        location: ScopeLocation,
    },
    /// The edit does not add `duplicate` beside an unchanged original.
    NotDuplicated {
        duplicate: NodeId,
    },
}

impl fmt::Display for GraphEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScopeShapeChanged { location } => {
                write!(f, "scope {location:?} changed shape during a graph edit")
            }
            Self::MissingDocument(document) => write!(f, "{document:?} no longer exists"),
            Self::NodePresence { node, expected } => {
                let state = if *expected { "present" } else { "absent" };
                write!(f, "mapping node {node} is not {state} as the edit requires")
            }
            Self::NodeDiffers { node } => write!(f, "mapping node {node} changed since the edit"),
            Self::MissingScope { location } => write!(f, "scope {location:?} no longer exists"),
            Self::BindingsDiffer { location } => {
                write!(f, "bindings of scope {location:?} changed since the edit")
            }
            Self::NoTargetScope => f.write_str("function graphs have no target bindings"),
            Self::NotRemoved { node } => write!(f, "mapping node {node} is not removed"),
            Self::EmptyRemoval => f.write_str("a node removal must remove at least one node"),
            Self::NotConnected { source, sink } => {
                write!(
                    f,
                    "the edit does not connect mapping node {source} to {sink}"
                )
            }
            Self::NotDisconnected { sink } => write!(f, "the edit does not disconnect {sink}"),
            Self::NotInserted { node } => write!(f, "mapping node {node} is not inserted"),
            Self::InsertionNotAdditive => {
                f.write_str("a node insertion must not change existing identities")
            }
            Self::SettingDiffers { location, setting } => {
                write!(
                    f,
                    "{setting:?} of scope {location:?} changed since the edit"
                )
            }
            Self::NodeNotEdited { node } => {
                write!(f, "the edit does not change mapping node {node}")
            }
            Self::BindingNotChanged { location } => {
                write!(
                    f,
                    "the edit does not change the bindings of scope {location:?} as described"
                )
            }
            Self::SettingNotChanged { location, setting } => {
                write!(
                    f,
                    "the edit does not change {setting:?} of scope {location:?}"
                )
            }
            Self::UnrelatedChanges => {
                f.write_str("the edit changes identities its action does not name")
            }
            Self::ChildrenDiffer { location } => {
                write!(
                    f,
                    "child scopes of scope {location:?} changed since the edit"
                )
            }
            Self::SettingInsideReplacedScopes { location } => {
                write!(
                    f,
                    "scope {location:?} lies inside child scopes the edit replaces"
                )
            }
            Self::ScopeTreeNotChanged { location } => {
                write!(
                    f,
                    "the edit does not change the child scopes of scope {location:?} as described"
                )
            }
            Self::IterationNotChanged { location } => {
                write!(
                    f,
                    "the edit does not change the iteration of scope {location:?} as described"
                )
            }
            Self::ComputedPropertiesNotChanged { location } => {
                write!(
                    f,
                    "the edit does not change the computed properties of scope {location:?} as described"
                )
            }
            Self::XmlTypeNotChanged { location } => {
                write!(
                    f,
                    "the edit does not change the XML type of scope {location:?} as described"
                )
            }
            Self::NotDuplicated { duplicate } => {
                write!(
                    f,
                    "the edit does not add duplicate mapping node {duplicate}"
                )
            }
        }
    }
}

impl std::error::Error for GraphEditError {}

/// Node, binding, scope-setting, and child-scope differences of one graph
/// edit, keyed by identity.
///
/// Applying or reverting validates every affected identity before changing
/// anything, so a stale patch fails without partial effects.
#[derive(Clone, Debug, Default)]
pub struct GraphPatch {
    nodes: BTreeMap<NodeId, NodeChange>,
    bindings: BTreeMap<ScopeLocation, BindingsChange>,
    settings: BTreeMap<(ScopeLocation, ScopeSetting), SettingChange>,
    /// Never nested: no key lies inside another, and no binding or setting
    /// key lies inside a key of this map.
    children: BTreeMap<ScopeLocation, ChildrenChange>,
}

impl GraphPatch {
    /// Captures node and binding differences between two states of one graph
    /// and scope tree. `same_node` decides node content equality; scope trees
    /// must keep their shape. Other scope fields are not captured, so hosts
    /// that derive a patch from arbitrary edits must verify that applying it
    /// reproduces the edited state.
    pub fn between(
        before: (&Graph, &Scope),
        after: (&Graph, &Scope),
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<Self, GraphEditError> {
        Self::diff(before, after, same_node, None)
    }

    /// Like [`Self::between`], but a scope whose static child scopes differ
    /// in number or target fields has its complete child list captured
    /// instead of failing; nothing inside a captured list is diffed.
    pub fn between_reshaped(
        before: (&Graph, &Scope),
        after: (&Graph, &Scope),
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<Self, GraphEditError> {
        let mut children = BTreeMap::new();
        let mut patch = Self::diff(before, after, same_node, Some(&mut children))?;
        patch.children = children;
        Ok(patch)
    }

    fn diff(
        before: (&Graph, &Scope),
        after: (&Graph, &Scope),
        same_node: impl Fn(&Node, &Node) -> bool,
        children: Option<&mut BTreeMap<ScopeLocation, ChildrenChange>>,
    ) -> Result<Self, GraphEditError> {
        let ids = before
            .0
            .nodes
            .keys()
            .chain(after.0.nodes.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let nodes = ids
            .into_iter()
            .filter_map(|id| {
                let change = NodeChange {
                    before: before.0.nodes.get(&id).cloned(),
                    after: after.0.nodes.get(&id).cloned(),
                };
                let unchanged = match (&change.before, &change.after) {
                    (Some(before), Some(after)) => same_node(before, after),
                    (before, after) => before.is_none() && after.is_none(),
                };
                (!unchanged).then_some((id, change))
            })
            .collect();
        let mut bindings = BTreeMap::new();
        diff_scopes(
            &ScopeLocation::root(),
            before.1,
            after.1,
            &mut bindings,
            children,
        )?;
        Ok(Self {
            nodes,
            bindings,
            ..Self::default()
        })
    }

    /// Adds the setting differences of the scope at `location` between two
    /// captures of its fields. Bindings in the captures are ignored; the
    /// scope must not lie inside child scopes the patch replaces.
    pub fn with_scope_settings(
        mut self,
        location: &ScopeLocation,
        before: &ScopeFields,
        after: &ScopeFields,
    ) -> Result<Self, GraphEditError> {
        if self
            .children
            .keys()
            .any(|replaced| location.is_below(replaced))
        {
            return Err(GraphEditError::SettingInsideReplacedScopes {
                location: location.clone(),
            });
        }
        self.settings
            .extend(Self::setting_changes(location, before, after));
        Ok(self)
    }

    fn setting_changes<'a>(
        location: &'a ScopeLocation,
        before: &'a ScopeFields,
        after: &'a ScopeFields,
    ) -> impl Iterator<Item = ((ScopeLocation, ScopeSetting), SettingChange)> + 'a {
        before.changed_settings(after).map(|(before, after)| {
            (
                (location.clone(), before.setting()),
                SettingChange {
                    before: before.clone(),
                    after: after.clone(),
                },
            )
        })
    }

    /// Captures the binding and setting differences of the scope at
    /// `location` between two captures of its fields. Settings without a
    /// value on either side are not captured, so hosts must verify that
    /// applying the patch reproduces the edited state.
    pub fn of_scope(location: &ScopeLocation, before: &ScopeFields, after: &ScopeFields) -> Self {
        let mut bindings = BTreeMap::new();
        if !same_bindings(before.bindings(), after.bindings()) {
            bindings.insert(
                location.clone(),
                BindingsChange {
                    before: before.bindings().to_vec(),
                    after: after.bindings().to_vec(),
                },
            );
        }
        let settings = Self::setting_changes(location, before, after).collect();
        Self {
            bindings,
            settings,
            ..Self::default()
        }
    }

    /// Splits a node-free patch into one patch per changed binding list and
    /// per changed setting, so each can form its own command. Returns the
    /// patch unchanged as its only part when it changes nodes.
    pub fn into_scope_parts(self) -> Vec<Self> {
        if !self.nodes.is_empty() || !self.children.is_empty() {
            return vec![self];
        }
        let bindings = self.bindings.into_iter().map(|(location, change)| Self {
            bindings: BTreeMap::from([(location, change)]),
            ..Self::default()
        });
        let settings = self.settings.into_iter().map(|(key, change)| Self {
            settings: BTreeMap::from([(key, change)]),
            ..Self::default()
        });
        bindings.chain(settings).collect()
    }

    /// The single edit equivalent to this one followed by `next`, or `None`
    /// when `next` does not start from the state this edit leads to.
    /// Identities the combined edit leaves unchanged are dropped.
    pub fn then(&self, next: &Self, same_node: impl Fn(&Node, &Node) -> bool) -> Option<Self> {
        let same_presence = |left: Option<&Node>, right: Option<&Node>| match (left, right) {
            (Some(left), Some(right)) => same_node(left, right),
            (left, right) => left.is_none() && right.is_none(),
        };
        let nodes = compose(&self.nodes, &next.nodes, |first, second| {
            same_presence(first.after(), second.before()).then(|| NodeChange {
                before: first.before.clone(),
                after: second.after.clone(),
            })
        })?
        .into_iter()
        .filter(|(_, change)| !same_presence(change.before(), change.after()))
        .collect();
        let bindings = compose(&self.bindings, &next.bindings, |first, second| {
            same_bindings(&first.after, &second.before).then(|| BindingsChange {
                before: first.before.clone(),
                after: second.after.clone(),
            })
        })?
        .into_iter()
        .filter(|(_, change)| !same_bindings(&change.before, &change.after))
        .collect();
        let settings = compose(&self.settings, &next.settings, |first, second| {
            (first.after == second.before).then(|| SettingChange {
                before: first.before.clone(),
                after: second.after.clone(),
            })
        })?
        .into_iter()
        .filter(|(_, change)| change.before != change.after)
        .collect();
        let children = compose(&self.children, &next.children, |first, second| {
            (first.after == second.before).then(|| ChildrenChange {
                before: first.before.clone(),
                after: second.after.clone(),
            })
        })?
        .into_iter()
        .filter(|(_, change)| change.before != change.after)
        .collect();
        Some(Self {
            nodes,
            bindings,
            settings,
            children,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.bindings.is_empty()
            && self.settings.is_empty()
            && self.children.is_empty()
    }

    /// Every node identity the edit adds, removes, or changes.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &NodeChange)> {
        self.nodes.iter().map(|(&id, change)| (id, change))
    }

    pub fn removed_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_some() && change.after.is_none())
    }

    pub fn added_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_none() && change.after.is_some())
    }

    pub fn changed_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes_where(|change| change.before.is_some() && change.after.is_some())
    }

    /// Every scope whose binding list the edit replaces.
    pub fn scopes(&self) -> impl Iterator<Item = (&ScopeLocation, &BindingsChange)> {
        self.bindings.iter()
    }

    /// The change of one node identity, if the edit affects it.
    pub fn node(&self, id: NodeId) -> Option<&NodeChange> {
        self.nodes.get(&id)
    }

    /// The binding change of one scope, if the edit affects it.
    pub fn scope(&self, location: &ScopeLocation) -> Option<&BindingsChange> {
        self.bindings.get(location)
    }

    /// Every scope setting the edit replaces.
    pub fn settings(&self) -> impl Iterator<Item = (&ScopeLocation, ScopeSetting, &SettingChange)> {
        self.settings
            .iter()
            .map(|((location, setting), change)| (location, *setting, change))
    }

    /// Every scope whose child scopes the edit replaces.
    pub fn children(&self) -> impl Iterator<Item = (&ScopeLocation, &ChildrenChange)> {
        self.children.iter()
    }

    /// The child-scope change of one scope, if the edit affects it.
    pub fn children_of(&self, location: &ScopeLocation) -> Option<&ChildrenChange> {
        self.children.get(location)
    }

    /// Whether the edit changes nothing besides nodes.
    pub(crate) fn only_nodes(&self) -> bool {
        self.bindings.is_empty() && self.settings.is_empty() && self.children.is_empty()
    }

    /// The change of one scope setting, if the edit affects it.
    pub fn setting(
        &self,
        location: &ScopeLocation,
        setting: ScopeSetting,
    ) -> Option<&SettingChange> {
        self.settings.get(&(location.clone(), setting))
    }

    /// Applies the edit, validating node presence and scope bindings.
    pub fn apply_to(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::After, &|_, _| true)
    }

    /// Reverts the edit, validating node presence and scope bindings.
    pub fn revert_on(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::Before, &|_, _| true)
    }

    /// Like [`Self::apply_to`], and also requires every present node the
    /// edit replaces to match its recorded content under `same_node`.
    pub fn apply_to_matching(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::After, &same_node)
    }

    /// Like [`Self::revert_on`], with the content check of
    /// [`Self::apply_to_matching`].
    pub fn revert_on_matching(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
        same_node: impl Fn(&Node, &Node) -> bool,
    ) -> Result<(), GraphEditError> {
        self.exchange(graph, scope, Side::Before, &same_node)
    }

    fn nodes_where(&self, keep: fn(&NodeChange) -> bool) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes
            .iter()
            .filter(move |(_, change)| keep(change))
            .map(|(&id, _)| id)
    }

    fn exchange(
        &self,
        graph: &mut Graph,
        scope: Option<&mut Scope>,
        target: Side,
        same_node: &dyn Fn(&Node, &Node) -> bool,
    ) -> Result<(), GraphEditError> {
        for (&node, change) in &self.nodes {
            let replaced = target.replaced_node(change);
            match (graph.nodes.get(&node), replaced) {
                (Some(current), Some(replaced)) if !same_node(current, replaced) => {
                    return Err(GraphEditError::NodeDiffers { node });
                }
                (current, replaced) if current.is_some() != replaced.is_some() => {
                    return Err(GraphEditError::NodePresence {
                        node,
                        expected: replaced.is_some(),
                    });
                }
                _ => {}
            }
        }
        if !self.bindings.is_empty() || !self.settings.is_empty() || !self.children.is_empty() {
            let root = scope.as_deref().ok_or(GraphEditError::NoTargetScope)?;
            let resolve = |location: &ScopeLocation| {
                location
                    .resolve(root)
                    .ok_or_else(|| GraphEditError::MissingScope {
                        location: location.clone(),
                    })
            };
            for (location, change) in &self.bindings {
                if !same_bindings(
                    &resolve(location)?.bindings,
                    target.replaced_bindings(change),
                ) {
                    return Err(GraphEditError::BindingsDiffer {
                        location: location.clone(),
                    });
                }
            }
            for ((location, setting), change) in &self.settings {
                let current = ScopeSettingValue::read(*setting, resolve(location)?);
                if current.as_ref() != Some(target.replaced_setting(change)) {
                    return Err(GraphEditError::SettingDiffers {
                        location: location.clone(),
                        setting: *setting,
                    });
                }
            }
            for (location, change) in &self.children {
                if resolve(location)?.children != target.replaced_children(change) {
                    return Err(GraphEditError::ChildrenDiffer {
                        location: location.clone(),
                    });
                }
            }
        }

        // Every identity was validated above; the mutations below cannot fail.
        for (&node, change) in &self.nodes {
            match target.node(change) {
                Some(content) => graph.nodes.insert(node, content.clone()),
                None => graph.nodes.remove(&node),
            };
        }
        if let Some(root) = scope {
            for (location, change) in &self.bindings {
                if let Some(current) = location.resolve_mut(root) {
                    current.bindings = target.bindings(change).to_vec();
                }
            }
            for ((location, _), change) in &self.settings {
                if let Some(current) = location.resolve_mut(root) {
                    target.setting(change).write(current);
                }
            }
            // No binding or setting key lies inside a replaced child list.
            for (location, change) in &self.children {
                if let Some(current) = location.resolve_mut(root) {
                    current.children = target.children(change).to_vec();
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Side {
    Before,
    After,
}

impl Side {
    fn node(self, change: &NodeChange) -> Option<&Node> {
        match self {
            Self::Before => change.before.as_ref(),
            Self::After => change.after.as_ref(),
        }
    }

    fn replaced_node(self, change: &NodeChange) -> Option<&Node> {
        self.opposite().node(change)
    }

    fn bindings(self, change: &BindingsChange) -> &[Binding] {
        match self {
            Self::Before => &change.before,
            Self::After => &change.after,
        }
    }

    fn replaced_bindings(self, change: &BindingsChange) -> &[Binding] {
        self.opposite().bindings(change)
    }

    fn setting(self, change: &SettingChange) -> &ScopeSettingValue {
        match self {
            Self::Before => &change.before,
            Self::After => &change.after,
        }
    }

    fn replaced_setting(self, change: &SettingChange) -> &ScopeSettingValue {
        self.opposite().setting(change)
    }

    fn children(self, change: &ChildrenChange) -> &[Scope] {
        match self {
            Self::Before => &change.before,
            Self::After => &change.after,
        }
    }

    fn replaced_children(self, change: &ChildrenChange) -> &[Scope] {
        self.opposite().children(change)
    }

    fn opposite(self) -> Self {
        match self {
            Self::Before => Self::After,
            Self::After => Self::Before,
        }
    }
}

/// Composes two keyed change maps; `join` combines changes of one key made
/// by both, or rejects them when the second does not start where the first
/// ends.
fn compose<K: Ord + Clone, V: Clone>(
    first: &BTreeMap<K, V>,
    second: &BTreeMap<K, V>,
    join: impl Fn(&V, &V) -> Option<V>,
) -> Option<BTreeMap<K, V>> {
    let mut composed = first.clone();
    for (key, change) in second {
        let joined = match first.get(key) {
            Some(earlier) => join(earlier, change)?,
            None => change.clone(),
        };
        composed.insert(key.clone(), joined);
    }
    Some(composed)
}

pub(crate) fn same_bindings(left: &[Binding], right: &[Binding]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.target_field == right.target_field && left.node == right.node)
}

fn diff_scopes(
    location: &ScopeLocation,
    before: &Scope,
    after: &Scope,
    changes: &mut BTreeMap<ScopeLocation, BindingsChange>,
    mut reshaped: Option<&mut BTreeMap<ScopeLocation, ChildrenChange>>,
) -> Result<(), GraphEditError> {
    if !same_bindings(&before.bindings, &after.bindings) {
        changes.insert(
            location.clone(),
            BindingsChange {
                before: before.bindings.clone(),
                after: after.bindings.clone(),
            },
        );
    }
    let shape_changed = || GraphEditError::ScopeShapeChanged {
        location: location.clone(),
    };
    if before.dynamic_children.len() != after.dynamic_children.len() {
        return Err(shape_changed());
    }
    let children_reshaped = before.children.len() != after.children.len()
        || before
            .children
            .iter()
            .zip(&after.children)
            .any(|(before, after)| before.target_field != after.target_field);
    match reshaped.as_deref_mut() {
        Some(reshaped) if children_reshaped => {
            reshaped.insert(
                location.clone(),
                ChildrenChange {
                    before: before.children.clone(),
                    after: after.children.clone(),
                },
            );
        }
        None if before.children.len() != after.children.len() => return Err(shape_changed()),
        _ => {
            for (index, (before, after)) in before.children.iter().zip(&after.children).enumerate()
            {
                diff_scopes(
                    &location.join(ScopeStep::Child(index)),
                    before,
                    after,
                    changes,
                    reshaped.as_deref_mut(),
                )?;
            }
        }
    }
    match (before.concatenated(), after.concatenated()) {
        (None, None) => {}
        (Some(before), Some(after)) if before.len() == after.len() => {
            for (index, (before, after)) in before.iter().zip(after.iter()).enumerate() {
                diff_scopes(
                    &location.join(ScopeStep::Segment(index)),
                    before,
                    after,
                    changes,
                    reshaped.as_deref_mut(),
                )?;
            }
        }
        _ => return Err(shape_changed()),
    }
    for (index, (before, after)) in before
        .dynamic_children
        .iter()
        .zip(&after.dynamic_children)
        .enumerate()
    {
        diff_scopes(
            &location.join(ScopeStep::DynamicChild(index)),
            &before.scope,
            &after.scope,
            changes,
            reshaped.as_deref_mut(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "graph_edit_tests.rs"]
mod tests;
