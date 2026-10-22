use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};

use ir::{
    Instance, ScalarType, Value, XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD,
    XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use mapping::{FunctionId, Graph, Node, NodeId, UserFunction};

use crate::aggregate::aggregate;
use crate::context::{parameter_value, runtime_field};
use crate::filter_map::{FilterMapRunState, FilterMapWork};
use crate::join::{AggregateInput as JoinAggregateInput, eval_aggregate as eval_join_aggregate};
use crate::policy::{BudgetMeter, RestrictedPolicy};
use crate::resolve::{
    dynamic_scalar, field_scalar, instance_in_active_collection, instance_in_frame, join_scalar,
    scalar_in_active_collection, scalar_in_frame, source_document_path,
};
use crate::sequence::{eval_sequence_aggregate, eval_sequence_exists, eval_sequence_item_at};
use crate::source_iteration::{PositionFrame, WalkExtension, walk};
use crate::trace::{TraceSink, record_node_input_value, record_node_value};
use crate::user_function;
use crate::{DebugHook, EngineError, ExecutionPurpose};

#[derive(Clone, Copy)]
pub(crate) struct EvalProgram<'a> {
    document_filename_clock: Option<&'a dyn crate::DocumentFilenameClock>,
    document_body_clock: Option<&'a dyn crate::DocumentBodyClock>,
    evaluating_document_filename: bool,
    pub(crate) invocation_counters: Option<&'a RefCell<functions::InvocationCounters>>,
    pub(crate) graph: &'a Graph,
    pub(crate) user_functions: &'a BTreeMap<FunctionId, UserFunction>,
    pub(crate) trace_sink: Option<&'a dyn TraceSink>,
    pub(crate) debug_hook: Option<&'a dyn DebugHook>,
    pub(crate) first_failure_reported: &'a Cell<bool>,
    purpose: ExecutionPurpose,
    primary_source: Option<&'a Instance>,
    pub(crate) owned_context: Option<&'a crate::owned_context::OwnedContext<'a>>,
    /// The active owned join tuple, read only by owned join reads and the
    /// flattened join position.
    pub(crate) owned_join: Option<&'a crate::owned_join_tuple::OwnedJoinTuple<'a>>,
    pub(crate) owned_trace_registry: Option<&'a dyn crate::trace::TraceOwnedRegistryView>,
    pub(crate) owned_trace_phase: crate::TraceOwnedPhase,
    pub(crate) filter_map_run_state: Option<&'a FilterMapRunState<'a>>,
    pub(crate) filter_map_work: Option<FilterMapWork<'a>>,
    pub(crate) restricted: Option<&'a RestrictedPolicy>,
    pub(crate) budget: Option<&'a BudgetMeter>,
}

impl<'a> EvalProgram<'a> {
    pub(crate) fn new(
        graph: &'a Graph,
        user_functions: &'a BTreeMap<FunctionId, UserFunction>,
        trace_sink: Option<&'a dyn TraceSink>,
        first_failure_reported: &'a Cell<bool>,
    ) -> Self {
        Self {
            document_filename_clock: None,
            document_body_clock: None,
            evaluating_document_filename: false,
            invocation_counters: None,
            graph,
            user_functions,
            trace_sink,
            debug_hook: None,
            first_failure_reported,
            purpose: ExecutionPurpose::Run,
            primary_source: None,
            owned_context: None,
            owned_join: None,
            owned_trace_registry: None,
            owned_trace_phase: crate::TraceOwnedPhase::Structural,
            filter_map_run_state: None,
            filter_map_work: None,
            restricted: None,
            budget: None,
        }
    }

    pub(crate) fn with_restricted_policy(
        mut self,
        policy: Option<&'a RestrictedPolicy>,
        budget: Option<&'a BudgetMeter>,
    ) -> Self {
        self.restricted = policy;
        self.budget = budget;
        self
    }

    pub(crate) fn with_invocation_counters(
        mut self,
        counters: Option<&'a RefCell<functions::InvocationCounters>>,
    ) -> Self {
        self.invocation_counters = counters;
        self
    }

    pub(crate) fn with_document_filename_clock(
        mut self,
        clock: Option<&'a dyn crate::DocumentFilenameClock>,
    ) -> Self {
        self.document_filename_clock = clock;
        self
    }

    pub(crate) fn for_document_filename(mut self) -> Self {
        self.evaluating_document_filename = true;
        self
    }

    pub(crate) fn with_document_body_clock(
        mut self,
        clock: Option<&'a dyn crate::DocumentBodyClock>,
    ) -> Self {
        self.document_body_clock = clock;
        self
    }

    pub(crate) fn with_debug_hook(mut self, hook: Option<&'a dyn DebugHook>) -> Self {
        self.debug_hook = hook;
        self
    }

    pub(crate) fn with_purpose(mut self, purpose: ExecutionPurpose) -> Self {
        self.purpose = purpose;
        self
    }

    pub(crate) fn with_primary_source(mut self, source: &'a Instance) -> Self {
        self.primary_source = Some(source);
        self
    }

    pub(crate) fn with_filter_map_run_state(mut self, state: &'a FilterMapRunState<'a>) -> Self {
        self.filter_map_run_state = Some(state);
        self
    }

    pub(crate) fn with_filter_map_work(mut self, work: FilterMapWork<'a>) -> Self {
        self.filter_map_work = Some(work);
        self
    }

    pub(crate) fn with_owned_context<'b>(
        self,
        context: &'b crate::owned_context::OwnedContext<'b>,
    ) -> EvalProgram<'b>
    where
        'a: 'b,
    {
        EvalProgram {
            owned_context: Some(context),
            ..self
        }
    }

    pub(crate) fn with_owned_join<'b>(
        self,
        tuple: &'b crate::owned_join_tuple::OwnedJoinTuple<'b>,
    ) -> EvalProgram<'b>
    where
        'a: 'b,
    {
        EvalProgram {
            owned_join: Some(tuple),
            ..self
        }
    }

    pub(crate) fn with_owned_trace_registry(
        mut self,
        registry: &'a dyn crate::trace::TraceOwnedRegistryView,
    ) -> Self {
        self.owned_trace_registry = Some(registry);
        self
    }

    pub(crate) fn with_owned_trace_phase(mut self, phase: crate::TraceOwnedPhase) -> Self {
        self.owned_trace_phase = phase;
        self
    }

    pub(crate) fn without_owned_context(self) -> Self {
        Self {
            owned_context: None,
            owned_trace_registry: None,
            ..self
        }
    }

    pub(super) fn purpose(self) -> ExecutionPurpose {
        self.purpose
    }

    pub(super) fn primary_source(self) -> Option<&'a Instance> {
        self.primary_source
    }
}

pub(crate) fn eval_expr(
    program: EvalProgram<'_>,
    node_id: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let work = program
        .filter_map_work
        .map(|work| work.at_node(None, node_id));
    // Restricted budgets are charged before the node evaluates and before any
    // filter/map charge, so a refused node has no value or side effects.
    let result = program
        .budget
        .map(BudgetMeter::enter_node)
        .transpose()
        .and_then(|_depth| {
            work.map_or(Ok(()), FilterMapWork::charge)
                .and_then(|()| eval_expr_inner(program, node_id, context, positions, in_progress))
                .map_err(|error| match work {
                    Some(work) => work.wrap(error),
                    None => error,
                })
        });
    if let Err(error) = &result {
        crate::debug::after_node_failure(
            program,
            node_id,
            error,
            positions,
            context,
            program.primary_source,
        )?;
    }
    result
}

/// Legacy and owned serializers share one writer and its failure category.
fn serialize_xml(
    node: NodeId,
    schema: &ir::SchemaNode,
    instance: &Instance,
    declaration: bool,
    indent: bool,
    namespace: Option<&str>,
) -> Result<Value, EngineError> {
    let options = format_xml::XmlWriteOptions {
        declaration,
        indent,
        default_namespace: namespace.map(str::to_owned),
        schema_hints: None,
    };
    format_xml::to_string_with_options(schema, instance, &options)
        .map(Value::String)
        .map_err(|error| EngineError::XmlSerialization {
            node,
            message: error.to_string(),
        })
}

/// Owned element reads report the same typed categories as owned fields.
fn owned_access_error(node: NodeId, source: crate::OwnedSourceError) -> EngineError {
    match source {
        crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: Some(node),
                function: None,
                scope: None,
                context: if found == "a document set" {
                    crate::OwnedCapabilityContext::Document
                } else {
                    crate::OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source,
        },
    }
}

fn eval_expr_inner(
    program: EvalProgram<'_>,
    node_id: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    if !in_progress.insert(node_id) {
        return Err(EngineError::Cycle(node_id));
    }

    let node = program
        .graph
        .nodes
        .get(&node_id)
        .ok_or(EngineError::MissingNode(node_id))?;

    let result = match node {
        Node::OwnedSourceField(field) => match program.owned_context {
            Some(context) => context.field(field).map_err(|source| match source {
                crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
                    EngineError::UnsupportedOwnedContext {
                        node: Some(node_id),
                        function: None,
                        scope: None,
                        context: if found == "a document set" {
                            crate::OwnedCapabilityContext::Document
                        } else {
                            crate::OwnedCapabilityContext::GeneratedItem
                        },
                    }
                }
                source => EngineError::OwnedSourceAccess {
                    node: Some(node_id),
                    scope: None,
                    source,
                },
            }),
            None => Err(EngineError::UnsupportedSourceOwnership {
                node: node_id,
                function: None,
                kind: "owned_source_field",
            }),
        },
        Node::OwnedPosition(position) => match program.owned_context {
            Some(context) => {
                let slot = context
                    .position_slot(&position.owner, position.collection.as_deref())
                    .map_err(|source| EngineError::OwnedSourceAccess {
                        node: Some(node_id),
                        scope: None,
                        source,
                    })?;
                let index = positions
                    .get(slot)
                    .ok_or_else(|| EngineError::OwnedSourceAccess {
                        node: Some(node_id),
                        scope: None,
                        source: crate::OwnedSourceError::InactiveSourceFrame {
                            owner: position.owner.clone(),
                            collection: position.collection.clone(),
                        },
                    })?
                    .index;
                i64::try_from(index)
                    .map(Value::Int)
                    .map_err(|_| EngineError::OwnedSourceAccess {
                        node: Some(node_id),
                        scope: None,
                        source: crate::OwnedSourceError::ResourceLimit {
                            resource: "position integer",
                            limit: i64::MAX as usize,
                        },
                    })
            }
            None => Err(EngineError::UnsupportedSourceOwnership {
                node: node_id,
                function: None,
                kind: "owned_position",
            }),
        },
        Node::OwnedRowDocumentPath(path) => match program.owned_context {
            Some(context) => context
                .row_document_path(&path.owner, path.collection.as_deref())
                .map_err(|source| EngineError::OwnedSourceAccess {
                    node: Some(node_id),
                    scope: None,
                    source,
                }),
            None => Err(EngineError::UnsupportedSourceOwnership {
                node: node_id,
                function: None,
                kind: "owned_row_document_path",
            }),
        },
        Node::OwnedLookup(lookup) => crate::owned_search::eval_owned_lookup(
            program,
            node_id,
            lookup,
            context,
            positions,
            in_progress,
        ),
        Node::OwnedCollectionFind(find) => crate::owned_search::eval_owned_collection_find(
            program,
            node_id,
            find,
            context,
            positions,
            in_progress,
        ),
        Node::OwnedAggregate(aggregate) => crate::owned_aggregate::eval_owned_aggregate(
            program,
            node_id,
            aggregate,
            context,
            positions,
            in_progress,
        ),
        Node::OwnedJoinField(field) => crate::owned_join_tuple::eval_field(program, node_id, field),
        Node::OwnedJoinSourcePosition(position) => {
            crate::owned_join_tuple::eval_source_position(program, node_id, position)
        }
        Node::OwnedJoinAggregate(aggregate) => {
            crate::owned_join_aggregate::eval_owned_join_aggregate(
                program,
                node_id,
                aggregate,
                context,
                positions,
                in_progress,
            )
        }
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        } => ir::primary_root_xml_type_equals(program.primary_source, canonical_expanded_type)
            .map(Value::Bool)
            .map_err(|source| EngineError::PrimaryRoot {
                node: node_id,
                source,
            }),
        Node::SourceRootField { path, required } => ir::primary_root_scalar_with_requirement(
            program.primary_source,
            &path.iter().map(String::as_str).collect::<Vec<_>>(),
            *required,
        )
        .map_err(|source| EngineError::PrimaryRoot {
            node: node_id,
            source,
        }),
        Node::SourceField { path, frame } => match frame {
            Some(frame) => {
                Ok(scalar_in_frame(context, positions, frame, path).unwrap_or(Value::Null))
            }
            None => scalar_in_active_collection(context, positions, path)
                .ok_or_else(|| EngineError::MissingSourceField(path.join("/"))),
        },
        Node::SourceDocumentPath => source_document_path(context, positions)
            .map(|path| Value::String(path.to_string()))
            .ok_or_else(|| EngineError::MissingSourceField("<document-path>".into())),
        Node::Position { collection } => Ok(Value::Int(position(positions, collection) as i64)),
        Node::JoinField {
            join,
            collection,
            path,
        } => join_scalar(context, positions, *join, collection, path).ok_or_else(|| {
            EngineError::MissingSourceField(format!(
                "join {}:{}/{}",
                join.get(),
                collection.join("/"),
                path.join("/")
            ))
        }),
        Node::JoinPosition { join } => positions
            .iter()
            .rev()
            .find_map(|position| {
                position
                    .join_position
                    .filter(|(owner, _)| owner == join)
                    .map(|(_, index)| Value::Int(index as i64))
            })
            .or_else(|| {
                program
                    .owned_join
                    .and_then(|tuple| tuple.position(*join))
                    .map(|index| Value::Int(index as i64))
            })
            .ok_or(EngineError::MissingJoinContext { join: *join }),
        Node::Unconnected => Ok(Value::Null),
        Node::Const { value } => Ok(value.clone()),
        Node::FunctionParameter { .. } => {
            Err(EngineError::FunctionParameterOutsideFunction { node: node_id })
        }
        Node::RuntimeValue { value } => {
            if program.evaluating_document_filename
                && *value == mapping::RuntimeValue::CurrentDateTime
                && let Some(clock) = program.document_filename_clock
            {
                clock
                    .sample_datetime()
                    .map(Value::String)
                    .map_err(|message| EngineError::DocumentFilenameClock {
                        node: node_id,
                        message,
                    })
            } else if !program.evaluating_document_filename
                && *value == mapping::RuntimeValue::CurrentDateTime
                && let Some(clock) = program.document_body_clock
            {
                clock
                    .sample_datetime()
                    .map(Value::String)
                    .map_err(|message| EngineError::DocumentBodyClock {
                        node: node_id,
                        message,
                    })
            } else {
                context
                    .first()
                    .and_then(|frame| frame.field(runtime_field(*value)))
                    .and_then(Instance::as_scalar)
                    .cloned()
                    .ok_or(EngineError::MissingRuntimeValue(*value))
            }
        }
        Node::InvocationCounter {
            key,
            start,
            step,
            restart,
        } => {
            let counters = program
                .invocation_counters
                .ok_or(EngineError::MissingInvocationCounters { node: node_id })?;
            let start_value =
                eval_node_input(program, node_id, *start, 0, context, positions, in_progress)?;
            let Value::Int(start_value) = start_value else {
                return Err(EngineError::InvocationCounterOperand {
                    node: node_id,
                    operand: "start",
                    expected: "integer",
                    found: start_value.type_name(),
                });
            };
            let step_value =
                eval_node_input(program, node_id, *step, 1, context, positions, in_progress)?;
            let Value::Int(step_value) = step_value else {
                return Err(EngineError::InvocationCounterOperand {
                    node: node_id,
                    operand: "step",
                    expected: "integer",
                    found: step_value.type_name(),
                });
            };
            let restart_value = match restart {
                Some(restart) => eval_node_input(
                    program,
                    node_id,
                    *restart,
                    2,
                    context,
                    positions,
                    in_progress,
                )?,
                None => Value::Null,
            };
            let restart = match &restart_value {
                Value::Null => None,
                Value::String(text) => Some(text.as_str()),
                other => {
                    return Err(EngineError::InvocationCounterOperand {
                        node: node_id,
                        operand: "restart",
                        expected: "string or missing",
                        found: other.type_name(),
                    });
                }
            };
            counters
                .borrow_mut()
                .next(key, start_value, step_value, restart)
                .map(Value::Int)
                .map_err(|source| EngineError::InvocationCounter {
                    node: node_id,
                    source,
                })
        }
        Node::RuntimeParameter { name, ty, preview } => {
            let value = parameter_value(
                context.first().copied(),
                name,
                preview.as_deref(),
                program.purpose,
                node_id,
            )?
            .ok_or_else(|| EngineError::MissingRuntimeParameter {
                node: node_id,
                name: name.clone(),
            })?;
            coerce_value_map_input(&value, *ty).ok_or_else(|| EngineError::RuntimeParameterType {
                node: node_id,
                name: name.clone(),
                expected: *ty,
                found: value.type_name(),
            })
        }
        Node::RuntimeParameterDefault {
            name,
            ty,
            default,
            preview,
        } => {
            let value = match parameter_value(
                context.first().copied(),
                name,
                preview.as_deref(),
                program.purpose,
                node_id,
            )? {
                Some(value) => value.into_owned(),
                None => eval_node_input(
                    program,
                    node_id,
                    *default,
                    0,
                    context,
                    positions,
                    in_progress,
                )?,
            };
            coerce_value_map_input(&value, *ty).ok_or_else(|| EngineError::RuntimeParameterType {
                node: node_id,
                name: name.clone(),
                expected: *ty,
                found: value.type_name(),
            })
        }
        Node::Call { function, args } => {
            let mut values = Vec::with_capacity(args.len());
            for (input_index, arg) in args.iter().enumerate() {
                values.push(eval_node_input(
                    program,
                    node_id,
                    *arg,
                    input_index,
                    context,
                    positions,
                    in_progress,
                )?);
            }
            functions::call(function, &values).map_err(EngineError::from)
        }
        Node::UserFunctionCall { function, args } => {
            let mut values = Vec::with_capacity(args.len());
            for (input_index, arg) in args.iter().enumerate() {
                values.push(eval_node_input(
                    program,
                    node_id,
                    *arg,
                    input_index,
                    context,
                    positions,
                    in_progress,
                )?);
            }
            user_function::evaluate(
                program.user_functions,
                *function,
                values,
                context.first().copied(),
                program.trace_sink,
                program.debug_hook,
                program.first_failure_reported,
                positions,
                program.purpose,
                program.filter_map_work,
                program.budget,
            )
        }
        Node::If {
            condition,
            then,
            else_,
        } => match eval_node_input(
            program,
            node_id,
            *condition,
            0,
            context,
            positions,
            in_progress,
        )? {
            Value::Bool(true) => {
                eval_node_input(program, node_id, *then, 1, context, positions, in_progress)
            }
            Value::Bool(false) => {
                eval_node_input(program, node_id, *else_, 2, context, positions, in_progress)
            }
            other => Err(EngineError::NotABool {
                node: *condition,
                found: other.type_name(),
            }),
        },
        Node::Raise { message } => {
            let message = message
                .map(|message| {
                    eval_node_input(
                        program,
                        node_id,
                        message,
                        0,
                        context,
                        positions,
                        in_progress,
                    )
                    .map(crate::failure::scalar_text)
                })
                .transpose()?;
            Err(EngineError::MappingException {
                node: node_id,
                message,
            })
        }
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let value =
                eval_node_input(program, node_id, *input, 0, context, positions, in_progress)?;
            let value = input_type
                .and_then(|ty| coerce_value_map_input(&value, ty))
                .unwrap_or(value);
            Ok(table
                .iter()
                .find(|(from, _)| *from == value)
                .map(|(_, to)| to.clone())
                .or_else(|| default.clone())
                .unwrap_or(Value::Null))
        }
        Node::Lookup {
            collection,
            key,
            matches,
            value,
        } => {
            let needle = eval_node_input(
                program,
                node_id,
                *matches,
                0,
                context,
                positions,
                in_progress,
            )?;
            let items = collection_items(context, collection)
                .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
            Ok(items
                .iter()
                .filter_map(|item| item.instances.last().copied())
                .find(|item| field_scalar(item, key).is_some_and(|key| *key == needle))
                .and_then(|item| field_scalar(item, value).cloned())
                .unwrap_or(Value::Null))
        }
        Node::DynamicSourceField { object, frame, key } => {
            let key = eval_node_input(program, node_id, *key, 0, context, positions, in_progress)?;
            match key {
                Value::String(key) => {
                    Ok(
                        dynamic_scalar(context, positions, frame.as_deref(), object, &key)
                            .unwrap_or(Value::Null),
                    )
                }
                _ => Ok(Value::Null),
            }
        }
        Node::OwnedDynamicSourceField(field) => {
            let Some(owned) = program.owned_context else {
                return Err(EngineError::UnsupportedSourceOwnership {
                    node: node_id,
                    function: None,
                    kind: "owned_dynamic_source_field",
                });
            };
            // The key evaluates before any source access; a non-string key
            // reads nothing, as in the legacy node.
            let key = eval_node_input(
                program,
                node_id,
                field.key,
                0,
                context,
                positions,
                in_progress,
            )?;
            match key {
                Value::String(key) => {
                    owned
                        .dynamic_field(field, &key)
                        .map_err(|source| match source {
                            crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
                                EngineError::UnsupportedOwnedContext {
                                    node: Some(node_id),
                                    function: None,
                                    scope: None,
                                    context: if found == "a document set" {
                                        crate::OwnedCapabilityContext::Document
                                    } else {
                                        crate::OwnedCapabilityContext::GeneratedItem
                                    },
                                }
                            }
                            source => EngineError::OwnedSourceAccess {
                                node: Some(node_id),
                                scope: None,
                                source,
                            },
                        })
                }
                _ => Ok(Value::Null),
            }
        }
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        } => eval_xml_mixed_content(
            program,
            node_id,
            path,
            frame.as_deref(),
            replacements,
            context,
            positions,
            in_progress,
        ),
        Node::XmlSerialize {
            path,
            frame,
            schema,
            declaration,
            indent,
            namespace,
        } => {
            let instance = match frame {
                Some(frame) => instance_in_frame(context, positions, frame, path),
                None => instance_in_active_collection(context, positions, path),
            }
            .ok_or_else(|| {
                let mut display = frame.clone().unwrap_or_default();
                display.extend(path.iter().cloned());
                EngineError::MissingSourceField(display.join("/"))
            })?;
            serialize_xml(
                node_id,
                schema,
                instance,
                *declaration,
                *indent,
                namespace.as_deref(),
            )
        }
        Node::OwnedXmlMixedContent(mixed) => {
            crate::owned_xml_mixed_content::eval_owned_xml_mixed_content(
                program,
                node_id,
                mixed,
                context,
                positions,
                in_progress,
            )
        }
        Node::OwnedXmlSerialize(serializer) => match program.owned_context {
            Some(owned) => {
                let access = |source| owned_access_error(node_id, source);
                // The exact owner's absence is final; no legacy frame repairs it.
                let instance = owned
                    .element(&serializer.source, serializer.frame.as_deref())
                    .map_err(access)?
                    .ok_or_else(|| {
                        access(crate::OwnedSourceError::MissingOwnedElement {
                            owner: serializer.source.owner.clone(),
                            path: serializer.source.path.clone(),
                        })
                    })?;
                serialize_xml(
                    node_id,
                    &serializer.schema,
                    instance,
                    serializer.declaration,
                    serializer.indent,
                    serializer.namespace.as_deref(),
                )
            }
            None => Err(EngineError::UnsupportedSourceOwnership {
                node: node_id,
                function: None,
                kind: "owned_xml_serialize",
            }),
        },
        Node::CollectionFind {
            collection,
            predicate,
            value,
        } => eval_collection_find(
            program,
            node_id,
            collection,
            *predicate,
            *value,
            context,
            positions,
            in_progress,
        ),
        Node::SequenceExists {
            sequence,
            predicate,
        } => eval_sequence_exists(
            program,
            node_id,
            sequence,
            *predicate,
            context,
            positions,
            in_progress,
        ),
        Node::SequenceItemAt { sequence, index } => eval_sequence_item_at(
            program,
            node_id,
            sequence,
            *index,
            context,
            positions,
            in_progress,
        ),
        Node::SequenceAggregate {
            function,
            sequence,
            predicate,
            expression,
            arg,
        } => eval_sequence_aggregate(
            program,
            node_id,
            *function,
            sequence,
            *predicate,
            *expression,
            *arg,
            context,
            positions,
            in_progress,
        ),
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } => {
            // Absent repeating data aggregates as an empty collection.
            let items = aggregate_items(context, collection);
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                let terminal = item.instances.last().copied();
                let item_value = if let Some(expression) = expression {
                    let mut item_context = context.to_vec();
                    item_context.extend(item.instances.iter().copied());
                    let mut item_positions = positions.to_vec();
                    item_positions.extend(item.positions.iter().cloned());
                    eval_node_input(
                        program,
                        node_id,
                        *expression,
                        0,
                        &item_context,
                        &item_positions,
                        in_progress,
                    )?
                } else if value.is_empty() {
                    terminal
                        .and_then(Instance::as_scalar)
                        .cloned()
                        .unwrap_or(Value::Null)
                } else {
                    terminal
                        .and_then(|item| field_scalar(item, value))
                        .cloned()
                        .unwrap_or(Value::Null)
                };
                crate::aggregate::collect_value(*function, &mut values, item_value)?;
            }
            let arg_value = match arg {
                Some(id) => Some(eval_node_input(
                    program,
                    node_id,
                    *id,
                    usize::from(expression.is_some()),
                    context,
                    positions,
                    in_progress,
                )?),
                None => None,
            };
            aggregate(*function, items.len(), &values, arg_value)
        }
        Node::JoinAggregate {
            function,
            join,
            plan,
            expression,
            arg,
        } => eval_join_aggregate(
            program,
            JoinAggregateInput {
                consumer: node_id,
                function: *function,
                join: *join,
                plan,
                expression: *expression,
                arg: *arg,
            },
            context,
            positions,
            in_progress,
        ),
    };

    in_progress.remove(&node_id);
    if let Ok(value) = &result {
        record_node_value(program, node_id, positions, value);
        crate::debug::after_node_value(
            program.debug_hook,
            node_id,
            value,
            positions,
            context,
            program.primary_source,
        )?;
    }
    result
}

pub(super) fn eval_node_input(
    program: EvalProgram<'_>,
    consumer: NodeId,
    input: NodeId,
    input_index: usize,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let value = eval_expr(program, input, context, positions, in_progress)?;
    record_node_input_value(program, consumer, input, input_index, positions, &value);
    crate::debug::after_node_input(
        program.debug_hook,
        consumer,
        input,
        input_index,
        &value,
        positions,
        context,
        program.primary_source,
    )?;
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
fn eval_xml_mixed_content(
    program: EvalProgram<'_>,
    consumer: NodeId,
    path: &[String],
    frame: Option<&[String]>,
    replacements: &[mapping::XmlMixedContentReplacement],
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let group = match frame {
        Some(frame) => instance_in_frame(context, positions, frame, path),
        None => instance_in_active_collection(context, positions, path),
    }
    .ok_or_else(|| EngineError::MissingSourceField(path.join("/")))?;
    let Some(items) = group
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
    else {
        return Ok(group
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null));
    };
    let mut output = String::new();
    let mut occurrences = std::collections::BTreeMap::<&str, usize>::new();
    for item in items {
        let name = item
            .field(XML_NODE_NAME_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let text = item
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(text) => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let Some((input_index, replacement)) = replacements
            .iter()
            .enumerate()
            .find(|(_, rule)| rule.element == name)
        else {
            output.push_str(text);
            continue;
        };
        let mut item_context = context.to_vec();
        let mut item_positions = positions.to_vec();
        if !replacement.collection.is_empty()
            && let Some(value) = item.field(XML_MIXED_CONTENT_VALUE_FIELD)
        {
            let index = occurrences.entry(name).or_default();
            *index += 1;
            item_context.push(value);
            item_positions.push(PositionFrame {
                collection: replacement.collection.clone(),
                index: *index,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            });
        }
        let value = eval_node_input(
            program,
            consumer,
            replacement.expression,
            input_index,
            &item_context,
            &item_positions,
            in_progress,
        )?;
        match value {
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
            Value::String(value) => output.push_str(&value),
            Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
            Value::Int(value) => output.push_str(&value.to_string()),
            Value::Float(value) => output.push_str(&value.to_string()),
            Value::Decimal(value) => output.push_str(&value.to_string()),
        }
    }
    Ok(Value::String(output))
}

fn aggregate_items<'a>(context: &[&'a Instance], collection: &[String]) -> Vec<WalkExtension<'a>> {
    collection_items(context, collection).unwrap_or_default()
}

fn collection_items<'a>(
    context: &[&'a Instance],
    collection: &[String],
) -> Option<Vec<WalkExtension<'a>>> {
    let base = if let Some(first) = collection.first() {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.field(first).is_some())
    } else {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| matches!(item, Instance::Repeated(_) | Instance::DocumentSet(_)))
    };
    base.map(|base| walk(base, collection, &[], &[], &[]))
}

#[allow(clippy::too_many_arguments)]
fn eval_collection_find(
    program: EvalProgram<'_>,
    consumer: NodeId,
    collection: &[String],
    predicate: NodeId,
    value: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let root = if let Some(first) = collection.first() {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.field(first).is_some())
    } else {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.as_repeated().is_some())
    }
    .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
    let mut item_context = context.to_vec();
    let mut item_positions = positions.to_vec();
    visit_collection_find(
        program,
        consumer,
        root,
        collection,
        0,
        predicate,
        value,
        &mut item_context,
        &mut item_positions,
        in_progress,
    )
    .map(|found| found.unwrap_or(Value::Null))
}

#[allow(clippy::too_many_arguments)]
fn visit_collection_find<'a>(
    program: EvalProgram<'_>,
    consumer: NodeId,
    current: &'a Instance,
    collection: &[String],
    consumed: usize,
    predicate: NodeId,
    value: NodeId,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
    in_progress: &mut HashSet<NodeId>,
) -> Result<Option<Value>, EngineError> {
    if let Instance::Repeated(items) = current {
        for (item_index, item) in items.iter().enumerate() {
            context.push(item);
            positions.push(PositionFrame {
                collection: collection[..consumed].to_vec(),
                index: item_index + 1,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            });
            let found = visit_collection_find(
                program,
                consumer,
                item,
                collection,
                consumed,
                predicate,
                value,
                context,
                positions,
                in_progress,
            )?;
            positions.pop();
            context.pop();
            if found.is_some() {
                return Ok(found);
            }
        }
        return Ok(None);
    }
    if consumed < collection.len() {
        return match current.field(&collection[consumed]) {
            Some(next) => visit_collection_find(
                program,
                consumer,
                next,
                collection,
                consumed + 1,
                predicate,
                value,
                context,
                positions,
                in_progress,
            ),
            None => Ok(None),
        };
    }
    match eval_node_input(
        program,
        consumer,
        predicate,
        0,
        context,
        positions,
        in_progress,
    )? {
        Value::Bool(true) => {
            eval_node_input(program, consumer, value, 1, context, positions, in_progress).map(Some)
        }
        Value::Bool(false) | Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Ok(None),
        other => Err(EngineError::NotABool {
            node: predicate,
            found: other.type_name(),
        }),
    }
}

pub(super) fn coerce_value_map_input(value: &Value, ty: ScalarType) -> Option<Value> {
    match (ty, value) {
        (_, Value::Decimal(_)) => None,
        (_, Value::Null) => Some(Value::Null),
        (_, Value::JsonNull(_)) => Some(Value::Null),
        (_, Value::XmlNil(value)) => Some(Value::XmlNil(*value)),
        (ScalarType::String, Value::String(value)) => Some(Value::String(value.clone())),
        (ScalarType::String, Value::Bool(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Int(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            Some(Value::String(value.to_string()))
        }
        (ScalarType::String, Value::Float(_)) => None,
        (ScalarType::Int, Value::Int(value)) => Some(Value::Int(*value)),
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < -(i64::MIN as f64) =>
        {
            Some(Value::Int(*value as i64))
        }
        (ScalarType::Int, Value::String(value)) => value.trim().parse().ok().map(Value::Int),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Some(Value::Float(*value)),
        (ScalarType::Float, Value::Int(value)) => Some(Value::Float(*value as f64)),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        (ScalarType::Bool, Value::Bool(value)) => Some(Value::Bool(*value)),
        (ScalarType::Bool, Value::String(value)) => match value.trim() {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        (ScalarType::Int | ScalarType::Float | ScalarType::Bool, _) => None,
    }
}

fn position(positions: &[PositionFrame], collection: &[String]) -> usize {
    positions
        .iter()
        .rev()
        .find(|position| {
            collection.is_empty()
                || position.collection.len() >= collection.len()
                    && position.collection[position.collection.len() - collection.len()..]
                        == *collection
        })
        .map(|position| position.index)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_flattens_repeating_collection_ancestors() {
        let record = |code: i64, name: &str| {
            Instance::Group(
                (vec![
                    ("Code".into(), Instance::Scalar(Value::Int(code))),
                    ("Name".into(), Instance::Scalar(Value::String(name.into()))),
                ])
                .into(),
            )
        };
        let branch = |record| {
            Instance::Group(
                (vec![(
                    "Cube".into(),
                    Instance::Repeated(vec![Instance::Group(
                        (vec![("Cube".into(), Instance::Repeated(vec![record]))]).into(),
                    )]),
                )])
                .into(),
            )
        };
        let source = Instance::Group(
            (vec![(
                "Cube".into(),
                Instance::Repeated(vec![
                    branch(record(1, "first")),
                    branch(record(42, "second")),
                ]),
            )])
            .into(),
        );
        let graph = Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(42),
                    },
                ),
                (
                    1,
                    Node::Lookup {
                        collection: vec!["Cube".into(), "Cube".into(), "Cube".into()],
                        key: vec!["Code".into()],
                        matches: 0,
                        value: vec!["Name".into()],
                    },
                ),
            ]
            .into(),
        };
        let user_functions = BTreeMap::new();

        let value = eval_expr(
            EvalProgram::new(&graph, &user_functions, None, &Cell::new(false)),
            1,
            &[&source],
            &[],
            &mut HashSet::new(),
        )
        .expect("nested lookup should evaluate");

        assert_eq!(value, Value::String("second".into()));
    }
}

#[cfg(test)]
mod invocation_counter_effect_tests {
    use super::*;
    use functions::{InvocationCounterLimits, InvocationCounters};

    #[test]
    fn nested_counter_effect_survives_a_later_outer_operand_refusal() {
        for outer_key in ["line", "outer"] {
            let graph = Graph {
                nodes: [
                    (
                        1,
                        Node::InvocationCounter {
                            key: outer_key.into(),
                            start: 2,
                            step: 4,
                            restart: None,
                        },
                    ),
                    (
                        2,
                        Node::InvocationCounter {
                            key: "line".into(),
                            start: 3,
                            step: 3,
                            restart: None,
                        },
                    ),
                    (
                        3,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        4,
                        Node::Const {
                            value: Value::Bool(true),
                        },
                    ),
                ]
                .into(),
            };
            let functions = BTreeMap::new();
            let reported = Cell::new(false);
            let counters = RefCell::new(InvocationCounters::new(InvocationCounterLimits {
                max_counters: 2,
                max_text_bytes: 32,
            }));
            let program = EvalProgram::new(&graph, &functions, None, &reported)
                .with_invocation_counters(Some(&counters));
            assert_eq!(
                eval_expr(program, 1, &[], &[], &mut HashSet::new()),
                Err(EngineError::InvocationCounterOperand {
                    node: 1,
                    operand: "step",
                    expected: "integer",
                    found: "bool"
                })
            );
            assert_eq!(counters.borrow().len(), 1);
            assert_eq!(counters.borrow_mut().next("line", 1, 1, None), Ok(2));
            if outer_key == "outer" {
                assert_eq!(counters.borrow_mut().next("outer", 9, 1, None), Ok(9));
            }
        }
    }
}

#[cfg(test)]
mod primary_root_owner_tests {
    use super::*;
    use ir::{InstanceGroup, PrimaryRootError, XmlTypeOrigin};
    fn marked(value: &str, annotation: &str) -> Instance {
        Instance::Group(
            InstanceGroup::from(vec![(
                "Code".into(),
                Instance::Scalar(Value::String(value.into())),
            )])
            .with_xml_type_origin(XmlTypeOrigin::Explicit(annotation))
            .unwrap(),
        )
    }
    #[test]
    fn primary_owner_is_explicit_and_independent_of_all_frame_indices() {
        let owner = marked("primary", "Derived");
        let outer = marked("outer", "Other");
        let inner = marked("inner", "Other");
        let graph = Graph {
            nodes: [
                (
                    0,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type: "Derived".into(),
                    },
                ),
                (
                    1,
                    Node::SourceRootField {
                        path: vec!["Code".into()],
                        required: false,
                    },
                ),
            ]
            .into(),
        };
        let functions = BTreeMap::new();
        let reported = Cell::new(false);
        let program =
            EvalProgram::new(&graph, &functions, None, &reported).with_primary_source(&owner);
        for frames in [
            vec![],
            vec![&inner],
            vec![&outer, &inner],
            vec![&outer, &inner, &outer, &inner],
        ] {
            assert_eq!(
                eval_expr(program, 0, &frames, &[], &mut HashSet::new()).unwrap(),
                Value::Bool(true)
            );
            assert_eq!(
                eval_expr(program, 1, &frames, &[], &mut HashSet::new()).unwrap(),
                Value::String("primary".into())
            );
        }
        let missing = EvalProgram::new(&graph, &functions, None, &reported);
        assert!(matches!(
            eval_expr(missing, 0, &[&owner], &[], &mut HashSet::new()),
            Err(EngineError::PrimaryRoot {
                node: 0,
                source: PrimaryRootError::MissingOwner
            })
        ));
    }
}
