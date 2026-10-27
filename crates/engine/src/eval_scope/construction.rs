//! Construct one selected item, preserving binding, child and observer order.
//!
//! Item contexts stay borrowed. Only constructors that require an owned output
//! copy payloads; the caller owns selection, grouping and position compaction.

use std::collections::{BTreeMap, HashSet};

use ir::{
    Instance, ScalarType, SchemaKind, Value, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use mapping::{IterationOutput, NamedSource, Scope, ScopeConstruction, XmlMixedContentElement};

use super::{eval_scope, passes_scope_filter};
use crate::debug::before_target_write;
use crate::dynamic_target::{self, eval_dynamic_key, insert_target_field};
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::recursive_filter;
use crate::source_iteration::PositionFrame;
use crate::trace::{
    TraceEvent, TraceFilterPhase, TraceOutputKind, TraceScope, TraceTargetFieldBinding, TraceValue,
    bounded_text, record_owned_event, trace_positions,
};
use crate::{DynamicSourceLoader, EngineError};

pub(super) struct ProducedItem {
    pub(super) instance: Instance,
    pub(super) output_path: Option<String>,
}

pub(super) struct ItemEvaluator<'a> {
    pub(super) program: EvalProgram<'a>,
    pub(super) scope: &'a Scope,
    pub(super) trace_scope: &'a TraceScope,
    pub(super) target: Option<&'a ir::SchemaNode>,
    pub(super) extra_sources: &'a [NamedSource],
    pub(super) source_loader: Option<&'a dyn DynamicSourceLoader>,
}

/// Evaluates one iteration item: the filter (`None` when it drops the item),
/// then the scope's bindings and child scopes.
impl ItemEvaluator<'_> {
    pub(super) fn produce(
        &self,
        context: &[&Instance],
        filter_positions: &[PositionFrame],
        output_positions: &[PositionFrame],
        apply_filter: bool,
    ) -> Result<Option<ProducedItem>, EngineError> {
        let Some(instance) =
            self.produce_instance(context, filter_positions, output_positions, apply_filter)?
        else {
            return Ok(None);
        };
        let output_path = self
            .scope
            .output_path()
            .map(|node| {
                let mut in_progress = HashSet::new();
                match eval_expr(
                    self.program,
                    node,
                    context,
                    output_positions,
                    &mut in_progress,
                )? {
                    Value::String(path) if !path.trim().is_empty() => Ok(path),
                    Value::String(_) => Err(EngineError::EmptyDynamicTargetPath { node }),
                    value => Err(EngineError::DynamicTargetPath {
                        node,
                        found: value.type_name(),
                    }),
                }
            })
            .transpose()?;
        record_owned_event(self.program, output_positions, || {
            TraceEvent::TargetProduced {
                scope: self.trace_scope.clone(),
                positions: trace_positions(output_positions),
                output_path: output_path.as_deref().map(bounded_text),
                kind: TraceOutputKind::of(&instance),
            }
        });
        Ok(Some(ProducedItem {
            instance,
            output_path,
        }))
    }

    fn produce_instance(
        &self,
        context: &[&Instance],
        filter_positions: &[PositionFrame],
        output_positions: &[PositionFrame],
        apply_filter: bool,
    ) -> Result<Option<Instance>, EngineError> {
        let Self {
            program,
            scope,
            trace_scope,
            target,
            extra_sources,
            source_loader,
        } = *self;
        let filter_phase = if scope.has_sort() {
            TraceFilterPhase::AfterSort
        } else {
            TraceFilterPhase::Selection
        };
        if apply_filter
            && !passes_scope_filter(
                program,
                scope.filter,
                filter_phase,
                trace_scope,
                context,
                filter_positions,
            )?
        {
            return Ok(None);
        }

        if matches!(scope.construction, ScopeConstruction::CollectDocuments) {
            let mut documents = Vec::new();
            for (index, child) in scope.children.iter().enumerate() {
                let value = eval_scope(
                    program,
                    child,
                    target,
                    context,
                    output_positions,
                    extra_sources,
                    source_loader,
                    &trace_scope.child(&child.target_field, index),
                )?;
                super::document_collection::extend(&mut documents, value)?;
            }
            return Ok(Some(Instance::DocumentSet(documents)));
        }

        if let ScopeConstruction::Scalar { value } = &scope.construction {
            let mut in_progress = HashSet::new();
            let value = eval_expr(program, *value, context, output_positions, &mut in_progress)?;
            return Ok(Some(Instance::Scalar(adapt_target_scalar(value, target))));
        }

        if let Some((source, plan)) = scope.owned_recursive_filter() {
            // Admission routes every owned recursive filter through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return recursive_filter::execute_owned(
                program,
                plan,
                source,
                state,
                context,
                output_positions,
            )
            .map(Some);
        }

        if let ScopeConstruction::RecursiveFilter { plan } = &scope.construction {
            let current =
                context
                    .last()
                    .copied()
                    .ok_or(EngineError::RecursiveFilterRequiresGroup {
                        found: "missing context",
                    })?;
            return recursive_filter::execute(program, plan, current, context, output_positions)
                .map(Some);
        }

        if let ScopeConstruction::PathHierarchy { plan } = &scope.construction {
            return crate::path_hierarchy::build(plan, context).map(Some);
        }

        if let ScopeConstruction::OwnedPathHierarchy { plan } = &scope.construction {
            // Admission routes every owned hierarchy through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return crate::path_hierarchy::build_owned(
                plan,
                state,
                output_positions.len(),
                &trace_scope.source_scope,
            )
            .map(Some);
        }

        if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction {
            return crate::adjacency_tree::construct(program, plan, context, output_positions)
                .map(Some);
        }

        if let ScopeConstruction::OwnedAdjacencyTree { plan } = &scope.construction {
            // Admission routes every owned adjacency tree through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return crate::adjacency_tree::construct_owned(
                program,
                plan,
                state,
                context,
                output_positions,
                &trace_scope.source_scope,
            )
            .map(Some);
        }

        if matches!(&scope.construction, ScopeConstruction::CopyCurrentSource) {
            return match context.last().copied() {
                Some(current @ Instance::Group(_)) => Ok(Some((*current).clone())),
                Some(Instance::Scalar(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup { found: "scalar" })
                }
                Some(Instance::Repeated(_)) => Err(EngineError::CopyCurrentSourceRequiresGroup {
                    found: "repeated collection",
                }),
                Some(Instance::MappedSequence(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup {
                        found: "mapped sequence",
                    })
                }
                Some(Instance::DocumentSet(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup {
                        found: "document set",
                    })
                }
                None => Err(EngineError::CopyCurrentSourceRequiresGroup {
                    found: "missing context",
                }),
            };
        }

        let mut fields = Vec::with_capacity(
            scope.bindings.len()
                + scope.dynamic_bindings.len()
                + scope.children.len()
                + scope.dynamic_children.len(),
        );
        for binding in &scope.bindings {
            let mut in_progress = HashSet::new();
            let value = eval_expr(
                program,
                binding.node,
                context,
                output_positions,
                &mut in_progress,
            )?;
            let target_field = target.and_then(|schema| schema.child(&binding.target_field));
            let value = adapt_target_scalar(value, target_field);
            let repeating = target_field.is_some_and(|field| field.repeating);
            let value = match repeating {
                true => match value {
                    Value::Null => Instance::Repeated(Vec::new()),
                    value => Instance::Repeated(vec![Instance::Scalar(value)]),
                },
                false => Instance::Scalar(value),
            };
            let traced_value = trace_field_value(&value);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::StaticBinding {
                    value: binding.node,
                },
                output_positions,
                context,
                &binding.target_field,
                &value,
                &fields,
                program.primary_source(),
            )?;
            insert_static_binding(&mut fields, binding.target_field.clone(), value, repeating)?;
            record_target_field(
                program,
                trace_scope,
                bounded_text(&binding.target_field),
                TraceTargetFieldBinding::StaticBinding {
                    value: binding.node,
                },
                output_positions,
                traced_value,
            );
        }
        for binding in &scope.dynamic_bindings {
            let key = eval_dynamic_key(program, binding.key, context, output_positions)?;
            let mut in_progress = HashSet::new();
            let value = eval_expr(
                program,
                binding.value,
                context,
                output_positions,
                &mut in_progress,
            )?;
            let value = adapt_target_scalar(value, target.and_then(ir::SchemaNode::dynamic_fields));
            let value = Instance::Scalar(value);
            let traced_key = bounded_text(&key);
            let traced_value = trace_field_value(&value);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::DynamicBinding {
                    key: binding.key,
                    value: binding.value,
                },
                output_positions,
                context,
                &key,
                &value,
                &fields,
                program.primary_source(),
            )?;
            dynamic_target::insert_dynamic_target_field(&mut fields, key, value, target)?;
            record_target_field(
                program,
                trace_scope,
                traced_key,
                TraceTargetFieldBinding::DynamicBinding {
                    key: binding.key,
                    value: binding.value,
                },
                output_positions,
                traced_value,
            );
        }
        for (index, child) in scope.children.iter().enumerate() {
            let child_target = target.and_then(|schema| schema.child(&child.target_field));
            let child_instance = eval_scope(
                program,
                child,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
                &trace_scope.child(&child.target_field, index),
            )?;
            let traced_value = trace_field_value(&child_instance);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::StaticChild,
                output_positions,
                context,
                &child.target_field,
                &child_instance,
                &fields,
                program.primary_source(),
            )?;
            insert_target_field(&mut fields, child.target_field.clone(), child_instance)?;
            record_target_field(
                program,
                trace_scope,
                bounded_text(&child.target_field),
                TraceTargetFieldBinding::StaticChild,
                output_positions,
                traced_value,
            );
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            if child.scope.iteration_output == IterationOutput::MappedSequence {
                return Err(EngineError::MappedSequenceDynamicTarget);
            }
            let key = eval_dynamic_key(program, child.key, context, output_positions)?;
            let child_target = target.and_then(ir::SchemaNode::dynamic_fields);
            let child_instance = eval_scope(
                program,
                &child.scope,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
                &trace_scope.dynamic_child(index, scope.children.len().saturating_add(index)),
            )?;
            let traced_key = bounded_text(&key);
            let traced_value = trace_field_value(&child_instance);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::DynamicChild { key: child.key },
                output_positions,
                context,
                &key,
                &child_instance,
                &fields,
                program.primary_source(),
            )?;
            dynamic_target::insert_dynamic_target_field(&mut fields, key, child_instance, target)?;
            record_target_field(
                program,
                trace_scope,
                traced_key,
                TraceTargetFieldBinding::DynamicChild { key: child.key },
                output_positions,
                traced_value,
            );
        }
        if let ScopeConstruction::XmlMixedContent { elements } = &scope.construction {
            // An owned selector attaches only its own exact current item.
            let source = match scope.source_selector() {
                Some(mapping::SourceSelectorRef::Owned(source)) => {
                    let state = program.owned_context.ok_or_else(|| {
                        EngineError::UnsupportedSourceIteration {
                            scope: trace_scope.source_scope.clone(),
                        }
                    })?;
                    crate::owned_xml_mixed_content::owned_construction_item(state, source).map_err(
                        |source| EngineError::OwnedSourceAccess {
                            node: None,
                            scope: Some(Box::new(trace_scope.source_scope.clone())),
                            source,
                        },
                    )?
                }
                _ => context.last().copied(),
            };
            attach_xml_mixed_content(&mut fields, source, elements);
        }
        Ok(Some(Instance::Group((fields).into())))
    }
}

fn trace_field_value(value: &Instance) -> (TraceOutputKind, Option<TraceValue>) {
    let preview = match value {
        Instance::Scalar(value) => Some(TraceValue::new(value)),
        Instance::Repeated(values) if values.len() == 1 => values
            .first()
            .and_then(Instance::as_scalar)
            .map(TraceValue::new),
        Instance::Group(_)
        | Instance::Repeated(_)
        | Instance::MappedSequence(_)
        | Instance::DocumentSet(_) => None,
    };
    (TraceOutputKind::of(value), preview)
}

fn record_target_field(
    program: EvalProgram<'_>,
    trace_scope: &TraceScope,
    field: String,
    binding: TraceTargetFieldBinding,
    positions: &[PositionFrame],
    (kind, value): (TraceOutputKind, Option<TraceValue>),
) {
    record_owned_event(program, positions, || TraceEvent::TargetFieldWritten {
        scope: trace_scope.clone(),
        field,
        binding,
        positions: trace_positions(positions),
        kind,
        value,
    });
}

fn attach_xml_mixed_content(
    fields: &mut Vec<(String, Instance)>,
    source: Option<&Instance>,
    elements: &[XmlMixedContentElement],
) {
    let Some(source_items) = source
        .and_then(|source| source.field(XML_MIXED_CONTENT_FIELD))
        .and_then(Instance::as_repeated)
    else {
        return;
    };
    let mut occurrences = BTreeMap::<&str, usize>::new();
    let items = source_items
        .iter()
        .filter_map(|item| {
            let name = item
                .field(XML_NODE_NAME_FIELD)
                .and_then(Instance::as_scalar)
                .and_then(|value| match value {
                    Value::String(name) => Some(name.as_str()),
                    _ => None,
                })?;
            if name.is_empty() {
                return Some(item.clone());
            }
            let element = elements.iter().find(|element| element.source == name)?;
            let index = occurrences.entry(&element.target).or_default();
            let value = fields
                .iter()
                .find(|(field, _)| field == &element.target)
                .and_then(|(_, value)| value.as_repeated())?
                .get(*index)?
                .clone();
            *index += 1;
            let text = value
                .as_scalar()
                .map(mixed_content_text)
                .unwrap_or_default();
            Some(Instance::Group(
                (vec![
                    (
                        XML_NODE_NAME_FIELD.to_string(),
                        Instance::Scalar(Value::String(element.target.clone())),
                    ),
                    (
                        XML_TEXT_FIELD.to_string(),
                        Instance::Scalar(Value::String(text)),
                    ),
                    (XML_MIXED_CONTENT_VALUE_FIELD.to_string(), value),
                ])
                .into(),
            ))
        })
        .collect::<Vec<_>>();
    if !items.is_empty() {
        fields.push((
            XML_MIXED_CONTENT_FIELD.to_string(),
            Instance::Repeated(items),
        ));
    }
}

fn mixed_content_text(value: &Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

fn adapt_numeric_target(value: Value, expected: ScalarType) -> Value {
    match (expected, value) {
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Value::Int(value as i64)
        }
        (ScalarType::Float, Value::Int(value)) => {
            let converted = value as f64;
            if (converted as i128) == i128::from(value) {
                Value::Float(converted)
            } else {
                Value::Int(value)
            }
        }
        (_, value) => value,
    }
}

fn adapt_target_scalar(value: Value, target: Option<&ir::SchemaNode>) -> Value {
    let Some(target) = target else {
        return value;
    };
    match target.kind {
        SchemaKind::Scalar { ty } => adapt_numeric_target(value, ty),
        SchemaKind::ScalarUnion { types } => {
            let actual = match value {
                Value::String(_) => Some(ScalarType::String),
                Value::Int(_) => Some(ScalarType::Int),
                Value::Float(_) => Some(ScalarType::Float),
                Value::Bool(_) => Some(ScalarType::Bool),
                Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Decimal(_) => None,
            };
            if actual.is_some_and(|ty| types.contains(ty)) {
                return value;
            }
            match value {
                value @ Value::Int(_) if types.contains(ScalarType::Float) => {
                    adapt_numeric_target(value, ScalarType::Float)
                }
                value @ Value::Float(_) if types.contains(ScalarType::Int) => {
                    adapt_numeric_target(value, ScalarType::Int)
                }
                value => value,
            }
        }
        SchemaKind::Group { .. } => value,
    }
}

fn insert_static_binding(
    fields: &mut Vec<(String, Instance)>,
    name: String,
    value: Instance,
    repeating: bool,
) -> Result<(), EngineError> {
    if repeating {
        let Instance::Repeated(mut additions) = value else {
            return insert_target_field(fields, name, value);
        };
        if let Some((_, Instance::Repeated(existing))) =
            fields.iter_mut().find(|(field, _)| field == &name)
        {
            existing.append(&mut additions);
            return Ok(());
        }
        return insert_target_field(fields, name, Instance::Repeated(additions));
    }
    insert_target_field(fields, name, value)
}
