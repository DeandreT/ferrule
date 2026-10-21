//! Conditional scalar presence drivers for declared target clones.
//!
//! A declared clone whose own entry is fed by a core `if-else` constructs one
//! group when the lazily selected scalar exists and none when it is absent.
//! The group stays in the plain enclosing source frame: it lowers to
//! [`ScopeIteration::OptionalPresence`] over `exists(selected)`, never to a
//! source iteration. Every proof here reads only; lowering runs against cloned
//! import state that is committed whole or discarded with one diagnostic.

use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, Value};
use mapping::{IterationOutput, Node, NodeId, ScopeIteration};

use super::branch_owners::{self, BindingOrigins, DeclaredBranch, DeclaredBranches};
use super::implicit_branches::{
    actual_json_frame, controlled_input, enclosing_anchor, exact_constant_node, exact_literal,
    ordinary_primary_json, ordinary_scalar_domain, physical_scalar, plain_parents, plain_shape,
    unique_producer,
};
use super::take_scope;
use crate::import::graph::GraphBuilder;
use crate::import::group_projection::TargetIteration;
use crate::import::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use crate::import::scope::{BindingOccurrence, ScopeBuilder, TargetLeaf};

const MAX_BRANCHES: usize = 64;
const MAX_BINDINGS: usize = 256;
const MAX_PATH: usize = 64;
const MAX_OPERAND_DEPTH: usize = 16;
const MAX_OPERANDS: usize = 256;

/// One proved scalar operand, compared against its lowered graph node.
#[derive(Debug)]
enum Operand {
    Literal(Value),
    /// A physical field of the primary source. Nested frames keep the
    /// enclosing collection as `frame`; the document root has none.
    Field {
        frame: Option<Vec<String>>,
        path: Vec<String>,
    },
    Conditional {
        condition: Box<Operand>,
        then: Option<Box<Operand>>,
        otherwise: Option<Box<Operand>>,
    },
    Equal(Box<Operand>, Box<Operand>),
    Empty(Box<Operand>),
}

impl Operand {
    fn matches_lowered(&self, node: NodeId, builder: &GraphBuilder<'_>) -> bool {
        let lowered = builder.graph.nodes.get(&node);
        match self {
            Self::Literal(value) => exact_constant_node(node, value, builder),
            Self::Field { frame, path } => matches!(
                lowered,
                Some(Node::SourceField { path: actual_path, frame: actual_frame })
                    if actual_path == path && actual_frame == frame
            ),
            Self::Conditional {
                condition,
                then,
                otherwise,
            } => {
                let Some(Node::If {
                    condition: actual_condition,
                    then: actual_then,
                    else_: actual_otherwise,
                }) = lowered
                else {
                    return false;
                };
                condition.matches_lowered(*actual_condition, builder)
                    && branch_matches(then.as_deref(), *actual_then, builder)
                    && branch_matches(otherwise.as_deref(), *actual_otherwise, builder)
            }
            Self::Equal(left, right) => matches!(
                lowered,
                Some(Node::Call { function, args }) if function == "equal"
                    && matches!(args.as_slice(), [actual_left, actual_right]
                        if left.matches_lowered(*actual_left, builder)
                            && right.matches_lowered(*actual_right, builder))
            ),
            Self::Empty(input) => matches!(
                lowered,
                Some(Node::Call { function, args }) if function == "is_empty"
                    && matches!(args.as_slice(), [actual] if input.matches_lowered(*actual, builder))
            ),
        }
    }
}

/// A declared keyless branch lowers to the existing lazy Null constant.
fn branch_matches(branch: Option<&Operand>, node: NodeId, builder: &GraphBuilder<'_>) -> bool {
    match branch {
        Some(operand) => operand.matches_lowered(node, builder),
        None => exact_constant_node(node, &Value::Null, builder),
    }
}

/// Bounded raw walk over connected operands in one plain enclosing frame.
struct OperandWalk<'w, 'a> {
    builder: &'w GraphBuilder<'a>,
    anchor: &'w [String],
    active: BTreeSet<u32>,
    visited: usize,
}

impl OperandWalk<'_, '_> {
    fn operand(&mut self, feed: u32, depth: usize) -> Result<(Operand, ScalarType), &'static str> {
        self.visited += 1;
        if depth > MAX_OPERAND_DEPTH || self.visited > MAX_OPERANDS {
            return Err("operand depth or count exceeds the bound");
        }
        if !unique_producer(feed, self.builder) {
            return Err("an operand has no unique declared producer");
        }
        if self
            .builder
            .sources
            .first()
            .is_some_and(|primary| primary.ports.contains_key(&feed))
        {
            return self.field(feed);
        }
        let Some(component) = self
            .builder
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.builder.fn_components.get(*index))
        else {
            return Err("an operand is not a literal, physical field, or admitted operator");
        };
        if !component.raw_single_output_valid
            || component.output_pins.as_slice() != [Some(feed)]
            || component.outputs.as_slice() != [feed]
        {
            return Err("an operand has malformed output pins");
        }
        if (
            component.library.as_str(),
            component.kind,
            component.name.as_str(),
        ) == ("core", 2, "constant")
        {
            let value = exact_literal(feed, self.builder)
                .filter(|_| component.raw_literal_valid)
                .ok_or("an operand literal is not one exact scalar constant")?;
            let ty = literal_type(&value).ok_or("an operand literal has no scalar type")?;
            return Ok((Operand::Literal(value), ty));
        }
        if !matches!(
            (
                component.library.as_str(),
                component.kind,
                component.name.as_str()
            ),
            ("core", 4, "if-else") | ("core", 5, "equal") | ("lang", 5, "empty")
        ) {
            return Err("an operand is not a literal, physical field, or admitted operator");
        }
        let operator = component
            .presence_operator
            .as_ref()
            .filter(|operator| operator.output == feed)
            .ok_or("an operator has malformed raw pins, metadata, or owners")?;
        if !self.active.insert(feed) {
            return Err("operands form a cycle");
        }
        let mut operands = Vec::with_capacity(operator.inputs.len());
        for (input, raw_feed) in operator.inputs.iter().zip(&operator.feeds) {
            operands.push(match input {
                None => None,
                Some(input) => {
                    let Some(feed) = self
                        .builder
                        .edge_from
                        .get(input)
                        .copied()
                        .filter(|feed| Some(*feed) == *raw_feed)
                    else {
                        return Err("an operator input is not connected to its saved feed");
                    };
                    Some(self.operand(feed, depth + 1)?)
                }
            });
        }
        self.active.remove(&feed);
        let mut operands = operands.into_iter();
        match (component.library.as_str(), component.name.as_str()) {
            ("core", "if-else") => {
                let (Some(Some((condition, ScalarType::Bool))), Some(then), Some(otherwise)) =
                    (operands.next(), operands.next(), operands.next())
                else {
                    return Err("a conditional has no connected Boolean condition");
                };
                let ty = match (&then, &otherwise) {
                    (Some((_, left)), Some((_, right))) if left == right => *left,
                    (Some((_, ty)), None) | (None, Some((_, ty))) => *ty,
                    (Some(_), Some(_)) => return Err("conditional branches have different types"),
                    (None, None) => return Err("a conditional selects no declared branch"),
                };
                Ok((
                    Operand::Conditional {
                        condition: Box::new(condition),
                        then: then.map(|(operand, _)| Box::new(operand)),
                        otherwise: otherwise.map(|(operand, _)| Box::new(operand)),
                    },
                    ty,
                ))
            }
            ("core", "equal") => {
                let (Some(Some((left, left_ty))), Some(Some((right, right_ty)))) =
                    (operands.next(), operands.next())
                else {
                    return Err("an equality has a disconnected operand");
                };
                let numeric = |ty| matches!(ty, ScalarType::Int | ScalarType::Float);
                if left_ty != right_ty && !(numeric(left_ty) && numeric(right_ty)) {
                    return Err("an equality compares incompatible scalar types");
                }
                Ok((
                    Operand::Equal(Box::new(left), Box::new(right)),
                    ScalarType::Bool,
                ))
            }
            ("lang", "empty") => {
                let Some(Some((input, ScalarType::String))) = operands.next() else {
                    return Err("an empty test requires one String operand");
                };
                Ok((Operand::Empty(Box::new(input)), ScalarType::Bool))
            }
            _ => Err("an operand is not a literal, physical field, or admitted operator"),
        }
    }

    fn field(&self, feed: u32) -> Result<(Operand, ScalarType), &'static str> {
        let builder = self.builder;
        let primary = builder
            .sources
            .first()
            .ok_or("the primary source is missing")?;
        let absolute = physical_scalar(feed, self.anchor, builder)
            .filter(|absolute| ordinary_scalar_domain(absolute, self.anchor, &primary.schema))
            .ok_or("a physical operand is outside the plain enclosing frame")?;
        let SchemaKind::Scalar { ty } = schema_node_at(&primary.schema, absolute)
            .ok_or("a physical operand is missing from its schema")?
            .kind
        else {
            return Err("a physical operand is not a single scalar");
        };
        let operand = if self.anchor.is_empty() {
            Operand::Field {
                frame: None,
                path: absolute.to_vec(),
            }
        } else {
            Operand::Field {
                frame: Some(self.anchor.to_vec()),
                path: absolute
                    .strip_prefix(self.anchor)
                    .ok_or("a physical operand is outside its frame")?
                    .to_vec(),
            }
        };
        Ok((operand, ty))
    }
}

fn literal_type(value: &Value) -> Option<ScalarType> {
    match value {
        Value::String(_) => Some(ScalarType::String),
        Value::Bool(_) => Some(ScalarType::Bool),
        Value::Int(_) => Some(ScalarType::Int),
        Value::Float(_) => Some(ScalarType::Float),
        _ => None,
    }
}

/// One declared clone controlled by one conditional presence driver.
struct BranchPlan {
    lineage: Vec<u32>,
    anchors: BTreeMap<Vec<String>, Vec<String>>,
    enclosing: Vec<String>,
    driver: u32,
    presence: Operand,
    bindings: Vec<(usize, Operand)>,
}

/// Lowers declared clones driven by conditional presence, or refuses them with
/// one diagnostic while leaving every import state unchanged. Returns whether
/// this recognizer owns the target path; the ordinary lowering still builds an
/// owned path that was refused, so the repair draft is retained.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    iteration: &TargetIteration,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    claimed: &mut BTreeSet<usize>,
    declared: &mut DeclaredBranches,
) -> Option<bool> {
    let path = &iteration.target_path;
    let feeds = std::iter::once(iteration.feed)
        .chain(iteration.additional_feeds.iter().map(|(feed, _, _)| *feed))
        .collect::<Vec<_>>();
    let markers = target
        .input_branch_paths
        .iter()
        .filter(|(_, branch)| branch == path)
        .map(|(marker, _)| *marker)
        .collect::<Vec<_>>();
    // Only declared clones driven by a conditional belong to this contract.
    if markers.len() < 2 || !feeds.iter().any(|feed| conditional_feed(*feed, builder)) {
        return None;
    }
    let plans = match plan(
        path, &markers, iteration, target, bindings, builder, scopes, declared,
    ) {
        Ok(plans) => plans,
        Err(reason) => {
            refuse(path, reason, builder);
            return Some(false);
        }
    };
    let Some((graph, candidate_scopes, indices, branches)) =
        lower(path, plans, target, bindings, builder, scopes)
    else {
        refuse(
            path,
            "the lowered operands differ from their proof",
            builder,
        );
        return Some(false);
    };
    *builder = graph;
    *scopes = candidate_scopes;
    claimed.extend(indices);
    declared.record(path, branches);
    Some(true)
}

fn refuse(path: &[String], reason: &str, builder: &mut GraphBuilder<'_>) {
    builder.warnings.push(format!(
        "declared target clones in `{}` cannot prove conditional scalar presence ({reason}); presence lowering skipped",
        path.join("/")
    ));
}

fn conditional_feed(feed: u32, builder: &GraphBuilder<'_>) -> bool {
    builder
        .fn_by_output
        .get(&feed)
        .and_then(|index| builder.fn_components.get(*index))
        .is_some_and(|component| {
            component.library == "core" && component.kind == 4 && component.name == "if-else"
        })
}

#[allow(clippy::too_many_arguments)]
fn plan(
    path: &[String],
    markers: &[u32],
    iteration: &TargetIteration,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
) -> Result<Vec<BranchPlan>, &'static str> {
    if target.format != ComponentFormat::Xml || !ordinary_primary_json(builder) {
        return Err("only plain JSON sources and XML targets are qualified");
    }
    if iteration.output != IterationOutput::Repeated
        || iteration.join.is_some()
        || iteration.projects_whole_group
        || iteration
            .additional_feeds
            .iter()
            .any(|(_, _, copy_all)| *copy_all)
    {
        return Err("the clone driver is controlled, joined, or copied");
    }
    let target_node = schema_node_at(&target.schema, path).ok_or("the clone path is missing")?;
    if target.has_target_node_controls
        || markers.len() > MAX_BRANCHES
        || path.len() > MAX_PATH
        || !target_node.repeating
        || !matches!(target_node.kind, SchemaKind::Group { .. })
        || !plain_shape(target_node, 0)
    {
        return Err("the clone shape is not a plain repeating group of scalars");
    }
    if !plain_parents(path, scopes)
        || scopes
            .scope(path)
            .is_some_and(|scope| !branch_owners::replaceable_placeholder(scope))
    {
        return Err("a parent or existing clone scope is controlled");
    }
    let mut owned = BTreeMap::<u32, Vec<usize>>::new();
    for (input, input_path) in &target.ports {
        if !input_path.starts_with(path) || !builder.edge_from.contains_key(input) {
            continue;
        }
        if !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || target.ambiguous_input_affects(*input, path)
            || controlled_input(target, *input, builder)
        {
            return Err("a clone input has ambiguous or controlled ownership");
        }
        let mut owners = target
            .input_ancestors
            .get(input)
            .into_iter()
            .flatten()
            .filter(|ancestor| markers.contains(ancestor));
        let (Some(_), None) = (owners.next(), owners.next()) else {
            return Err("a clone input has no unique declared clone");
        };
    }
    let mut count = 0;
    for (index, (binding, _, input)) in bindings.iter().enumerate() {
        let binding_path = binding.path();
        if binding_path.len() <= path.len() || !binding_path.starts_with(path) {
            continue;
        }
        count += 1;
        let mut owners = target
            .input_ancestors
            .get(input)
            .into_iter()
            .flatten()
            .filter(|ancestor| markers.contains(ancestor));
        let (Some(owner), None) = (owners.next(), owners.next()) else {
            return Err("a clone binding has no unique declared clone");
        };
        owned.entry(*owner).or_default().push(index);
        if count > MAX_BINDINGS {
            return Err("the clone binding count exceeds the bound");
        }
    }
    let mut plans = Vec::with_capacity(markers.len());
    for marker in markers {
        let mut drivers = Vec::new();
        let mut connected = false;
        for (input, input_path) in &target.ports {
            if !input_path.starts_with(path)
                || !builder.edge_from.contains_key(input)
                || !target
                    .input_ancestors
                    .get(input)
                    .is_some_and(|ancestors| ancestors.contains(marker))
            {
                continue;
            }
            connected = true;
            let node = schema_node_at(&target.schema, input_path)
                .ok_or("a clone input is missing from the target schema")?;
            if node.is_scalar() {
                continue;
            }
            if input_path != path {
                return Err("a clone has a connected structural descendant");
            }
            drivers.push(builder.edge_from[input]);
        }
        let indices = owned.remove(marker).unwrap_or_default();
        let driver = match drivers.as_slice() {
            // An unconnected declaration has no executable occurrence.
            [] if !connected => continue,
            [] => return Err("a connected clone has no presence driver"),
            [driver] if !unique_producer(*driver, builder) => {
                return Err("a clone driver has no unique declared producer");
            }
            [driver]
                if conditional_feed(*driver, builder) && iteration_owns(iteration, *driver) =>
            {
                *driver
            }
            [_] => return Err("a clone driver is not a conditional presence driver"),
            _ => return Err("a clone has competing drivers"),
        };
        let lineage =
            branch_owners::lineage(target, *marker).ok_or("a clone has ambiguous lineage")?;
        let anchors = declared
            .enclosing_anchors(path, &lineage, &scopes.anchors)
            .ok_or("a clone has ambiguous declared ancestor ownership")?;
        let enclosing = enclosing_anchor(path, &anchors);
        let plain_frame = if enclosing.is_empty() {
            (0..path.len()).all(|length| {
                scopes
                    .scope(&path[..length])
                    .is_none_or(|scope| matches!(scope.iteration, ScopeIteration::None))
            })
        } else {
            actual_json_frame(path, &enclosing, target, builder, scopes)
        };
        if !plain_frame {
            return Err("the enclosing source frame is not one plain physical iteration");
        }
        let mut walk = OperandWalk {
            builder,
            anchor: &enclosing,
            active: BTreeSet::new(),
            visited: 0,
        };
        let (presence, _) = walk.operand(driver, 0)?;
        let mut assigned = BTreeSet::new();
        let mut proved = Vec::with_capacity(indices.len());
        for index in indices {
            let (binding, feed, input) = &bindings[index];
            let binding_path = binding.path();
            let SchemaKind::Scalar { ty } = schema_node_at(&target.schema, &binding_path)
                .filter(|node| !node.repeating)
                .ok_or("a clone binding is not a single scalar")?
                .kind
            else {
                return Err("a clone binding is not a single scalar");
            };
            if !assigned.insert(binding_path.clone())
                || target.ports.get(input) != Some(&binding_path)
                || builder.edge_from.get(input) != Some(feed)
            {
                return Err("a clone binding is assigned more than once");
            }
            let (operand, operand_ty) = walk.operand(*feed, 0)?;
            if operand_ty != ty {
                return Err("a clone binding type differs from its target");
            }
            proved.push((index, operand));
        }
        plans.push(BranchPlan {
            lineage,
            anchors,
            enclosing,
            driver,
            presence,
            bindings: proved,
        });
    }
    if !owned.is_empty() || plans.is_empty() {
        return Err("a clone binding has no presence-driven clone");
    }
    Ok(plans)
}

fn iteration_owns(iteration: &TargetIteration, feed: u32) -> bool {
    iteration.feed == feed
        || iteration
            .additional_feeds
            .iter()
            .any(|(candidate, _, _)| *candidate == feed)
}

fn binding_count(scope: &mapping::Scope) -> usize {
    scope.bindings.len() + scope.children.iter().map(binding_count).sum::<usize>()
}

type Lowered<'a> = (
    GraphBuilder<'a>,
    ScopeBuilder,
    BTreeSet<usize>,
    Vec<DeclaredBranch>,
);

/// Trial lowering against cloned graph and scope state.
fn lower<'a>(
    path: &[String],
    plans: Vec<BranchPlan>,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'a>,
    scopes: &ScopeBuilder,
) -> Option<Lowered<'a>> {
    let mut graph = builder.clone();
    let mut segments = Vec::with_capacity(plans.len());
    let mut branches = Vec::with_capacity(plans.len());
    let mut indices = BTreeSet::new();
    for plan in plans {
        let mut anchors = plan.anchors;
        anchors.retain(|candidate, _| !candidate.starts_with(path));
        let mut local = ScopeBuilder::new(anchors);
        local.ensure_scope(path);
        let mut origins = BindingOrigins::new();
        for (index, operand) in &plan.bindings {
            let (binding, feed, input) = &bindings[*index];
            let binding_path = binding.path();
            let anchor = local.enclosing_anchor(&binding_path);
            let node = graph.binding_node_at_anchor(*feed, &binding_path, &anchor)?;
            if anchor != plan.enclosing || !operand.matches_lowered(node, &graph) {
                return None;
            }
            origins
                .entry((binding_path, node))
                .or_default()
                .insert(*index);
            local.add_binding(
                binding.clone(),
                node,
                BindingOccurrence::authored(target, *input),
            );
            indices.insert(*index);
        }
        let selected = graph.binding_node_at_anchor(plan.driver, path, &plan.enclosing)?;
        if !plan.presence.matches_lowered(selected, &graph) {
            return None;
        }
        // Presence is existence of the lazily selected scalar: false, zero and
        // empty String are present; Null, including a keyless branch, is not.
        let condition = graph.alloc(Node::Call {
            function: "exists".into(),
            args: vec![selected],
        });
        let mut segment = take_scope(&mut local.root, path)?;
        if binding_count(&segment) != plan.bindings.len() {
            return None;
        }
        segment.target_field.clear();
        segment.iteration = ScopeIteration::OptionalPresence { condition };
        branches.push(DeclaredBranch {
            lineage: plan.lineage,
            anchors: local.anchors,
            bindings: origins,
        });
        segments.push(segment);
    }
    let mut segments = segments.into_iter();
    let first = segments.next()?;
    let mut scopes = scopes.clone();
    let _ = take_scope(&mut scopes.root, path);
    scopes
        .anchors
        .retain(|candidate, _| !candidate.starts_with(path));
    scopes.add_concatenated(path, first, segments.collect(), IterationOutput::Repeated);
    Some((graph, scopes, indices, branches))
}
