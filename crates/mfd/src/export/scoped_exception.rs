//! A single item-owned exception branch, separate from global failure rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    FormatOptions, IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration,
};

use crate::MfdError;

use super::exception::RenderArgs;
use super::schema::{PortTree, SideFormat, side_format};
use super::source::SourceExports;

const MAX_PROOF_NODES: usize = 4096;
const MAX_PROOF_DEPTH: usize = 128;
const MAX_PROOF_WORK: usize = 100_000;

struct Branch {
    chain: Vec<String>,
    collection: Vec<String>,
    filter: NodeId,
    predicate: NodeId,
    raise: NodeId,
    message: Option<NodeId>,
    target_is_false: bool,
    trigger: Option<u32>,
}

#[derive(Default)]
pub(super) struct Branches {
    branch: Option<Branch>,
    owned: BTreeSet<NodeId>,
}

impl Branches {
    pub(super) fn build(project: &Project) -> Result<Self, MfdError> {
        if project.user_functions.values().any(|function| {
            function
                .body
                .nodes
                .values()
                .any(|node| matches!(node, Node::Raise { .. }))
        }) {
            return Err(unsupported(
                "a user-function Raise has no item-owned native export",
            ));
        }
        let raises = project
            .graph
            .nodes
            .iter()
            .filter_map(|(&id, node)| matches!(node, Node::Raise { .. }).then_some(id))
            .take(2)
            .collect::<Vec<_>>();
        if raises.is_empty() {
            return Ok(Self::default());
        }
        let [raise] = raises.as_slice() else {
            return Err(unsupported("requires exactly one Raise node"));
        };
        if !project.failure_rules.is_empty()
            || !project.extra_sources.is_empty()
            || !project.extra_targets.is_empty()
            || !project.user_functions.is_empty()
        {
            return Err(unsupported(
                "requires one primary boundary pair and no global failures or user functions",
            ));
        }
        for (path, options) in [
            (&project.source_path, &project.source_options),
            (&project.target_path, &project.target_options),
        ] {
            let mut ordinary = options.clone();
            ordinary.xml_document = false;
            if side_format(path, options) != SideFormat::Xml
                || ordinary != FormatOptions::default()
                || path.as_deref().is_some_and(|path| {
                    path.contains("://") || path.contains('*') || path.contains('?')
                })
            {
                return Err(unsupported(
                    "requires ordinary local primary XML boundaries",
                ));
            }
        }
        bounded_shape(project)?;
        let issues = engine::validate(project);
        if !issues.is_empty() {
            return Err(unsupported(&format!(
                "requires a valid project: {issues:?}"
            )));
        }
        let mut owner = None;
        find_owner(
            &project.root,
            &mut Vec::new(),
            &mut owner,
            &mut BTreeSet::new(),
        )?;
        let (chain, scope) =
            owner.ok_or_else(|| unsupported("requires one plain primary-source scope owner"))?;
        if chain.is_empty() {
            return Err(unsupported(
                "requires a repeated group beneath a document root",
            ));
        }
        validate_scopes(&project.root, &chain, &mut Vec::new(), false)?;
        let ScopeIteration::Source(collection) = &scope.iteration else {
            return Err(unsupported("requires ordinary source iteration"));
        };
        validate_collection(&project.source, collection)?;
        validate_collection(&project.target, &chain)?;
        let filter = scope
            .filter
            .ok_or_else(|| unsupported("requires a guard filter"))?;
        let Some(Node::If {
            condition,
            then,
            else_,
        }) = project.graph.nodes.get(&filter)
        else {
            return Err(unsupported(
                "requires an exact If(predicate, Raise, true) guard or inverse",
            ));
        };
        let (true_node, target_is_false) = if then == raise {
            (*else_, true)
        } else if else_ == raise {
            (*then, false)
        } else {
            return Err(unsupported("the sole Raise must be a direct guard branch"));
        };
        if !matches!(
            project.graph.nodes.get(&true_node),
            Some(Node::Const {
                value: Value::Bool(true)
            })
        ) {
            return Err(unsupported(
                "the opposite guard branch must be an exact true constant",
            ));
        }
        let owned = BTreeSet::from([filter, *raise, true_node]);
        if owned.len() != 3 || owned.contains(condition) {
            return Err(unsupported(
                "guard helpers must have distinct identities separate from the predicate",
            ));
        }
        for (&id, node) in &project.graph.nodes {
            for dependency in node.dependencies() {
                if owned.contains(&dependency)
                    && !(id == filter && (dependency == *raise || dependency == true_node))
                {
                    return Err(unsupported(
                        "guard, Raise, and true helpers must have no other graph consumers",
                    ));
                }
            }
        }
        // Prove that no binding, scope control, or other project root consumes
        // an omitted helper. This clone is only an ownership proof, not output.
        let mut ordinary = project.clone();
        scope_at_mut(&mut ordinary.root, &chain).filter = None;
        ordinary.prune_unreachable_nodes();
        if owned.iter().any(|id| ordinary.graph.nodes.contains_key(id)) {
            return Err(unsupported(
                "guard helpers must be exclusive to the exact owner filter",
            ));
        }
        let Some(Node::Raise { message }) = project.graph.nodes.get(raise) else {
            unreachable!("Raise identity was selected from the graph");
        };
        let mut projected = BTreeSet::new();
        validate_expression(project, *condition, collection, &mut projected)?;
        validate_bindings(project, scope, collection, &mut projected)?;
        if let Some(message) = message {
            validate_expression(project, *message, collection, &mut projected)?;
        }
        if PredicateProof::new(project, collection).scalar_type(*condition)?
            != Some(ScalarType::Bool)
        {
            return Err(unsupported(
                "predicate is not proven to return a present boolean on schema-valid XML items",
            ));
        }
        Ok(Self {
            branch: Some(Branch {
                chain,
                collection: collection.clone(),
                filter,
                predicate: *condition,
                raise: *raise,
                message: *message,
                target_is_false,
                trigger: None,
            }),
            owned,
        })
    }

    pub(super) fn owned_nodes(&self) -> &BTreeSet<NodeId> {
        &self.owned
    }

    pub(super) fn validate_ports(
        &self,
        sources: &SourceExports<'_>,
        target: &PortTree,
    ) -> Result<(), MfdError> {
        if let Some(branch) = &self.branch
            && (sources.key_for_abs(&branch.collection).is_none()
                || target.key_for_abs(&branch.chain).is_none())
        {
            return Err(unsupported(
                "the exact primary collection and target owner must have structural ports",
            ));
        }
        Ok(())
    }

    fn matching(&self, chain: &[String], collection: &[String], filter: NodeId) -> Option<&Branch> {
        self.branch.as_ref().filter(|branch| {
            branch.chain.as_slice() == chain
                && branch.collection.as_slice() == collection
                && branch.filter == filter
        })
    }

    pub(super) fn predicate(
        &self,
        chain: &[String],
        collection: &[String],
        filter: NodeId,
    ) -> Option<NodeId> {
        self.matching(chain, collection, filter)
            .map(|branch| branch.predicate)
    }

    pub(super) fn target_is_false(
        &self,
        chain: &[String],
        collection: &[String],
        filter: NodeId,
    ) -> bool {
        self.matching(chain, collection, filter)
            .is_some_and(|branch| branch.target_is_false)
    }

    pub(super) fn message(
        &self,
        chain: &[String],
        collection: &[String],
        filter: NodeId,
    ) -> Option<NodeId> {
        self.matching(chain, collection, filter)
            .and_then(|branch| branch.message)
    }

    pub(super) fn claim(
        &mut self,
        chain: &[String],
        collection: &[String],
        filter: NodeId,
        trigger: u32,
    ) {
        if let Some(branch) = &mut self.branch
            && branch.chain.as_slice() == chain
            && branch.collection.as_slice() == collection
            && branch.filter == filter
        {
            branch.trigger = Some(trigger);
        }
    }

    pub(super) fn render(&self, args: RenderArgs<'_>) -> Result<(), MfdError> {
        let Some(branch) = &self.branch else {
            return Ok(());
        };
        let RenderArgs {
            graph,
            node_out_key,
            position_contexts,
            keys,
            uid,
            components,
            edges,
        } = args;
        for position in super::position::position_nodes_for_roots(
            std::iter::once(branch.predicate).chain(branch.message),
            graph,
        ) {
            if !matches!(position_contexts.get(&position), Some(Some(_))) {
                return Err(unsupported(&format!(
                    "Raise node {} position node {position} has no unambiguous raw item context",
                    branch.raise
                )));
            }
        }
        let trigger = branch
            .trigger
            .ok_or_else(|| unsupported("could not claim the exact owner filter branch"))?;
        let message = branch
            .message
            .map(|message| {
                node_out_key.get(&message).copied().ok_or_else(|| {
                    unsupported(&format!(
                        "Raise message node {message} has no native output"
                    ))
                })
            })
            .transpose()?;
        let trigger_input = keys.next();
        let message_input = keys.next();
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"exception\" library=\"core\" uid=\"{uid}\" kind=\"18\">\n\
             \t\t\t\t\t<properties/>\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{trigger_input}\"/><datapoint pos=\"1\" key=\"{message_input}\"/></sources>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t\t<data><wsdl/><exception/></data>\n\
             \t\t\t\t</component>\n"
        );
        edges.push((trigger, trigger_input));
        if let Some(message) = message {
            edges.push((message, message_input));
        }
        Ok(())
    }
}

fn unsupported(reason: &str) -> MfdError {
    MfdError::Unsupported(format!("item-owned exception {reason}"))
}

fn plain(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && scope.iteration_output == IterationOutput::Repeated
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn find_owner<'a>(
    scope: &'a Scope,
    chain: &mut Vec<String>,
    owner: &mut Option<(Vec<String>, &'a Scope)>,
    seen: &mut BTreeSet<Vec<String>>,
) -> Result<(), MfdError> {
    if !seen.insert(chain.clone()) {
        return Err(unsupported("requires unique target scope paths"));
    }
    if !plain(scope) {
        return Err(unsupported(
            "requires plain constructed scopes without controls",
        ));
    }
    match &scope.iteration {
        ScopeIteration::None => {}
        ScopeIteration::Source(_) if owner.is_none() => {
            *owner = Some((chain.clone(), scope));
        }
        _ => {
            return Err(unsupported(
                "requires exactly one source iteration, without sequences, joins, or document iteration",
            ));
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        find_owner(child, chain, owner, seen)?;
        chain.pop();
    }
    Ok(())
}

fn validate_scopes(
    scope: &Scope,
    owner: &[String],
    chain: &mut Vec<String>,
    in_owner: bool,
) -> Result<(), MfdError> {
    let at_owner = chain.as_slice() == owner;
    if (!at_owner && scope.filter.is_some())
        || (!in_owner && !at_owner && !scope.bindings.is_empty())
    {
        return Err(unsupported(
            "only the owner may filter and evaluate target bindings",
        ));
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        validate_scopes(child, owner, chain, in_owner || at_owner)?;
        chain.pop();
    }
    Ok(())
}

fn scope_at_mut<'a>(scope: &'a mut Scope, chain: &[String]) -> &'a mut Scope {
    let Some((first, rest)) = chain.split_first() else {
        return scope;
    };
    let child = scope
        .children
        .iter_mut()
        .find(|child| &child.target_field == first)
        .expect("validated owner chain");
    scope_at_mut(child, rest)
}

fn validate_collection(root: &SchemaNode, path: &[String]) -> Result<(), MfdError> {
    if path.is_empty() || root.repeating || !matches!(root.kind, SchemaKind::Group { .. }) {
        return Err(unsupported(
            "requires an exact group path beneath a non-repeating XML root",
        ));
    }
    let mut node = root;
    for (index, segment) in path.iter().enumerate() {
        node = node
            .child(segment)
            .ok_or_else(|| unsupported("the owner schema path does not exist"))?;
        if !matches!(node.kind, SchemaKind::Group { .. })
            || (index + 1 != path.len() && node.repeating)
        {
            return Err(unsupported(
                "the owner must have non-repeating group ancestors",
            ));
        }
    }
    if !node.repeating {
        return Err(unsupported("the owner must be a repeating group"));
    }
    Ok(())
}

fn validate_bindings(
    project: &Project,
    scope: &Scope,
    collection: &[String],
    projected: &mut BTreeSet<NodeId>,
) -> Result<(), MfdError> {
    for binding in &scope.bindings {
        validate_expression(project, binding.node, collection, projected)?;
    }
    for child in &scope.children {
        validate_bindings(project, child, collection, projected)?;
    }
    Ok(())
}

fn scalar_item_path(project: &Project, collection: &[String], absolute: &[String]) -> bool {
    let Some(suffix) = absolute.strip_prefix(collection) else {
        return false;
    };
    let Some(mut schema) = collection
        .iter()
        .try_fold(&project.source, |schema, segment| schema.child(segment))
    else {
        return false;
    };
    for segment in suffix {
        let Some(child) = schema.child(segment) else {
            return false;
        };
        if child.repeating {
            return false;
        }
        schema = child;
    }
    matches!(schema.kind, SchemaKind::Scalar { .. })
}

fn validate_expression(
    project: &Project,
    id: NodeId,
    collection: &[String],
    seen: &mut BTreeSet<NodeId>,
) -> Result<(), MfdError> {
    if !seen.insert(id) {
        return Ok(());
    }
    let node = project
        .graph
        .nodes
        .get(&id)
        .ok_or_else(|| unsupported("guard expression references a missing node"))?;
    match node {
        Node::Const { .. } | Node::Call { .. } | Node::If { .. } => {}
        Node::SourceField { path, frame } => {
            let mut absolute = frame.clone().unwrap_or_default();
            absolute.extend(path.iter().cloned());
            if frame.as_deref().is_some_and(|frame| frame != collection)
                || !absolute.starts_with(collection)
                || !scalar_item_path(project, collection, &absolute)
            {
                return Err(unsupported(
                    "item expressions must resolve to scalar leaves of the exact primary item without nested repetition",
                ));
            }
        }
        Node::Position {
            collection: position_collection,
        } if position_collection.is_empty() || position_collection.as_slice() == collection => {}
        _ => {
            return Err(unsupported(
                "item expressions support only scalar fields, constants, builtin calls, lazy If, and owner positions",
            ));
        }
    }
    for dependency in node.dependencies() {
        validate_expression(project, dependency, collection, seen)?;
    }
    Ok(())
}

// This bounds the additional exception proof before recursive validation or
// ownership snapshots. It does not bound string bytes or the whole exporter.
fn bounded_shape(project: &Project) -> Result<(), MfdError> {
    let reject = || unsupported("exceeds the bounded closed scalar proof shape");
    if project.graph.nodes.len() > MAX_PROOF_NODES {
        return Err(reject());
    }
    for root in [&project.source, &project.target] {
        let mut pending = vec![(root, 1usize)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            if count > MAX_PROOF_NODES
                || depth > MAX_PROOF_DEPTH
                || node.recursive_ref.is_some()
                || node.name == "element()"
                || node.name == "attribute()"
                || node.xml_wildcard_namespace.is_some()
                || !node.xml_name_alternatives.is_empty()
                || !node.xml_repeating_choices.is_empty()
                || !node.xml_repeating_sequences.is_empty()
                || node.xml_type_alternatives
                || node.json_any
                || node.container_nullable
                || node.json_contains.is_some()
                || node.json_dependent_schemas.is_some()
            {
                return Err(reject());
            }
            match &node.kind {
                SchemaKind::Scalar { .. } => {}
                SchemaKind::ScalarUnion { .. } => return Err(reject()),
                SchemaKind::Group {
                    children,
                    alternatives,
                    required,
                    xml_restricted_alternatives,
                    dynamic,
                } => {
                    // In a closed group, valid required names are unique declared
                    // children. Bound this public vector before schema validation
                    // can clone it while checking property-count metadata.
                    if !alternatives.is_empty()
                        || dynamic.is_some()
                        || required.len() > children.len()
                        || !xml_restricted_alternatives.is_empty()
                    {
                        return Err(reject());
                    }
                    for child in children {
                        if count + pending.len() >= MAX_PROOF_NODES {
                            return Err(reject());
                        }
                        pending.push((child, depth + 1));
                    }
                }
            }
        }
    }
    let mut pending = vec![(&project.root, 1usize)];
    let mut count = 0;
    let mut work = 0usize;
    while let Some((scope, depth)) = pending.pop() {
        count += 1;
        if count > MAX_PROOF_NODES
            || depth > MAX_PROOF_DEPTH
            || !plain(scope)
            || !matches!(
                scope.iteration,
                ScopeIteration::None | ScopeIteration::Source(_)
            )
        {
            return Err(reject());
        }
        if let ScopeIteration::Source(path) = &scope.iteration
            && path.len() > MAX_PROOF_DEPTH
        {
            return Err(reject());
        }
        work = work
            .checked_add(scope.bindings.len())
            .filter(|work| *work <= MAX_PROOF_WORK)
            .ok_or_else(reject)?;
        for child in &scope.children {
            if count + pending.len() >= MAX_PROOF_NODES {
                return Err(reject());
            }
            pending.push((child, depth + 1));
        }
    }
    work = work
        .checked_add(project.graph.nodes.len())
        .filter(|work| *work <= MAX_PROOF_WORK)
        .ok_or_else(reject)?;
    for node in project.graph.nodes.values() {
        let edges = match node {
            Node::Const { .. } => 0,
            Node::SourceField { path, frame } => {
                if path.len() > MAX_PROOF_DEPTH
                    || frame
                        .as_ref()
                        .is_some_and(|frame| frame.len() > MAX_PROOF_DEPTH)
                {
                    return Err(reject());
                }
                0
            }
            Node::Position { collection } => {
                if collection.len() > MAX_PROOF_DEPTH {
                    return Err(reject());
                }
                0
            }
            Node::Call { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::Raise { message } => usize::from(message.is_some()),
            _ => return Err(reject()),
        };
        work = work
            .checked_add(edges)
            .filter(|work| *work <= MAX_PROOF_WORK)
            .ok_or_else(reject)?;
    }
    // Memoize logical height, not only the current DFS stack: a warm child
    // still contributes its complete height to its next parent.
    let mut heights = BTreeMap::new();
    let mut active = BTreeSet::new();
    for id in project.graph.nodes.keys() {
        expression_height(*id, &project.graph, &mut heights, &mut active)?;
    }
    Ok(())
}

fn expression_height(
    id: NodeId,
    graph: &mapping::Graph,
    heights: &mut BTreeMap<NodeId, usize>,
    active: &mut BTreeSet<NodeId>,
) -> Result<usize, MfdError> {
    if let Some(height) = heights.get(&id) {
        return Ok(*height);
    }
    if active.len() >= MAX_PROOF_DEPTH || !active.insert(id) {
        return Err(unsupported(
            "exceeds the bounded scalar dependency depth or contains a cycle",
        ));
    }
    let node = graph
        .nodes
        .get(&id)
        .ok_or_else(|| unsupported("references a missing scalar dependency"))?;
    let mut height = 1usize;
    for dependency in node.dependencies() {
        let child = expression_height(dependency, graph, heights, active)?;
        height = height.max(child + 1);
        if height > MAX_PROOF_DEPTH {
            return Err(unsupported("exceeds the bounded scalar dependency depth"));
        }
    }
    active.remove(&id);
    heights.insert(id, height);
    Ok(height)
}

// This is an admission proof, never an evaluator or scalar coercion. It runs
// after bounded_shape and exact item projection. Unknown computed predicates
// remain unsupported; messages and target bindings keep their existing rules.
struct PredicateProof<'a> {
    project: &'a Project,
    collection: &'a [String],
    memo: BTreeMap<NodeId, Option<ScalarType>>,
    direct: BTreeMap<NodeId, Option<ScalarType>>,
    work: usize,
}

impl<'a> PredicateProof<'a> {
    fn new(project: &'a Project, collection: &'a [String]) -> Self {
        Self {
            project,
            collection,
            memo: BTreeMap::new(),
            direct: BTreeMap::new(),
            work: 0,
        }
    }

    fn charge(&mut self, work: usize) -> Result<(), MfdError> {
        self.work = self
            .work
            .checked_add(work)
            .filter(|work| *work <= MAX_PROOF_WORK)
            .ok_or_else(|| unsupported("predicate proof exceeds the bounded scalar work budget"))?;
        Ok(())
    }

    fn scalar_type(&mut self, id: NodeId) -> Result<Option<ScalarType>, MfdError> {
        if let Some(result) = self.memo.get(&id) {
            return Ok(*result);
        }
        self.charge(1)?;
        let project = self.project;
        let node = project
            .graph
            .nodes
            .get(&id)
            .ok_or_else(|| unsupported("predicate references a missing node"))?;
        let result = match node {
            Node::Const { .. } | Node::SourceField { .. } | Node::Position { .. } => {
                self.direct_type(id)?
            }
            Node::If {
                condition,
                then,
                else_,
            } => {
                let condition = self.scalar_type(*condition)?;
                let then = self.scalar_type(*then)?;
                let else_ = self.scalar_type(*else_)?;
                (condition == Some(ScalarType::Bool)
                    && then == Some(ScalarType::Bool)
                    && else_ == Some(ScalarType::Bool))
                .then_some(ScalarType::Bool)
            }
            Node::Call { function, args } => match (function.as_str(), args.as_slice()) {
                ("not", [value]) => (self.scalar_type(*value)? == Some(ScalarType::Bool))
                    .then_some(ScalarType::Bool),
                ("and" | "or", [left, right]) => {
                    let left = self.scalar_type(*left)?;
                    let right = self.scalar_type(*right)?;
                    (left == Some(ScalarType::Bool) && right == Some(ScalarType::Bool))
                        .then_some(ScalarType::Bool)
                }
                (
                    "equal" | "not_equal" | "less_than" | "greater_than" | "less_or_equal"
                    | "greater_or_equal",
                    [left, right],
                ) => {
                    // Comparisons intentionally admit direct present leaves
                    // only, excluding arithmetic/coercion and mixed domains.
                    let left = self.direct_type(*left)?;
                    let right = self.direct_type(*right)?;
                    (left.is_some() && left == right).then_some(ScalarType::Bool)
                }
                _ => None,
            },
            _ => None,
        };
        self.memo.insert(id, result);
        Ok(result)
    }

    fn direct_type(&mut self, id: NodeId) -> Result<Option<ScalarType>, MfdError> {
        if let Some(result) = self.direct.get(&id) {
            return Ok(*result);
        }
        self.charge(1)?;
        let project = self.project;
        let node = project
            .graph
            .nodes
            .get(&id)
            .ok_or_else(|| unsupported("predicate references a missing scalar leaf"))?;
        let result = match node {
            Node::Const {
                value: Value::Bool(_),
            } => Some(ScalarType::Bool),
            Node::Const {
                value: Value::Int(_),
            } => Some(ScalarType::Int),
            Node::Const {
                value: Value::String(_),
            } => Some(ScalarType::String),
            Node::Position { collection }
                if collection.is_empty() || collection.as_slice() == self.collection =>
            {
                Some(ScalarType::Int)
            }
            Node::SourceField { path, frame } => {
                self.charge(path.len() + frame.as_ref().map_or(0, Vec::len))?;
                self.present_item_scalar(path, frame.as_deref())?
            }
            _ => None,
        };
        self.direct.insert(id, result);
        Ok(result)
    }

    fn present_item_scalar(
        &mut self,
        path: &[String],
        frame: Option<&[String]>,
    ) -> Result<Option<ScalarType>, MfdError> {
        let mut absolute = frame.unwrap_or_default().to_vec();
        absolute.extend(path.iter().cloned());
        if frame.is_some_and(|frame| frame != self.collection)
            || !absolute.starts_with(self.collection)
        {
            return Ok(None);
        }
        let project = self.project;
        let mut schema = &project.source;
        if !required_xml_value(schema) {
            return Ok(None);
        }
        for (index, segment) in absolute.iter().enumerate() {
            let SchemaKind::Group { children, .. } = &schema.kind else {
                return Ok(None);
            };
            let mut found = None;
            for child in children {
                self.charge(1)?;
                if child.name == *segment {
                    found = Some(child);
                    break;
                }
            }
            let Some(child) = found else {
                return Ok(None);
            };
            if !required_xml_value(child) || child.repeating && index + 1 != self.collection.len() {
                return Ok(None);
            }
            schema = child;
        }
        Ok(match &schema.kind {
            SchemaKind::Scalar {
                ty: ScalarType::Bool,
            } => Some(ScalarType::Bool),
            SchemaKind::Scalar {
                ty: ScalarType::Int,
            } => Some(ScalarType::Int),
            SchemaKind::Scalar {
                ty: ScalarType::String,
            } => Some(ScalarType::String),
            _ => None,
        })
    }
}

fn required_xml_value(node: &SchemaNode) -> bool {
    !node.nillable && !node.xml_optional && (!node.attribute || node.xml_attribute_required)
}
