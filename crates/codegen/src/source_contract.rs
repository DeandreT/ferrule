//! Reconstruct only the closed, context-preserving subset and use mapping's
//! authoritative owner proof. Unsupported IR fields are refused, never erased.
use std::collections::BTreeMap;
use std::io::{self, Write};

use mapping::{
    ClosedPrimarySourcePlan, Graph, MAX_SOURCE_CONTRACT_DEPTH, MAX_SOURCE_CONTRACT_METADATA_BYTES,
    MAX_SOURCE_CONTRACT_SHAPE_NODES, MAX_SOURCE_CONTRACT_VISITS, Node,
    SourceContractReason as Reason, SourceContractViolation,
};

use crate::{
    Expression, ExpressionNode, IterationOutput, IterationSource, Program, ProgramValidationError,
    TargetConstruction, TargetScope,
};

fn violation(reason: Reason) -> SourceContractViolation {
    SourceContractViolation {
        path: Vec::new(),
        reason,
    }
}

#[derive(Default)]
struct Budget {
    bytes: usize,
    visits: usize,
}

impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|bytes| *bytes <= MAX_SOURCE_CONTRACT_METADATA_BYTES)
            .ok_or_else(|| io::Error::other("source contract metadata limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Budget {
    fn claim(&mut self, count: usize) -> Result<(), SourceContractViolation> {
        self.visits = self
            .visits
            .checked_add(count)
            .filter(|count| *count <= MAX_SOURCE_CONTRACT_VISITS)
            .ok_or_else(|| violation(Reason::ResourceLimit))?;
        Ok(())
    }
    fn serialized(&mut self, value: &impl serde::Serialize) -> Result<(), SourceContractViolation> {
        serde_json::to_writer(self, value).map_err(|_| violation(Reason::ResourceLimit))
    }
}

fn schema_metadata(
    schema: &ir::SchemaNode,
    budget: &mut Budget,
) -> Result<(), SourceContractViolation> {
    let mut pending = vec![(schema, 0)];
    let mut nodes = 0_usize;
    while let Some((node, depth)) = pending.pop() {
        nodes = nodes
            .checked_add(1)
            .ok_or_else(|| violation(Reason::ResourceLimit))?;
        budget.claim(1)?;
        if depth > MAX_SOURCE_CONTRACT_DEPTH || nodes > MAX_SOURCE_CONTRACT_SHAPE_NODES {
            return Err(violation(Reason::ResourceLimit));
        }
        if let ir::SchemaKind::Group {
            children, dynamic, ..
        } = &node.kind
        {
            let count = children
                .len()
                .checked_add(usize::from(dynamic.is_some()))
                .and_then(|count| count.checked_add(pending.len()))
                .and_then(|count| count.checked_add(nodes))
                .ok_or_else(|| violation(Reason::ResourceLimit))?;
            if count > MAX_SOURCE_CONTRACT_SHAPE_NODES {
                return Err(violation(Reason::ResourceLimit));
            }
            pending.extend(children.iter().map(|child| (child, depth + 1)));
            pending.extend(dynamic.as_deref().map(|child| (child, depth + 1)));
        }
    }
    budget.serialized(schema)
}

fn graph(nodes: &[ExpressionNode], budget: &mut Budget) -> Result<Graph, SourceContractViolation> {
    if nodes.len() > MAX_SOURCE_CONTRACT_SHAPE_NODES {
        return Err(violation(Reason::ResourceLimit));
    }
    budget.claim(nodes.len())?;
    let mut graph = Graph::default();
    for expression in nodes {
        let node = match &expression.expression {
            Expression::SourceField { path, frame } => {
                budget.claim(path.len())?;
                budget.claim(frame.as_ref().map_or(0, Vec::len))?;
                if path.len() > MAX_SOURCE_CONTRACT_DEPTH
                    || frame
                        .as_ref()
                        .is_some_and(|pin| pin.len() > MAX_SOURCE_CONTRACT_DEPTH)
                {
                    return Err(violation(Reason::ResourceLimit));
                }
                budget.serialized(path)?;
                budget.serialized(frame)?;
                Node::SourceField {
                    path: path.clone(),
                    frame: frame.clone(),
                }
            }
            Expression::Const { value } => {
                budget.serialized(value)?;
                Node::Const {
                    value: value.clone(),
                }
            }
            Expression::FunctionParameter { parameter } => Node::FunctionParameter {
                parameter: *parameter,
            },
            Expression::Call { function, args } => {
                budget.claim(args.len())?;
                budget.serialized(args)?;
                Node::Call {
                    function: function.as_str().to_owned(),
                    args: args.clone(),
                }
            }
            Expression::UserFunctionCall { function, args } => {
                budget.claim(args.len())?;
                budget.serialized(args)?;
                Node::UserFunctionCall {
                    function: *function,
                    args: args.clone(),
                }
            }
            Expression::If {
                condition,
                then,
                else_,
            } => Node::If {
                condition: *condition,
                then: *then,
                else_: *else_,
            },
            Expression::Raise { message } => Node::Raise { message: *message },
            Expression::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                budget.claim(table.len())?;
                budget.serialized(table)?;
                budget.serialized(default)?;
                Node::ValueMap {
                    input: *input,
                    input_type: *input_type,
                    table: table.clone(),
                    default: default.clone(),
                }
            }
            _ => return Err(violation(Reason::UnsupportedModel)),
        };
        if graph.nodes.insert(expression.id, node).is_some() {
            return Err(violation(Reason::UnsupportedModel));
        }
    }
    Ok(graph)
}

fn scope(
    value: &TargetScope,
    depth: usize,
    budget: &mut Budget,
) -> Result<mapping::Scope, SourceContractViolation> {
    if depth > MAX_SOURCE_CONTRACT_DEPTH {
        return Err(violation(Reason::ResourceLimit));
    }
    budget.claim(1)?;
    budget.claim(value.children.len())?;
    budget.claim(value.bindings.len())?;
    budget.serialized(&value.target_field)?;
    if value.construction != TargetConstruction::Group {
        return Err(violation(Reason::UnsupportedModel));
    }
    let iteration = match &value.iteration {
        None => mapping::ScopeIteration::None,
        Some(iteration) => {
            if iteration.filter().is_some()
                || iteration.sort().is_some()
                || iteration.grouping().is_some()
                || iteration.post_group_filter().is_some()
                || !iteration.windows().is_empty()
                || iteration.output() != IterationOutput::Repeated
            {
                return Err(violation(Reason::UnsupportedModel));
            }
            let IterationSource::Source(driver) = iteration.input() else {
                return Err(violation(Reason::UnsupportedModel));
            };
            if driver.path().len() > MAX_SOURCE_CONTRACT_DEPTH {
                return Err(violation(Reason::ResourceLimit));
            }
            budget.serialized(&driver.path())?;
            mapping::ScopeIteration::Source(driver.path().to_vec())
        }
    };
    let bindings = value
        .bindings
        .iter()
        .map(|binding| {
            budget.serialized(&binding.target_field)?;
            Ok(mapping::Binding {
                target_field: binding.target_field.clone(),
                node: binding.expression,
            })
        })
        .collect::<Result<_, SourceContractViolation>>()?;
    let children = value
        .children
        .iter()
        .map(|child| scope(child, depth + 1, budget))
        .collect::<Result<_, _>>()?;
    // Scalar coercion/cardinality on a static output binding does not change
    // its receiving source context. Those fields remain in Program and are
    // checked by the normal target validator, not this source-owner proof.
    Ok(mapping::Scope {
        target_field: value.target_field.clone(),
        iteration,
        bindings,
        children,
        ..Default::default()
    })
}

impl Program {
    /// Re-proves the whole current public Program, including unused expressions,
    /// unselected targets and every isolated body. Plans are never persisted.
    pub fn validate_execution_contract(
        &self,
    ) -> Result<Option<ClosedPrimarySourcePlan>, ProgramValidationError> {
        self.prove_source_contract()
            .map_err(ProgramValidationError::SourceContractViolation)
    }

    fn prove_source_contract(
        &self,
    ) -> Result<Option<ClosedPrimarySourcePlan>, SourceContractViolation> {
        match self.execution_contract {
            None => return Ok(None),
            Some(mapping::ExecutionContract::ClosedPrimarySourceV1) => {}
        }
        if !self.extra_sources.is_empty() || !self.failure_rules.is_empty() {
            return Err(violation(Reason::UnsupportedModel));
        }
        // Establish depth/shape bounds before cloning or serializing the schema.
        ClosedPrimarySourcePlan::compile(&self.source)?;
        let mut budget = Budget::default();
        budget.serialized(&self.source)?;
        // Normal target validation still reads the original complete schemas.
        // Bound its recursive metadata work before entering that validator.
        schema_metadata(&self.target, &mut budget)?;
        budget.claim(self.extra_targets.len())?;
        for target in &self.extra_targets {
            schema_metadata(&target.target, &mut budget)?;
        }
        budget.claim(self.user_functions.len())?;
        if self.user_functions.len() > MAX_SOURCE_CONTRACT_SHAPE_NODES {
            return Err(violation(Reason::ResourceLimit));
        }
        let main_graph = graph(&self.expressions, &mut budget)?;
        let root = scope(&self.root, 0, &mut budget)?;
        let mut user_functions = BTreeMap::new();
        for function in &self.user_functions {
            budget.claim(function.parameters.len())?;
            budget.serialized(&function.library)?;
            budget.serialized(&function.name)?;
            let body = graph(&function.expressions, &mut budget)?;
            let parameters = function
                .parameters
                .iter()
                .map(|parameter| mapping::FunctionParameter {
                    id: parameter.id,
                    name: String::new(),
                    ty: parameter.ty,
                })
                .collect();
            if user_functions
                .insert(
                    function.id,
                    mapping::UserFunction {
                        library: function.library.clone(),
                        name: function.name.clone(),
                        description: None,
                        parameters,
                        output_name: String::new(),
                        output_type: function.output_type,
                        body,
                        output: function.output,
                    },
                )
                .is_some()
            {
                return Err(violation(Reason::UnsupportedModel));
            }
        }
        let extra_targets = self
            .extra_targets
            .iter()
            .map(|target| {
                budget.serialized(&target.name)?;
                Ok(mapping::NamedTarget {
                    name: target.name.clone(),
                    path: None,
                    options: Default::default(),
                    // Target schemas have no role in source-owner validation; the
                    // complete original schemas stay under normal Program validation.
                    schema: ir::SchemaNode::group("Output", Vec::new()),
                    root: scope(&target.root, 0, &mut budget)?,
                })
            })
            .collect::<Result<_, SourceContractViolation>>()?;
        let project = mapping::Project {
            execution_contract: self.execution_contract,
            source: self.source.clone(),
            target: ir::SchemaNode::group("Output", Vec::new()),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets,
            failure_rules: Vec::new(),
            user_functions,
            graph: main_graph,
            root,
        };
        project.validate_execution_contract()
    }
}
