use super::*;
use crate::{
    ExecutionContract, Graph, IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction,
    ScopeIteration,
};
use std::collections::BTreeSet;

struct Work {
    nodes: usize,
    bytes: usize,
    limit: usize,
}

impl Work {
    fn claim(&mut self, nodes: usize, bytes: usize) -> Result<(), SourceContractViolation> {
        self.nodes = self
            .nodes
            .checked_add(nodes)
            .ok_or_else(|| failure(SourceContractReason::ResourceLimit))?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| failure(SourceContractReason::ResourceLimit))?;
        if self.nodes > self.limit || self.bytes > MAX_SOURCE_CONTRACT_METADATA_BYTES {
            return Err(failure(SourceContractReason::ResourceLimit));
        }
        Ok(())
    }
}

// Binary search over at most 65,536 slots takes at most 17 comparisons.
// Reserve comparisons, query-byte bounds and possible path copies BEFORE work.
const LOOKUP_COMPARISONS: usize = 17;

fn add(left: usize, right: usize) -> Result<usize, SourceContractViolation> {
    left.checked_add(right)
        .ok_or_else(|| failure(SourceContractReason::ResourceLimit))
}
fn mul(left: usize, right: usize) -> Result<usize, SourceContractViolation> {
    left.checked_mul(right)
        .ok_or_else(|| failure(SourceContractReason::ResourceLimit))
}
fn path_bytes(path: &[String]) -> Result<usize, SourceContractViolation> {
    path.iter()
        .try_fold(0, |bytes, name| add(bytes, name.len()))
}
fn charge_lookups(
    work: &mut Work,
    lookups: usize,
    query_bytes: usize,
    other_work: usize,
    other_bytes: usize,
) -> Result<(), SourceContractViolation> {
    work.claim(
        add(mul(lookups, LOOKUP_COMPARISONS)?, other_work)?,
        add(mul(query_bytes, LOOKUP_COMPARISONS)?, other_bytes)?,
    )
}

fn prove_read(
    plan: &ClosedPrimarySourcePlan,
    path: &[String],
    frame: Option<&[String]>,
    active: &[Vec<String>],
    work: &mut Work,
) -> Result<(), SourceContractViolation> {
    let scalar_bytes = path_bytes(path)?;
    let pin_bytes = frame.map(path_bytes).transpose()?.unwrap_or(0);
    let mut lookups = path.len();
    let mut queries = scalar_bytes;
    let mut other_bytes = mul(add(scalar_bytes, pin_bytes)?, 2)?;
    if let Some(pin) = frame {
        lookups = add(lookups, pin.len())?;
        queries = add(queries, pin_bytes)?;
        other_bytes = add(
            other_bytes,
            mul(active.len(), pin.last().map_or(0, String::len))?,
        )?;
    } else {
        let first = path.first().map_or(0, String::len);
        for collection in active {
            lookups = add(lookups, add(collection.len(), 1)?)?;
            queries = add(queries, add(path_bytes(collection)?, first)?)?;
        }
        other_bytes = add(other_bytes, mul(active.len(), first)?)?;
    }
    let path_work = mul(add(path.len(), frame.map_or(0, <[String]>::len))?, 3)?;
    charge_lookups(
        work,
        lookups,
        queries,
        add(mul(active.len(), 2)?, path_work)?,
        other_bytes,
    )?;
    plan.validate_read(path, frame, active)
}

fn prove_driver(
    plan: &ClosedPrimarySourcePlan,
    active: &[Vec<String>],
    driver: &[String],
    work: &mut Work,
) -> Result<Vec<String>, SourceContractViolation> {
    let parent = active.last().map_or(&[][..], Vec::as_slice);
    let count = add(parent.len(), driver.len())?;
    let bytes = add(path_bytes(parent)?, path_bytes(driver)?)?;
    charge_lookups(work, count, bytes, add(mul(count, 2)?, 1)?, bytes)?;
    plan.validate_driver(active, driver)
}

fn charge_ancestor_inventory(
    pin: &[String],
    work: &mut Work,
) -> Result<(), SourceContractViolation> {
    let mut prefixes = 0_usize;
    let mut bytes = 0_usize;
    let mut prefix_bytes = 0_usize;
    for (index, name) in pin.iter().enumerate() {
        prefixes = add(prefixes, index + 1)?;
        prefix_bytes = add(prefix_bytes, name.len())?;
        bytes = add(bytes, prefix_bytes)?;
    }
    charge_lookups(work, prefixes, bytes, mul(prefixes, 3)?, bytes)
}

fn node_profile(node: &Node, body: bool, work: &mut Work) -> Result<(), SourceContractViolation> {
    use SourceContractReason::UnsupportedModel;
    work.claim(1, 0)?;
    match node {
        Node::SourceField { path, frame } if !body => {
            if path.len() > MAX_SOURCE_CONTRACT_DEPTH
                || frame
                    .as_ref()
                    .is_some_and(|pin| pin.len() > MAX_SOURCE_CONTRACT_DEPTH)
            {
                return Err(failure(SourceContractReason::ResourceLimit));
            }
            for part in path.iter().chain(frame.iter().flatten()) {
                work.claim(1, part.len())?;
            }
        }
        Node::Call { function, args } => {
            work.claim(args.len(), function.len())?;
            if matches!(
                function.as_str(),
                "sleep" | "create_guid" | "random" | "resolve_filepath"
            ) {
                return Err(failure(UnsupportedModel));
            }
        }
        Node::UserFunctionCall { args, .. } => work.claim(args.len(), 0)?,
        Node::Const { value } => work.claim(0, value_bytes(value))?,
        Node::Unconnected | Node::If { .. } | Node::Raise { .. } => {}
        Node::ValueMap { table, default, .. } => {
            work.claim(table.len(), 0)?;
            for (key, value) in table {
                work.claim(0, value_bytes(key))?;
                work.claim(0, value_bytes(value))?;
            }
            if let Some(value) = default {
                work.claim(0, value_bytes(value))?;
            }
        }
        Node::FunctionParameter { .. } if body => {}
        _ => return Err(failure(UnsupportedModel)),
    }
    Ok(())
}

fn value_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.len(),
        _ => 16,
    }
}

fn expression(
    plan: &ClosedPrimarySourcePlan,
    graph: &Graph,
    root: NodeId,
    active: &[Vec<String>],
    work: &mut Work,
) -> Result<(), SourceContractViolation> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        work.claim(1, 0)?;
        if !seen.insert(id) {
            continue;
        }
        let node = graph
            .nodes
            .get(&id)
            .ok_or_else(|| failure(SourceContractReason::UnsupportedModel))?;
        if let Node::SourceField { path, frame } = node {
            prove_read(plan, path, frame.as_deref(), active, work)?;
        }
        let mut status = Ok(());
        node.visit_inputs(&mut |input| {
            if status.is_ok() {
                status = work.claim(1, 0);
                if status.is_ok() {
                    pending.push(input.node);
                }
            }
        });
        status?;
    }
    Ok(())
}

impl Project {
    /// Re-proves the entire current guarded model. No persisted proof or list of
    /// alleged protected nodes is trusted; None leaves legacy behavior untouched.
    pub fn validate_execution_contract(
        &self,
    ) -> Result<Option<ClosedPrimarySourcePlan>, SourceContractViolation> {
        self.validate_execution_contract_with_work_limit(MAX_SOURCE_CONTRACT_VISITS)
    }

    fn validate_execution_contract_with_work_limit(
        &self,
        limit: usize,
    ) -> Result<Option<ClosedPrimarySourcePlan>, SourceContractViolation> {
        let Some(ExecutionContract::ClosedPrimarySourceV1) = self.execution_contract else {
            return Ok(None);
        };
        if !self.extra_sources.is_empty()
            || !self.failure_rules.is_empty()
            || self.source_options.external_source.is_some()
            || self.source_options.local_xml_file_set
        {
            return Err(failure(SourceContractReason::UnsupportedModel));
        }
        let plan = ClosedPrimarySourcePlan::compile(&self.source)?;
        let mut work = Work {
            nodes: 0,
            bytes: 0,
            limit: limit.min(MAX_SOURCE_CONTRACT_VISITS),
        };
        if self.graph.nodes.len() > MAX_SOURCE_CONTRACT_SHAPE_NODES
            || self.user_functions.len() > MAX_SOURCE_CONTRACT_SHAPE_NODES
        {
            return Err(failure(SourceContractReason::ResourceLimit));
        }
        for node in self.graph.nodes.values() {
            node_profile(node, false, &mut work)?;
            if let Node::SourceField { path, frame } = node {
                // Even unused source nodes must have an exact declared scalar
                // identity. Receiving availability is separately proved per use.
                let mut ancestors = Vec::new();
                if let Some(pin) = frame {
                    charge_ancestor_inventory(pin, &mut work)?;
                    for length in 1..=pin.len() {
                        if plan
                            .exact(&pin[..length])
                            .is_some_and(|index| plan.nodes[index].repeating)
                        {
                            ancestors.push(pin[..length].to_vec());
                        }
                    }
                }
                prove_read(&plan, path, frame.as_deref(), &ancestors, &mut work)?;
            }
        }
        for function in self.user_functions.values() {
            work.claim(function.parameters.len(), function.name.len())?;
            work.claim(0, function.library.len())?;
            work.claim(0, function.output_name.len())?;
            if let Some(description) = &function.description {
                work.claim(0, description.len())?;
            }
            for parameter in &function.parameters {
                work.claim(0, parameter.name.len())?;
            }
            if function.body.nodes.len() > MAX_SOURCE_CONTRACT_SHAPE_NODES {
                return Err(failure(SourceContractReason::ResourceLimit));
            }
            for node in function.body.nodes.values() {
                node_profile(node, true, &mut work)?;
            }
        }
        enum Visit<'a> {
            Enter(&'a Scope, usize),
            Exit(usize),
        }
        work.claim(add(self.extra_targets.len(), 1)?, 0)?;
        let mut pending = self
            .extra_targets
            .iter()
            .rev()
            .map(|target| Visit::Enter(&target.root, 0))
            .collect::<Vec<_>>();
        pending.push(Visit::Enter(&self.root, 0));
        let mut active = Vec::new();
        while let Some(visit) = pending.pop() {
            let Visit::Enter(scope, depth) = visit else {
                if let Visit::Exit(length) = visit {
                    active.truncate(length);
                }
                continue;
            };
            work.claim(1, scope.target_field.len())?;
            if depth > MAX_SOURCE_CONTRACT_DEPTH {
                return Err(failure(SourceContractReason::ResourceLimit));
            }
            if scope.construction != ScopeConstruction::Constructed
                || scope.filter.is_some()
                || scope.post_group_filter.is_some()
                || scope.has_grouping()
                || scope.has_sort()
                || !scope.windows.is_empty()
                || !scope.dynamic_bindings.is_empty()
                || !scope.dynamic_children.is_empty()
                || scope.merge_dynamic_fields
                || scope.iteration_output != IterationOutput::Repeated
            {
                return Err(failure(SourceContractReason::UnsupportedModel));
            }
            let previous = active.len();
            match &scope.iteration {
                ScopeIteration::None => {}
                ScopeIteration::Source(driver) => {
                    if driver.len() != 1 {
                        return Err(failure(SourceContractReason::UnprovedOwner));
                    }
                    work.claim(driver.len(), path_bytes(driver)?)?;
                    active.push(prove_driver(&plan, &active, driver, &mut work)?);
                }
                _ => return Err(failure(SourceContractReason::UnsupportedModel)),
            }
            work.claim(scope.bindings.len(), 0)?;
            for binding in &scope.bindings {
                expression(&plan, &self.graph, binding.node, &active, &mut work)?;
            }
            work.claim(add(scope.children.len(), 1)?, 0)?;
            pending.push(Visit::Exit(previous));
            pending.extend(
                scope
                    .children
                    .iter()
                    .rev()
                    .map(|child| Visit::Enter(child, depth + 1)),
            );
        }
        Ok(Some(plan))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SchemaNode {
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Root", ScalarType::String),
                SchemaNode::group(
                    "A",
                    vec![
                        SchemaNode::group(
                            "B",
                            vec![
                                SchemaNode::group(
                                    "C",
                                    vec![SchemaNode::scalar("Text", ScalarType::String)],
                                )
                                .repeating(),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        )
    }

    fn work(limit: usize) -> Work {
        Work {
            nodes: 0,
            bytes: 0,
            limit,
        }
    }

    #[test]
    fn owner_queries_reserve_lookup_and_copy_work_before_touching_the_schema() {
        let plan = ClosedPrimarySourcePlan::compile(&source()).unwrap();
        let active = vec![
            vec!["A".into()],
            vec!["A".into(), "B".into()],
            vec!["A".into(), "B".into(), "C".into()],
        ];
        let path = vec!["Root".into()];
        plan.validate_read(&path, None, &active).unwrap();
        plan.lookups.set(0);
        // 10 possible child lookups * 17 comparisons + nine path/frame units.
        assert_eq!(
            prove_read(&plan, &path, None, &active, &mut work(178))
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
        assert_eq!(plan.lookups.get(), 0);
        prove_read(&plan, &path, None, &active, &mut work(179)).unwrap();
        assert_eq!(plan.lookups.get(), 10);
        plan.lookups.set(0);
        // Three possible lookups plus the returned absolute-path copy.
        assert_eq!(
            prove_driver(&plan, &active[..2], &["C".into()], &mut work(57))
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
        assert_eq!(plan.lookups.get(), 0);
        assert_eq!(
            prove_driver(&plan, &active[..2], &["C".into()], &mut work(58)).unwrap(),
            active[2]
        );
    }

    #[test]
    fn owner_queries_reserve_exact_metadata_bytes_before_schema_traversal_or_path_copy() {
        let plan = ClosedPrimarySourcePlan::compile(&source()).unwrap();
        let active = vec![
            vec!["A".into()],
            vec!["A".into(), "B".into()],
            vec!["A".into(), "B".into(), "C".into()],
        ];
        let path = vec!["Root".into()];
        // 22 possible query bytes * 17 comparisons + 20 predicate/copy bytes.
        const READ_BYTES: usize = 394;
        let mut below = work(179);
        below.bytes = MAX_SOURCE_CONTRACT_METADATA_BYTES - READ_BYTES + 1;
        assert_eq!(
            prove_read(&plan, &path, None, &active, &mut below)
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
        assert_eq!(plan.lookups.get(), 0);
        let mut exact = work(179);
        exact.bytes = MAX_SOURCE_CONTRACT_METADATA_BYTES - READ_BYTES;
        prove_read(&plan, &path, None, &active, &mut exact).unwrap();
        assert_eq!(exact.bytes, MAX_SOURCE_CONTRACT_METADATA_BYTES);
        assert_eq!(exact.nodes, 179);
        assert_eq!(plan.lookups.get(), 10);

        plan.lookups.set(0);
        // Three query bytes * 17 comparisons + three returned-path copy bytes.
        const DRIVER_BYTES: usize = 54;
        let driver = vec!["C".into()];
        let mut below = work(58);
        below.bytes = MAX_SOURCE_CONTRACT_METADATA_BYTES - DRIVER_BYTES + 1;
        assert_eq!(
            prove_driver(&plan, &active[..2], &driver, &mut below)
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
        assert_eq!(plan.lookups.get(), 0);
        let mut exact = work(58);
        exact.bytes = MAX_SOURCE_CONTRACT_METADATA_BYTES - DRIVER_BYTES;
        assert_eq!(
            prove_driver(&plan, &active[..2], &driver, &mut exact).unwrap(),
            active[2]
        );
        assert_eq!(exact.bytes, MAX_SOURCE_CONTRACT_METADATA_BYTES);
        assert_eq!(exact.nodes, 58);
        assert_eq!(plan.lookups.get(), 3);
    }

    #[test]
    fn repeated_deep_bindings_share_one_static_ownership_work_limit() {
        let mut leaf = Scope {
            target_field: "C".into(),
            iteration: ScopeIteration::Source(vec!["C".into()]),
            bindings: vec![
                crate::Binding {
                    target_field: "Result".into(),
                    node: 0,
                },
                crate::Binding {
                    target_field: "Copy".into(),
                    node: 0,
                },
            ],
            ..Default::default()
        };
        for name in ["B", "A"] {
            leaf = Scope {
                target_field: name.into(),
                iteration: ScopeIteration::Source(vec![name.into()]),
                children: vec![leaf],
                ..Default::default()
            };
        }
        let mut target = SchemaNode::group(
            "C",
            vec![
                SchemaNode::scalar("Result", ScalarType::String),
                SchemaNode::scalar("Copy", ScalarType::String),
            ],
        )
        .repeating();
        for name in ["B", "A"] {
            target = SchemaNode::group(name, vec![target]).repeating();
        }
        let project = Project {
            execution_contract: Some(ExecutionContract::ClosedPrimarySourceV1),
            source: source(),
            target: SchemaNode::group("Output", vec![target]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [(
                    0,
                    Node::SourceField {
                        path: vec!["Root".into()],
                        frame: None,
                    },
                )]
                .into(),
            },
            root: Scope {
                children: vec![leaf],
                ..Default::default()
            },
        };
        project
            .validate_execution_contract_with_work_limit(1_000)
            .unwrap();
        assert_eq!(
            project
                .validate_execution_contract_with_work_limit(400)
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
    }
}
