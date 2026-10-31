use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;
use mapping::{
    ComputedJoinSource, ComputedJoinSourceId, ComputedJoinValue, IterationOutput,
    JoinConditions as MappingJoinConditions, JoinId, JoinKey as MappingJoinKey, JoinPlan,
    JoinSource, Node as MappingNode, NodeId,
};
use roxmltree::Node as XmlNode;

use super::graph::GraphBuilder;
use super::group_projection::TargetIteration;
use super::iteration::split_at_innermost_repeating;
use super::schema::{SchemaComponent, collect_matching_scalar_paths, parse_u32, schema_node_at};
use super::scope::ScopeBuilder;
use super::source::SourcePath;

mod computed;
mod parse;

use computed::ComputedInput;
pub(super) use computed::KeyContext;
use parse::{declared_output_ports, parse};

/// A plan holds at most one computed source, so its id is fixed.
const COMPUTED_KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParsedJoin {
    pub(super) tuple_output: Option<u32>,
    pub(super) inputs: Vec<JoinInput>,
    pub(super) equalities: Vec<JoinEquality>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinInput {
    pub(super) index: usize,
    pub(super) name: String,
    pub(super) input_port: u32,
    pub(super) outputs: Vec<JoinOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinOutput {
    pub(super) port: u32,
    pub(super) path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinEquality {
    pub(super) first: JoinKey,
    pub(super) second: JoinKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JoinKey {
    pub(super) input_index: usize,
    pub(super) path_id: u32,
    pub(super) path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlannedJoin {
    pub(super) plan: JoinPlan,
    pub(super) tuple_output: Option<u32>,
    pub(super) outputs: Vec<PlannedJoinOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlannedJoinOutput {
    pub(super) port: u32,
    pub(super) input_index: usize,
    pub(super) collection: Vec<String>,
    pub(super) path: Vec<String>,
}

impl ParsedJoin {
    #[cfg(test)]
    pub(super) fn to_plan(&self, collections: &[Vec<String>]) -> Result<PlannedJoin, String> {
        let sources = collections
            .iter()
            .cloned()
            .map(JoinSource::new)
            .collect::<Vec<_>>();
        self.to_plan_sources(&sources)
    }

    fn to_plan_sources(&self, sources: &[JoinSource]) -> Result<PlannedJoin, String> {
        if sources.len() != self.inputs.len() || sources.len() < 2 {
            return Err(format!(
                "join has {} parsed inputs but {} resolved collections",
                self.inputs.len(),
                sources.len()
            ));
        }
        let mut plan = JoinPlan::new(
            sources[0].clone(),
            sources[1].clone(),
            self.stage_conditions(1, sources)?,
        )
        .map_err(|error| error.to_string())?;
        for (stage, source) in sources.iter().enumerate().skip(2) {
            plan = plan
                .then(source.clone(), self.stage_conditions(stage, sources)?)
                .map_err(|error| error.to_string())?;
        }
        let mut outputs = Vec::new();
        for input in self.inputs.iter().filter(|input| !input.outputs.is_empty()) {
            let collection = sources[input.index]
                .as_collection()
                .ok_or_else(|| format!("computed join input {} has an output", input.index))?
                .collection();
            outputs.extend(input.outputs.iter().map(|output| PlannedJoinOutput {
                port: output.port,
                input_index: input.index,
                collection: collection.to_vec(),
                path: output.path.clone(),
            }));
        }
        Ok(PlannedJoin {
            plan,
            tuple_output: self.tuple_output,
            outputs,
        })
    }

    fn stage_conditions(
        &self,
        stage: usize,
        sources: &[JoinSource],
    ) -> Result<MappingJoinConditions, String> {
        let mut conditions = self.equalities.iter().filter_map(|equality| {
            let (left, right) = if equality.first.input_index == stage
                && equality.second.input_index < stage
            {
                (&equality.second, &equality.first)
            } else if equality.second.input_index == stage && equality.first.input_index < stage {
                (&equality.first, &equality.second)
            } else {
                return None;
            };
            Some(match &sources[left.input_index] {
                JoinSource::Collection(source) => MappingJoinKey::new(
                    source.collection().to_vec(),
                    left.path.clone(),
                    right.path.clone(),
                ),
                JoinSource::Computed(source) => {
                    MappingJoinKey::computed(source.id(), right.path.clone())
                }
            })
        });
        let first = conditions
            .next()
            .ok_or_else(|| format!("join input {stage} has no equality with an earlier input"))?;
        Ok(conditions.fold(
            MappingJoinConditions::new(first),
            |conditions, condition| conditions.and(condition),
        ))
    }
}

#[derive(Default)]
pub(super) struct PendingJoins {
    joins: Vec<PendingJoin>,
    rejected_row_outputs: BTreeSet<u32>,
    rejected_field_outputs: BTreeSet<u32>,
}

struct PendingJoin {
    name: String,
    id: JoinId,
    parsed: ParsedJoin,
}

#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug))]
pub(super) struct Registry {
    joins: BTreeMap<JoinId, ResolvedJoin>,
    row_outputs: BTreeMap<u32, JoinId>,
    field_outputs: BTreeMap<u32, ResolvedField>,
    hierarchical_rows: BTreeMap<u32, SourcePath>,
    hierarchical_fields: BTreeMap<u32, SourcePath>,
    rejected_row_outputs: BTreeSet<u32>,
    rejected_field_outputs: BTreeSet<u32>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
struct ResolvedJoin {
    parsed: ParsedJoin,
    inputs: Vec<ResolvedInput>,
    /// The root-context plan. A computed key is lowered only in the
    /// receiving context of the iteration it drives, so it has none.
    root: Option<PlannedJoin>,
    planned: Option<PlannedJoin>,
    target_path: Option<Vec<String>>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
enum ResolvedInput {
    Source(SourceInput),
    Computed(ComputedInput),
}

impl ResolvedInput {
    fn as_source(&self) -> Option<&SourceInput> {
        match self {
            Self::Source(input) => Some(input),
            Self::Computed(_) => None,
        }
    }
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
struct SourceInput {
    source: usize,
    path: Vec<String>,
    singleton: bool,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
struct ResolvedField {
    join: JoinId,
    input_index: usize,
    path: Vec<String>,
}

struct ResolvedHierarchy {
    rows: Vec<(u32, SourcePath)>,
    fields: Vec<(u32, SourcePath)>,
}

pub(super) enum IterationFeed {
    Ordinary,
    Join(JoinId),
    Rejected,
}

pub(super) enum PreparedIteration {
    Owner,
    Projection,
    Rejected,
}

impl PendingJoins {
    pub(super) fn read(&mut self, component: XmlNode<'_, '_>, warnings: &mut Vec<String>) {
        let name = component.attribute("name").unwrap_or("join").to_string();
        let (row_outputs, field_outputs) = declared_output_ports(component);
        let result = parse_u32(component.attribute("uid"))
            .ok_or_else(|| "component uid is missing or invalid".to_string())
            .and_then(|uid| parse(&component).map(|parsed| (JoinId::new(u64::from(uid)), parsed)));
        match result {
            Ok((id, parsed)) => self.joins.push(PendingJoin { name, id, parsed }),
            Err(reason) => {
                self.rejected_row_outputs.extend(row_outputs);
                self.rejected_field_outputs.extend(field_outputs);
                warnings.push(format!("join `{name}` is unsupported: {reason}; skipped"))
            }
        }
    }

    pub(super) fn resolve(self, context: &KeyContext<'_>, warnings: &mut Vec<String>) -> Registry {
        let mut registry = Registry {
            rejected_row_outputs: self.rejected_row_outputs,
            rejected_field_outputs: self.rejected_field_outputs,
            ..Registry::default()
        };
        let mut seen = BTreeSet::new();
        for pending in self.joins {
            if !seen.insert(pending.id) {
                registry.reject(&pending.parsed);
                warnings.push(format!(
                    "join `{}` is unsupported: duplicate component uid {}; skipped",
                    pending.name,
                    pending.id.get()
                ));
                continue;
            }
            let resolved = if pending.parsed.equalities.is_empty() {
                resolve_hierarchy(&pending, context.edge_from, context.sources)
                    .map(EitherJoin::Hierarchy)
            } else {
                resolve_join(&pending, context)
                    .map(Box::new)
                    .map(EitherJoin::Keyed)
            };
            match resolved {
                Ok(EitherJoin::Keyed(resolved)) => registry.insert(pending.id, *resolved),
                Ok(EitherJoin::Hierarchy(resolved)) => registry.insert_hierarchy(resolved),
                Err(reason) => {
                    registry.reject(&pending.parsed);
                    warnings.push(format!(
                        "join `{}` is unsupported: {reason}; skipped",
                        pending.name
                    ));
                }
            }
        }
        registry
    }
}

impl Registry {
    fn reject(&mut self, parsed: &ParsedJoin) {
        self.rejected_row_outputs
            .extend(parsed.tuple_output.iter().copied());
        for output in parsed.inputs.iter().flat_map(|input| &input.outputs) {
            if output.path.is_empty() {
                self.rejected_row_outputs.insert(output.port);
            } else {
                self.rejected_field_outputs.insert(output.port);
            }
        }
    }

    fn insert(&mut self, id: JoinId, resolved: ResolvedJoin) {
        if let Some(port) = resolved.parsed.tuple_output {
            self.row_outputs.insert(port, id);
        }
        for input in &resolved.parsed.inputs {
            for output in &input.outputs {
                if output.path.is_empty() {
                    self.row_outputs.insert(output.port, id);
                } else {
                    self.field_outputs.insert(
                        output.port,
                        ResolvedField {
                            join: id,
                            input_index: input.index,
                            path: output.path.clone(),
                        },
                    );
                }
            }
        }
        self.joins.insert(id, resolved);
    }

    fn insert_hierarchy(&mut self, resolved: ResolvedHierarchy) {
        self.hierarchical_rows.extend(resolved.rows);
        self.hierarchical_fields.extend(resolved.fields);
    }

    pub(super) fn hierarchical_source(&self, port: u32) -> Option<&SourcePath> {
        self.hierarchical_rows
            .get(&port)
            .or_else(|| self.hierarchical_fields.get(&port))
    }

    fn hierarchical_field(&self, port: u32) -> Option<&SourcePath> {
        self.hierarchical_fields.get(&port)
    }

    pub(super) fn row_join(&self, port: u32) -> Option<JoinId> {
        self.row_outputs.get(&port).copied()
    }

    fn row_rejected(&self, port: u32) -> bool {
        self.rejected_row_outputs.contains(&port)
    }

    pub(super) fn output_rejected(&self, port: u32) -> bool {
        self.rejected_row_outputs.contains(&port) || self.rejected_field_outputs.contains(&port)
    }

    fn prepare(
        &mut self,
        id: JoinId,
        target_path: &[String],
        anchor: &[String],
        computed_key: Option<ComputedJoinValue>,
        sources: &[&SchemaComponent],
        source_names: &[String],
    ) -> Result<PreparedIteration, String> {
        let join = self
            .joins
            .get_mut(&id)
            .ok_or("join descriptor is missing")?;
        if let Some(existing) = &join.target_path {
            if existing == target_path {
                return Ok(PreparedIteration::Owner);
            }
            if target_path.starts_with(existing) {
                return Ok(PreparedIteration::Projection);
            }
            return Err("one join output is used under incomparable target contexts".to_string());
        }
        let plan_sources = join
            .inputs
            .iter()
            .map(|input| {
                let input = match input {
                    ResolvedInput::Source(input) => input,
                    ResolvedInput::Computed(_) => {
                        let value = computed_key
                            .clone()
                            .ok_or("the computed join key was not lowered")?;
                        return Ok(JoinSource::Computed(ComputedJoinSource::with_value(
                            COMPUTED_KEY,
                            value,
                        )));
                    }
                };
                let mut absolute = if input.source == 0 {
                    Vec::new()
                } else {
                    vec![
                        source_names
                            .get(input.source)
                            .cloned()
                            .ok_or("join source has no runtime name")?,
                    ]
                };
                absolute.extend(input.path.iter().cloned());
                let path = if absolute.starts_with(anchor) {
                    absolute[anchor.len()..].to_vec()
                } else {
                    absolute
                };
                Ok(if input.singleton {
                    JoinSource::singleton(path)
                } else {
                    JoinSource::new(path)
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let planned = join.parsed.to_plan_sources(&plan_sources)?;
        validate_outputs(&planned, &join.inputs, sources)?;
        join.target_path = Some(target_path.to_vec());
        join.planned = Some(planned);
        Ok(PreparedIteration::Owner)
    }

    /// The computed input of a join no iteration has prepared yet: only the
    /// first preparation lowers its key.
    fn unprepared_computed_key(&self, id: JoinId) -> Option<ComputedInput> {
        let join = self.joins.get(&id)?;
        if join.target_path.is_some() {
            return None;
        }
        join.inputs.iter().find_map(|input| match input {
            ResolvedInput::Computed(computed) => Some(computed.clone()),
            ResolvedInput::Source(_) => None,
        })
    }

    fn planned(&self, id: JoinId) -> Option<&PlannedJoin> {
        self.joins.get(&id)?.planned.as_ref()
    }

    fn activate_root_plan(&mut self, id: JoinId) -> Result<JoinPlan, String> {
        let join = self
            .joins
            .get_mut(&id)
            .ok_or("join descriptor is missing")?;
        let root = join.root.as_ref().ok_or(
            "a computed join key feeding a joined aggregate is not supported; it was not observed",
        )?;
        if let Some(planned) = &join.planned
            && planned.plan != root.plan
        {
            return Err("joined sequence is owned by an incompatible nested context".to_string());
        }
        join.planned.get_or_insert_with(|| root.clone());
        Ok(root.plan.clone())
    }

    fn field(&self, port: u32) -> Option<&ResolvedField> {
        self.field_outputs.get(&port)
    }

    fn field_join(&self, port: u32) -> Option<JoinId> {
        self.field(port).map(|field| field.join)
    }
}

impl GraphBuilder<'_> {
    pub(super) fn classify_join_iteration(
        &mut self,
        feed: u32,
        target_path: &[String],
    ) -> IterationFeed {
        let resolved = self.resolve_iteration_feed(feed);
        let Some(id) = self.joins.row_join(resolved.source_key) else {
            if self.joins.row_rejected(resolved.source_key) {
                self.rejected_join_paths.insert(target_path.to_vec());
                return IterationFeed::Rejected;
            }
            return IterationFeed::Ordinary;
        };
        let grouped = resolved.has_key_grouping
            || resolved.has_start_grouping
            || resolved.has_adjacent_grouping
            || resolved.has_end_grouping
            || resolved.has_block_grouping
            || resolved.distinct_key.is_some();
        let controlled = resolved.sequence_component.is_some()
            || resolved.db_where_component.is_some()
            || resolved.order_issue.is_some()
            || !resolved.source_suffix.is_empty()
            || resolved.projects_whole_group
            || !resolved.projections.is_empty();
        if grouped || controlled {
            self.rejected_join_paths.insert(target_path.to_vec());
            if self.warned_join_controls.insert(id) {
                let reason = if grouped {
                    "is followed by grouping, which is not supported"
                } else {
                    "uses sequence controls that cannot be represented"
                };
                self.warnings.push(format!(
                    "join feeding `{}` {reason}; iteration skipped",
                    target_path.join("/")
                ));
            }
            IterationFeed::Rejected
        } else {
            IterationFeed::Join(id)
        }
    }

    pub(super) fn prepare_join_iteration(
        &mut self,
        id: JoinId,
        target_path: &[String],
        allow_projection: bool,
        scopes: &ScopeBuilder,
    ) -> PreparedIteration {
        let anchor = scopes.enclosing_anchor(target_path);
        let computed_key = self
            .joins
            .unprepared_computed_key(id)
            .map(|computed| self.lower_computed_key(&computed, &anchor))
            .transpose();
        let prepared = computed_key.and_then(|computed_key| {
            self.joins.prepare(
                id,
                target_path,
                &anchor,
                computed_key,
                self.sources,
                self.source_names,
            )
        });
        match prepared {
            Ok(PreparedIteration::Projection) if !allow_projection => {
                self.rejected_join_paths.insert(target_path.to_vec());
                if self.warned_join_controls.insert(id) {
                    self.warnings.push(format!(
                        "join feeding `{}` projects into a repeating descendant target; iteration skipped",
                        target_path.join("/")
                    ));
                }
                PreparedIteration::Rejected
            }
            Ok(prepared) => prepared,
            Err(reason) => {
                self.rejected_join_paths.insert(target_path.to_vec());
                if self.warned_join_controls.insert(id) {
                    self.warnings.push(format!(
                        "join feeding `{}` is unsupported: {reason}; iteration skipped",
                        target_path.join("/")
                    ));
                }
                PreparedIteration::Rejected
            }
        }
    }

    pub(super) fn join_plan(&self, id: JoinId) -> Option<JoinPlan> {
        self.joins.planned(id).map(|join| join.plan.clone())
    }

    /// Resolves a naked joined tuple sequence, or a scalar expression whose
    /// dynamic leaves all belong to one root-context join. Sequence controls
    /// and physical source leaves deliberately stop provenance so aggregates
    /// cannot silently broaden or change their iteration context.
    pub(super) fn join_aggregate_sequence(
        &mut self,
        feed: u32,
    ) -> Result<Option<(JoinId, JoinPlan, Option<NodeId>)>, String> {
        if let Some(join) = self.joins.row_join(feed) {
            let plan = self.joins.activate_root_plan(join)?;
            return Ok(Some((join, plan, None)));
        }

        fn merge(owner: &mut Option<JoinId>, candidate: JoinId) -> Result<(), ()> {
            match *owner {
                Some(existing) if existing != candidate => Err(()),
                Some(_) => Ok(()),
                None => {
                    *owner = Some(candidate);
                    Ok(())
                }
            }
        }

        fn visit(
            builder: &GraphBuilder<'_>,
            port: u32,
            visiting: &mut BTreeSet<u32>,
            visited: &mut BTreeSet<u32>,
            owner: &mut Option<JoinId>,
        ) -> Result<(), ()> {
            if visited.contains(&port) {
                return Ok(());
            }
            if !visiting.insert(port) {
                return Err(());
            }
            let result = if let Some(join) = builder.joins.row_join(port) {
                merge(owner, join)
            } else if let Some(join) = builder.joins.field_join(port) {
                merge(owner, join)
            } else if builder.joins.output_rejected(port)
                || builder.source_abs_path(port).is_some()
                || builder.intermediate_feed(port).is_some()
                || builder.udf_by_output.contains_key(&port)
            {
                Err(())
            } else {
                let &index = builder.fn_by_output.get(&port).ok_or(())?;
                let component = &builder.fn_components[index];
                if !super::function::produces_scalar(component)
                    || super::function::aggregate_op(&component.name).is_some()
                {
                    return Err(());
                }
                for input in component.inputs.iter().flatten() {
                    if let Some(&upstream) = builder.edge_from.get(input) {
                        visit(builder, upstream, visiting, visited, owner)?;
                    }
                }
                Ok(())
            };
            visiting.remove(&port);
            if result.is_ok() {
                visited.insert(port);
            }
            result
        }

        let joined_dependency = self.join_dependency_any(feed);
        let mut owner = None;
        if visit(
            self,
            feed,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut owner,
        )
        .is_err()
        {
            return if joined_dependency {
                Err(
                    "sequence expression mixes joined tuples with an unsupported context"
                        .to_string(),
                )
            } else {
                Ok(None)
            };
        }
        let Some(join) = owner else {
            return Ok(None);
        };
        let plan = self.joins.activate_root_plan(join)?;
        let expression = self
            .value_node(feed)
            .ok_or_else(|| "joined sequence expression cannot be materialized".to_string())?;
        Ok(Some((join, plan, Some(expression))))
    }

    pub(super) fn join_dependency_any(&self, port: u32) -> bool {
        fn visit(builder: &GraphBuilder<'_>, port: u32, visited: &mut BTreeSet<u32>) -> bool {
            if !visited.insert(port) {
                return false;
            }
            if builder.joins.row_join(port).is_some()
                || builder.joins.field_join(port).is_some()
                || builder.joins.output_rejected(port)
            {
                return true;
            }
            if let Some(intermediate) = builder.intermediate_feed(port) {
                return visit(builder, intermediate.feed, visited);
            }
            let Some(&component) = builder.fn_by_output.get(&port) else {
                return false;
            };
            builder.fn_components[component]
                .inputs
                .iter()
                .flatten()
                .filter_map(|input| builder.edge_from.get(input).copied())
                .any(|feed| visit(builder, feed, visited))
        }

        visit(self, port, &mut BTreeSet::new())
    }

    pub(super) fn join_dependency_rejected(&self, port: u32) -> bool {
        fn visit(builder: &GraphBuilder<'_>, port: u32, visited: &mut BTreeSet<u32>) -> bool {
            if !visited.insert(port) {
                return false;
            }
            if builder.joins.output_rejected(port) {
                return true;
            }
            let Some(&component) = builder.fn_by_output.get(&port) else {
                return false;
            };
            builder.fn_components[component]
                .inputs
                .iter()
                .flatten()
                .filter_map(|input| builder.edge_from.get(input).copied())
                .any(|feed| visit(builder, feed, visited))
        }

        visit(self, port, &mut BTreeSet::new())
    }

    pub(super) fn join_field_node(&mut self, port: u32) -> Option<NodeId> {
        if let Some(source) = self.joins.hierarchical_field(port).cloned() {
            let source = self.source_value_path(source.source, source.path);
            return self.source_field_at(&source);
        }
        let field = self.joins.field(port)?;
        let join = field.join;
        let input_index = field.input_index;
        let path = field.path.clone();
        let collection = self
            .joins
            .planned(join)?
            .outputs
            .iter()
            .find(|output| output.port == port && output.input_index == input_index)?
            .collection
            .clone();
        Some(self.alloc(MappingNode::JoinField {
            join,
            collection,
            path,
        }))
    }

    /// Materializes same-named scalar descendants for a structural join
    /// branch. Unlike ordinary group projection, every field is pinned to the
    /// exact joined tuple member and cannot fall back to another source frame.
    pub(super) fn join_branch_fields(
        &self,
        port: u32,
        target: &ir::SchemaNode,
    ) -> Vec<(Vec<String>, MappingNode)> {
        let Some(join) = self.joins.row_join(port) else {
            return Vec::new();
        };
        let Some(resolved) = self.joins.joins.get(&join) else {
            return Vec::new();
        };
        let Some(output) = resolved.planned.as_ref().and_then(|planned| {
            planned
                .outputs
                .iter()
                .find(|output| output.port == port && output.path.is_empty())
        }) else {
            // The tuple output is also a row output, but does not identify one
            // structural source branch.
            return Vec::new();
        };
        let Some(input) = resolved
            .inputs
            .get(output.input_index)
            .and_then(ResolvedInput::as_source)
        else {
            return Vec::new();
        };
        let Some(source) = self.sources.get(input.source) else {
            return Vec::new();
        };
        let Some(source_group) = schema_node_at(&source.schema, &input.path) else {
            return Vec::new();
        };
        let collection = output.collection.clone();
        let mut paths = Vec::new();
        collect_matching_scalar_paths(source_group, target, &mut Vec::new(), &mut paths);

        paths
            .into_iter()
            .map(|path| {
                let node = MappingNode::JoinField {
                    join,
                    collection: collection.clone(),
                    path: path.clone(),
                };
                (path, node)
            })
            .collect()
    }

    pub(super) fn join_position_node(&self, component: usize) -> Option<MappingNode> {
        let input = self
            .fn_components
            .get(component)?
            .inputs
            .first()?
            .as_ref()?;
        let feed = self.edge_from.get(input).copied()?;
        let resolved = self.resolve_iteration_feed(feed);
        self.joins
            .row_join(resolved.source_key)
            .map(|join| MappingNode::JoinPosition { join })
    }
}

pub(super) fn prepare_iterations(
    iterations: &[TargetIteration],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    for iteration in iterations {
        if let Some(join) = iteration.join {
            builder.prepare_join_iteration(
                join,
                &iteration.target_path,
                iteration.output == IterationOutput::MappedSequence,
                scopes,
            );
            continue;
        }
        let feed = builder.resolve_iteration_feed(iteration.feed);
        let Some(source_path) = builder.iteration_source_path(&feed) else {
            continue;
        };
        let structural_source = builder
            .schema_node(&source_path)
            .is_some_and(|node| matches!(node.kind, ir::SchemaKind::Group { .. }));
        let scope_source =
            if iteration.output == IterationOutput::MappedSequence || structural_source {
                builder
                    .sources
                    .get(source_path.source)
                    .map(|source| SourcePath {
                        source: source_path.source,
                        path: split_at_innermost_repeating(&source.schema, &source_path.path).0,
                    })
                    .unwrap_or_else(|| source_path.clone())
            } else {
                source_path
            };
        scopes.anchors.insert(
            iteration.target_path.clone(),
            builder.context_path(&scope_source),
        );
    }
}

fn resolve_join(pending: &PendingJoin, context: &KeyContext<'_>) -> Result<ResolvedJoin, String> {
    let sources = context.sources;
    let inputs = resolve_keyed_inputs(&pending.parsed, context)?;
    for equality in &pending.parsed.equalities {
        for key in [&equality.first, &equality.second] {
            let input = match inputs
                .get(key.input_index)
                .ok_or("join equality references an unknown input")?
            {
                ResolvedInput::Source(input) => input,
                ResolvedInput::Computed(_) if key.path.is_empty() => continue,
                ResolvedInput::Computed(_) => {
                    return Err(format!(
                        "input {} computed key equality path must be empty",
                        key.input_index
                    ));
                }
            };
            let mut path = input.path.clone();
            path.extend(key.path.iter().cloned());
            if input.singleton && !key.path.is_empty() {
                return Err(format!(
                    "input {} singleton equality path must be empty",
                    key.input_index
                ));
            }
            if !sources
                .get(input.source)
                .and_then(|source| schema_node_at(&source.schema, &path))
                .is_some_and(|node| !node.repeating && node.is_scalar())
            {
                return Err(format!(
                    "input {} equality path `{}` is not scalar",
                    key.input_index,
                    key.path.join("/")
                ));
            }
        }
    }
    let plan_sources = inputs
        .iter()
        .map(|input| {
            let input = match input {
                ResolvedInput::Source(input) => input,
                // Checks the plan's shape now; the key node itself is
                // lowered in the receiving context of its iteration.
                ResolvedInput::Computed(_) => {
                    return Ok(JoinSource::computed(COMPUTED_KEY, NodeId::default()));
                }
            };
            let mut collection = if input.source == 0 {
                Vec::new()
            } else {
                vec![
                    context
                        .source_names
                        .get(input.source)
                        .cloned()
                        .ok_or("join source has no runtime name")?,
                ]
            };
            collection.extend(input.path.iter().cloned());
            Ok(if input.singleton {
                JoinSource::singleton(collection)
            } else {
                JoinSource::new(collection)
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let root = pending.parsed.to_plan_sources(&plan_sources)?;
    validate_outputs(&root, &inputs, sources)?;
    let computed = inputs
        .iter()
        .any(|input| matches!(input, ResolvedInput::Computed(_)));
    Ok(ResolvedJoin {
        parsed: pending.parsed.clone(),
        inputs,
        root: (!computed).then_some(root),
        planned: None,
        target_path: None,
    })
}

enum EitherJoin {
    Keyed(Box<ResolvedJoin>),
    Hierarchy(ResolvedHierarchy),
}

/// Resolves the inputs of a keyed join: source feeds as today, plus at most
/// one computed key beside one repeating source collection.
fn resolve_keyed_inputs(
    parsed: &ParsedJoin,
    context: &KeyContext<'_>,
) -> Result<Vec<ResolvedInput>, String> {
    let computed = parsed
        .inputs
        .iter()
        .filter(|input| {
            context
                .edge_from
                .get(&input.input_port)
                .is_some_and(|feed| context.is_candidate(*feed))
        })
        .map(|input| input.index)
        .collect::<Vec<_>>();
    if let [first, second, ..] = computed.as_slice() {
        return Err(format!(
            "inputs {first} and {second} are both computed keys; a join admits at most one"
        ));
    }
    let mut inputs = Vec::with_capacity(parsed.inputs.len());
    for input in &parsed.inputs {
        let feed = connected_feed(input, context.edge_from)?;
        inputs.push(if computed.contains(&input.index) {
            ResolvedInput::Computed(computed::admit(input, feed, parsed.inputs.len(), context)?)
        } else {
            ResolvedInput::Source(resolve_source_input(input, feed, context.sources)?)
        });
    }
    if !computed.is_empty()
        && let Some(index) = inputs
            .iter()
            .position(|input| input.as_source().is_some_and(|input| input.singleton))
    {
        return Err(format!(
            "input {index} beside a computed key must be a repeating source collection"
        ));
    }
    Ok(inputs)
}

fn connected_feed(input: &JoinInput, edge_from: &BTreeMap<u32, u32>) -> Result<u32, String> {
    edge_from
        .get(&input.input_port)
        .copied()
        .ok_or_else(|| format!("input {} is not connected", input.index))
}

fn resolve_source_input(
    input: &JoinInput,
    feed: u32,
    sources: &[&SchemaComponent],
) -> Result<SourceInput, String> {
    let (source, path) = sources
        .iter()
        .enumerate()
        .find_map(|(index, component)| {
            component
                .ports
                .get(&feed)
                .cloned()
                .map(|path| (index, path))
        })
        .ok_or_else(|| format!("input {} is not a plain source feed", input.index))?;
    let source_node = sources
        .get(source)
        .and_then(|source| schema_node_at(&source.schema, &path));
    let singleton = source_node.is_some_and(|node| !node.repeating && node.is_scalar());
    if !singleton
        && !source_node
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err(format!(
            "input {} must be a repeating structural source or singleton scalar",
            input.index
        ));
    }
    Ok(SourceInput {
        source,
        path,
        singleton,
    })
}

fn resolve_hierarchy(
    pending: &PendingJoin,
    edge_from: &BTreeMap<u32, u32>,
    sources: &[&SchemaComponent],
) -> Result<ResolvedHierarchy, String> {
    let inputs = pending
        .parsed
        .inputs
        .iter()
        .map(|input| resolve_source_input(input, connected_feed(input, edge_from)?, sources))
        .collect::<Result<Vec<_>, String>>()?;
    let Some(first) = inputs.first() else {
        return Err("keyless hierarchy has no inputs".to_string());
    };
    if inputs.iter().any(|input| input.singleton) {
        return Err("keyless hierarchy inputs must all be repeating groups".to_string());
    }
    for pair in inputs.windows(2) {
        let [parent, child] = pair else {
            continue;
        };
        if child.source != first.source
            || child.path.len() <= parent.path.len()
            || !child.path.starts_with(&parent.path)
        {
            return Err(
                "keyless join inputs must form one strict nested source hierarchy".to_string(),
            );
        }
    }

    let mut rows = Vec::new();
    let mut fields = Vec::new();
    if let Some(port) = pending.parsed.tuple_output {
        let driver = inputs
            .last()
            .ok_or("keyless hierarchy has no driver input")?;
        rows.push((
            port,
            SourcePath {
                source: driver.source,
                path: driver.path.clone(),
            },
        ));
    }
    for input in &pending.parsed.inputs {
        let resolved = inputs
            .get(input.index)
            .ok_or("keyless hierarchy output references an unknown input")?;
        for output in &input.outputs {
            let mut path = resolved.path.clone();
            path.extend(output.path.iter().cloned());
            let source_path = SourcePath {
                source: resolved.source,
                path,
            };
            if output.path.is_empty() {
                rows.push((output.port, source_path));
            } else if sources
                .get(resolved.source)
                .and_then(|source| schema_node_at(&source.schema, &source_path.path))
                .is_some_and(|node| !node.repeating && node.is_scalar())
            {
                fields.push((output.port, source_path));
            } else {
                return Err(format!(
                    "output port {} does not project a scalar descendant",
                    output.port
                ));
            }
        }
    }
    Ok(ResolvedHierarchy { rows, fields })
}

fn validate_outputs(
    planned: &PlannedJoin,
    inputs: &[ResolvedInput],
    sources: &[&SchemaComponent],
) -> Result<(), String> {
    for output in &planned.outputs {
        let input = inputs
            .get(output.input_index)
            .and_then(ResolvedInput::as_source)
            .ok_or("join output references an unknown source input")?;
        let mut absolute = input.path.clone();
        absolute.extend(output.path.iter().cloned());
        if !output.path.is_empty()
            && !sources
                .get(input.source)
                .and_then(|source| schema_node_at(&source.schema, &absolute))
                .is_some_and(|node| !node.repeating && node.is_scalar())
        {
            return Err(format!(
                "output port {} does not project a scalar descendant",
                output.port
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
