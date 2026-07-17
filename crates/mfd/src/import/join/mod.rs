use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;
use mapping::{
    IterationOutput, JoinConditions as MappingJoinConditions, JoinId, JoinKey as MappingJoinKey,
    JoinPlan, JoinSource, Node as MappingNode, NodeId,
};
use roxmltree::Node as XmlNode;

use super::graph::GraphBuilder;
use super::group_projection::TargetIteration;
use super::iteration::split_at_innermost_repeating;
use super::schema::{SchemaComponent, normalize_xml_entry_name, parse_u32, schema_node_at};
use super::scope::ScopeBuilder;
use super::source::SourcePath;

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
        let collections = sources
            .iter()
            .map(|source| source.collection().to_vec())
            .collect::<Vec<_>>();
        let mut plan = JoinPlan::new(
            sources[0].clone(),
            sources[1].clone(),
            self.stage_conditions(1, &collections)?,
        )
        .map_err(|error| error.to_string())?;
        for (stage, source) in sources.iter().enumerate().skip(2) {
            plan = plan
                .then(source.clone(), self.stage_conditions(stage, &collections)?)
                .map_err(|error| error.to_string())?;
        }
        let outputs = self
            .inputs
            .iter()
            .flat_map(|input| {
                let collection = collections[input.index].clone();
                input.outputs.iter().map(move |output| PlannedJoinOutput {
                    port: output.port,
                    input_index: input.index,
                    collection: collection.clone(),
                    path: output.path.clone(),
                })
            })
            .collect();
        Ok(PlannedJoin {
            plan,
            tuple_output: self.tuple_output,
            outputs,
        })
    }

    fn stage_conditions(
        &self,
        stage: usize,
        collections: &[Vec<String>],
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
            Some(MappingJoinKey::new(
                collections[left.input_index].clone(),
                left.path.clone(),
                right.path.clone(),
            ))
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

#[derive(Default)]
pub(super) struct Registry {
    joins: BTreeMap<JoinId, ResolvedJoin>,
    row_outputs: BTreeMap<u32, JoinId>,
    field_outputs: BTreeMap<u32, ResolvedField>,
    rejected_row_outputs: BTreeSet<u32>,
    rejected_field_outputs: BTreeSet<u32>,
}

struct ResolvedJoin {
    parsed: ParsedJoin,
    inputs: Vec<ResolvedInput>,
    root: PlannedJoin,
    planned: Option<PlannedJoin>,
    target_path: Option<Vec<String>>,
}

struct ResolvedInput {
    source: usize,
    path: Vec<String>,
    singleton: bool,
}

struct ResolvedField {
    join: JoinId,
    input_index: usize,
    path: Vec<String>,
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

    pub(super) fn resolve(
        self,
        edge_from: &BTreeMap<u32, u32>,
        sources: &[&SchemaComponent],
        source_names: &[String],
        warnings: &mut Vec<String>,
    ) -> Registry {
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
            match resolve_join(&pending, edge_from, sources, source_names) {
                Ok(resolved) => registry.insert(pending.id, resolved),
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

    fn planned(&self, id: JoinId) -> Option<&PlannedJoin> {
        self.joins.get(&id)?.planned.as_ref()
    }

    fn activate_root_plan(&mut self, id: JoinId) -> Option<JoinPlan> {
        let join = self.joins.get_mut(&id)?;
        if let Some(planned) = &join.planned
            && planned.plan != join.root.plan
        {
            return None;
        }
        join.planned.get_or_insert_with(|| join.root.clone());
        Some(join.root.plan.clone())
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
        match self
            .joins
            .prepare(id, target_path, &anchor, self.sources, self.source_names)
        {
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
            let plan = self.joins.activate_root_plan(join).ok_or_else(|| {
                "joined sequence is owned by an incompatible nested context".to_string()
            })?;
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
        let plan = self.joins.activate_root_plan(join).ok_or_else(|| {
            "joined sequence is owned by an incompatible nested context".to_string()
        })?;
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

fn resolve_join(
    pending: &PendingJoin,
    edge_from: &BTreeMap<u32, u32>,
    sources: &[&SchemaComponent],
    source_names: &[String],
) -> Result<ResolvedJoin, String> {
    let mut inputs = Vec::with_capacity(pending.parsed.inputs.len());
    for input in &pending.parsed.inputs {
        let feed = edge_from
            .get(&input.input_port)
            .copied()
            .ok_or_else(|| format!("input {} is not connected", input.index))?;
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
        let singleton = source_node
            .is_some_and(|node| !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }));
        if !singleton
            && !source_node
                .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        {
            return Err(format!(
                "input {} must be a repeating structural source or singleton scalar",
                input.index
            ));
        }
        inputs.push(ResolvedInput {
            source,
            path,
            singleton,
        });
    }
    for equality in &pending.parsed.equalities {
        for key in [&equality.first, &equality.second] {
            let input = inputs
                .get(key.input_index)
                .ok_or("join equality references an unknown input")?;
            let mut path = input.path.clone();
            path.extend(key.path.iter().cloned());
            if inputs[key.input_index].singleton && !key.path.is_empty() {
                return Err(format!(
                    "input {} singleton equality path must be empty",
                    key.input_index
                ));
            }
            if !sources
                .get(input.source)
                .and_then(|source| schema_node_at(&source.schema, &path))
                .is_some_and(|node| {
                    !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                })
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
            let mut collection = if input.source == 0 {
                Vec::new()
            } else {
                vec![
                    source_names
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
    Ok(ResolvedJoin {
        parsed: pending.parsed.clone(),
        inputs,
        root,
        planned: None,
        target_path: None,
    })
}

fn validate_outputs(
    planned: &PlannedJoin,
    inputs: &[ResolvedInput],
    sources: &[&SchemaComponent],
) -> Result<(), String> {
    for output in &planned.outputs {
        let input = inputs
            .get(output.input_index)
            .ok_or("join output references an unknown input")?;
        let mut absolute = input.path.clone();
        absolute.extend(output.path.iter().cloned());
        if !output.path.is_empty()
            && !sources
                .get(input.source)
                .and_then(|source| schema_node_at(&source.schema, &absolute))
                .is_some_and(|node| {
                    !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                })
        {
            return Err(format!(
                "output port {} does not project a scalar descendant",
                output.port
            ));
        }
    }
    Ok(())
}

fn declared_output_ports(component: XmlNode<'_, '_>) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut rows = BTreeSet::new();
    let mut fields = BTreeSet::new();
    let Some(tuple) = component
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("tuple"))
    else {
        return (rows, fields);
    };
    if let Some(port) = tuple
        .attribute("outkey")
        .and_then(|value| value.parse().ok())
    {
        rows.insert(port);
    }
    for branch in tuple.children().filter(|node| node.has_tag_name("entry")) {
        let Some(root) = branch.children().find(|node| node.has_tag_name("entry")) else {
            continue;
        };
        for output in root.descendants().filter(|node| node.has_tag_name("entry")) {
            let Some(port) = output
                .attribute("outkey")
                .and_then(|value| value.parse().ok())
            else {
                continue;
            };
            if output == root {
                rows.insert(port);
            } else {
                fields.insert(port);
            }
        }
    }
    (rows, fields)
}

pub(super) fn parse(component: &XmlNode<'_, '_>) -> Result<ParsedJoin, String> {
    if component.attribute("library") != Some("core") || component.attribute("kind") != Some("32") {
        return Err("component is not a core kind=32 join".to_string());
    }
    let data = one_child(*component, "data", "join component")?;
    let root = one_child(data, "root", "join data")?;
    let document = one_named_entry(root, "document", "join root")?;
    let tuple = one_named_entry(document, "tuple", "join document")?;
    let tuple_output = optional_port(tuple, "outkey", "join tuple")?;
    let inputs = parse_inputs(tuple)?;

    let join = one_child(data, "join", "join data")?;
    ensure_empty_custom_conditions(join)?;
    let keypaths = one_child(join, "keypaths", "join metadata")?;
    let paths = parse_keypaths(keypaths)?;
    let joinkeys = one_child(join, "joinkeys", "join metadata")?;
    let equalities = parse_equalities(joinkeys, &paths, inputs.len())?;

    validate_ports(tuple_output, &inputs)?;
    Ok(ParsedJoin {
        tuple_output,
        inputs,
        equalities,
    })
}

fn parse_inputs(tuple: XmlNode<'_, '_>) -> Result<Vec<JoinInput>, String> {
    let mut branches = BTreeMap::new();
    for branch in tuple.children().filter(|node| node.has_tag_name("entry")) {
        let name = branch.attribute("name").unwrap_or_default();
        let index = name
            .strip_prefix("dynamic_tree_node")
            .filter(|suffix| !suffix.is_empty())
            .and_then(|suffix| suffix.parse::<usize>().ok())
            .ok_or_else(|| format!("join tuple has unsupported branch `{name}`"))?;
        if branches.insert(index, branch).is_some() {
            return Err(format!("join tuple repeats input index {index}"));
        }
    }
    if branches.len() < 2 {
        return Err("join must declare at least two inputs".to_string());
    }
    for (expected, actual) in branches.keys().copied().enumerate() {
        if actual != expected {
            return Err(format!(
                "join input indices must be contiguous from 0; expected {expected}, found {actual}"
            ));
        }
    }

    branches
        .into_iter()
        .map(|(index, branch)| parse_input(index, branch))
        .collect()
}

fn parse_input(index: usize, branch: XmlNode<'_, '_>) -> Result<JoinInput, String> {
    let roots = branch
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(format!(
            "join input {index} must contain exactly one root entry"
        ));
    };
    let input_port = required_port(*root, "inpkey", &format!("join input {index}"))?;
    let (name, _) = normalize_xml_entry_name(root.attribute("name").unwrap_or_default());
    if name.is_empty() {
        return Err(format!("join input {index} has no root name"));
    }
    let mut outputs = Vec::new();
    if let Some(port) = optional_port(*root, "outkey", &format!("join input {index} root"))? {
        outputs.push(JoinOutput {
            port,
            path: Vec::new(),
        });
    }
    collect_outputs(*root, &mut Vec::new(), &mut outputs)?;
    Ok(JoinInput {
        index,
        name: name.to_string(),
        input_port,
        outputs,
    })
}

fn collect_outputs(
    entry: XmlNode<'_, '_>,
    path: &mut Vec<String>,
    outputs: &mut Vec<JoinOutput>,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let (name, _) = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        if name.is_empty() {
            return Err("join output entry has no name".to_string());
        }
        path.push(name.to_string());
        if let Some(port) = optional_port(child, "outkey", "join output entry")? {
            outputs.push(JoinOutput {
                port,
                path: path.clone(),
            });
        }
        collect_outputs(child, path, outputs)?;
        path.pop();
    }
    Ok(())
}

fn parse_keypaths(keypaths: XmlNode<'_, '_>) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let mut paths = BTreeMap::new();
    for root in keypaths
        .children()
        .filter(|node| node.has_tag_name("entry"))
    {
        collect_keypaths(root, &mut Vec::new(), &mut paths)?;
    }
    if paths.is_empty() {
        return Err("join declares no key paths".to_string());
    }
    Ok(paths)
}

fn collect_keypaths(
    entry: XmlNode<'_, '_>,
    path: &mut Vec<String>,
    paths: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    let raw_name = entry.attribute("name").unwrap_or_default();
    let (name, _) = normalize_xml_entry_name(raw_name);
    if !name.is_empty() {
        path.push(name.to_string());
    }
    if let Some(id) = optional_port(entry, "outkey", "join key path")?
        && paths.insert(id, path.clone()).is_some()
    {
        return Err(format!("join repeats key path id {id}"));
    }
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        collect_keypaths(child, path, paths)?;
    }
    if !name.is_empty() {
        path.pop();
    }
    Ok(())
}

fn parse_equalities(
    joinkeys: XmlNode<'_, '_>,
    paths: &BTreeMap<u32, Vec<String>>,
    input_count: usize,
) -> Result<Vec<JoinEquality>, String> {
    let pairs = joinkeys
        .children()
        .filter(|node| node.has_tag_name("keypair"))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Err("join must declare at least one equality condition".to_string());
    }
    let mut equalities = Vec::with_capacity(pairs.len());
    let mut used_inputs = BTreeSet::new();
    for pair in pairs {
        let first = parse_key(
            one_child(pair, "first-key", "join key pair")?,
            0,
            paths,
            input_count,
        )?;
        let second = parse_key(
            one_child(pair, "second-key", "join key pair")?,
            1,
            paths,
            input_count,
        )?;
        if first.input_index == second.input_index {
            return Err(format!(
                "join equality must compare distinct inputs; both use index {}",
                first.input_index
            ));
        }
        used_inputs.insert(first.input_index);
        used_inputs.insert(second.input_index);
        equalities.push(JoinEquality { first, second });
    }
    if used_inputs.len() != input_count {
        return Err("every join input must participate in an equality condition".to_string());
    }
    for stage in 1..input_count {
        if !equalities.iter().any(|equality| {
            equality.first.input_index == stage && equality.second.input_index < stage
                || equality.second.input_index == stage && equality.first.input_index < stage
        }) {
            return Err(format!(
                "join input {stage} must have an equality with an earlier input"
            ));
        }
    }
    Ok(equalities)
}

fn parse_key(
    key: XmlNode<'_, '_>,
    default_input: usize,
    paths: &BTreeMap<u32, Vec<String>>,
    input_count: usize,
) -> Result<JoinKey, String> {
    let path_id = required_port(key, "path-id", "join equality key")?;
    let input_index = match key.attribute("input-index") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("join equality has invalid input index `{value}`"))?,
        None => default_input,
    };
    if input_index >= input_count {
        return Err(format!(
            "join equality input index {input_index} is out of range for {input_count} inputs"
        ));
    }
    let path = paths
        .get(&path_id)
        .cloned()
        .ok_or_else(|| format!("join equality references unknown key path id {path_id}"))?;
    Ok(JoinKey {
        input_index,
        path_id,
        path,
    })
}

fn ensure_empty_custom_conditions(join: XmlNode<'_, '_>) -> Result<(), String> {
    for condition in join
        .descendants()
        .filter(|node| node.has_tag_name("condition"))
    {
        let has_content = condition.attributes().len() != 0
            || condition.children().any(|child| {
                child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty())
            });
        if has_content {
            return Err("join custom key-path conditions are not supported".to_string());
        }
    }
    Ok(())
}

fn validate_ports(tuple_output: Option<u32>, inputs: &[JoinInput]) -> Result<(), String> {
    let mut ports = BTreeSet::new();
    if let Some(port) = tuple_output {
        ports.insert(port);
    }
    for input in inputs {
        if !ports.insert(input.input_port) {
            return Err(format!("join repeats port key {}", input.input_port));
        }
        for output in &input.outputs {
            if !ports.insert(output.port) {
                return Err(format!("join repeats port key {}", output.port));
            }
        }
    }
    Ok(())
}

fn one_child<'a, 'input>(
    parent: XmlNode<'a, 'input>,
    tag: &str,
    context: &str,
) -> Result<XmlNode<'a, 'input>, String> {
    let matches = parent
        .children()
        .filter(|node| node.has_tag_name(tag))
        .collect::<Vec<_>>();
    let [node] = matches.as_slice() else {
        return Err(format!("{context} must contain exactly one <{tag}>"));
    };
    Ok(*node)
}

fn one_named_entry<'a, 'input>(
    parent: XmlNode<'a, 'input>,
    name: &str,
    context: &str,
) -> Result<XmlNode<'a, 'input>, String> {
    let matches = parent
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some(name))
        .collect::<Vec<_>>();
    let [node] = matches.as_slice() else {
        return Err(format!("{context} must contain exactly one `{name}` entry"));
    };
    Ok(*node)
}

fn required_port(node: XmlNode<'_, '_>, attribute: &str, context: &str) -> Result<u32, String> {
    let value = node
        .attribute(attribute)
        .ok_or_else(|| format!("{context} has no `{attribute}`"))?;
    value
        .parse::<u32>()
        .map_err(|_| format!("{context} has invalid `{attribute}` value `{value}`"))
}

fn optional_port(
    node: XmlNode<'_, '_>,
    attribute: &str,
    context: &str,
) -> Result<Option<u32>, String> {
    node.attribute(attribute)
        .map(|_| required_port(node, attribute, context))
        .transpose()
}

#[cfg(test)]
mod tests;
