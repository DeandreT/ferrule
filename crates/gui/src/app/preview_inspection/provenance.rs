//! A source-row snapshot alone is not evidence that it supplied a cell.
//! Admit a direct field or unary call only when the selected output write,
//! input delivery and candidate agree in one simple, uncompacted scope.

use engine::{TraceEvent, TraceOutputKind, TraceScope, TraceTarget, TraceTargetFieldBinding};
use mapping::{Node, NodeId, Project, Scope, ScopeIteration};

use crate::preview::PreviewTarget;
use crate::preview_table::{MAX_COLUMNS, PreviewTable};
use crate::run_report::TraceReport;

#[derive(Debug)]
struct Field {
    name: String,
    write_node: NodeId,
    input_node: NodeId,
    source_name: String,
    function: Option<String>,
}

#[derive(Debug)]
pub(super) struct Provenance {
    target: TraceTarget,
    source_root: String,
    collection: Vec<String>,
    produced: Vec<usize>,
    fields: Vec<Field>,
}

impl Provenance {
    pub(super) fn capture(
        project: &Project,
        root: &Scope,
        selected: &PreviewTarget,
        trace: &TraceReport,
    ) -> Option<Self> {
        // A filtered/sorted output position can differ from the candidate's
        // original position. Do not use positional evidence for those scopes.
        let ScopeIteration::Source(collection) = &root.iteration else {
            return None;
        };
        if trace.dropped != 0
            || !trace.stages.is_empty()
            || root.filter.is_some()
            || root.has_grouping()
            || root.has_sort()
            || !root.windows.is_empty()
            || !root.children.is_empty()
            || !root.dynamic_children.is_empty()
            || !root.dynamic_bindings.is_empty()
            || root.bindings.len() > MAX_COLUMNS
            || collection.len() > 128
        {
            return None;
        }
        let target = match selected {
            PreviewTarget::Primary => TraceTarget::Primary,
            PreviewTarget::Named(name) => TraceTarget::Named(name.clone()),
        };
        let mut fields = Vec::new();
        for binding in &root.bindings {
            let Some(node) = project.graph.nodes.get(&binding.node) else {
                continue;
            };
            let (input_node, function) = match node {
                Node::Call { function, args } if args.len() == 1 => {
                    (args[0], Some(function.clone()))
                }
                Node::SourceField { .. } => (binding.node, None),
                _ => continue,
            };
            let Some(Node::SourceField { path, frame }) = project.graph.nodes.get(&input_node)
            else {
                continue;
            };
            if path.len() != 1 || frame.as_ref().is_some_and(|frame| frame != collection) {
                continue;
            }
            // Trace names are capped at 160 characters without an explicit
            // truncation flag on writes. A boundary-length name is not proof.
            if binding.target_field.chars().count() >= 160 {
                continue;
            }
            fields.push(Field {
                name: binding.target_field.clone(),
                write_node: binding.node,
                input_node,
                source_name: path[0].clone(),
                function,
            });
        }
        let produced = trace
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                TraceEvent::TargetProduced {
                    scope,
                    output_path: None,
                    kind: TraceOutputKind::Group,
                    ..
                } if is_root(&target, scope) => Some(index),
                _ => None,
            })
            .collect();
        Some(Self {
            target,
            source_root: project.source.name.clone(),
            collection: collection.clone(),
            produced,
            fields,
        })
    }

    pub(super) fn cell(
        &self,
        trace: &TraceReport,
        table: &PreviewTable,
        row: usize,
        col: usize,
    ) -> Option<String> {
        if self.produced.len() != table.rows.len() {
            return None;
        }
        let end = *self.produced.get(row)?;
        let start = row.checked_sub(1).map_or(0, |i| self.produced[i] + 1);
        let TraceEvent::TargetProduced { positions, .. } = &trace.events[end] else {
            return None;
        };
        if positions.last()?.collection != self.collection {
            return None;
        }
        let header = table.headers.get(col)?;
        if table.column(header).ok()? != col {
            return None;
        }
        let field = self.fields.iter().find(|field| &field.name == header)?;
        let actual = &table.rows[row][col];
        let mut writes = trace.events[start..end]
            .iter()
            .enumerate()
            .filter_map(|(i, event)| match event {
                TraceEvent::TargetFieldWritten {
                    scope,
                    positions: at,
                    field: name,
                    binding: TraceTargetFieldBinding::StaticBinding { value },
                    kind: TraceOutputKind::Scalar,
                    value: Some(written),
                } if is_root(&self.target, scope)
                    && name == &field.name
                    && *value == field.write_node
                    && at == positions
                    && !written.truncated
                    && &written.preview == actual =>
                {
                    Some(i + start)
                }
                _ => None,
            });
        let write = writes.next()?;
        if writes.next().is_some() {
            return None;
        }
        let mut candidates = trace.events[..write]
            .iter()
            .filter_map(|event| match event {
                TraceEvent::IterationCandidate {
                    scope,
                    positions: at,
                    source_row: Some(source),
                    ..
                } if is_root(&self.target, scope) && at == positions => Some(source),
                _ => None,
            });
        let source = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let read = source
            .fields
            .iter()
            .find(|f| !f.name_truncated && f.name == field.source_name)?
            .value
            .as_ref()?;
        let evaluated = &trace.events[start..write];
        let source_evaluated = evaluated.iter().any(|event| {
            matches!(event, TraceEvent::NodeValue { node, positions: at, value }
                if *node == field.input_node && positions == at && value == read)
        });
        if read.truncated || !source_evaluated {
            return None;
        }
        let path = std::iter::once(&self.source_root)
            .chain(&self.collection)
            .chain(std::iter::once(&field.source_name))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("/");
        let input_delivered = evaluated.iter().any(|event| {
            matches!(event, TraceEvent::NodeInputValue { consumer, input, input_index: 0, positions: at, value }
                if *consumer == field.write_node && *input == field.input_node && at == positions && value == read)
        });
        match &field.function {
            None if &read.preview == actual => {
                Some(format!("Captured /{path}: {} → {actual}", read.preview))
            }
            Some(function) if input_delivered => Some(format!(
                "Captured /{path}: {} → {function} → {actual}",
                read.preview
            )),
            _ => None,
        }
    }
}

fn is_root(target: &TraceTarget, scope: &TraceScope) -> bool {
    &scope.target == target && scope.target_path.is_empty() && scope.structural_path.is_empty()
}

/// A finished scope describes how many candidates it accepted. It does not
/// identify any individual input item as the source of a serialized row.
pub(super) fn scope_counts(target: &PreviewTarget, trace: &TraceReport) -> Option<(usize, usize)> {
    let target = match target {
        PreviewTarget::Primary => TraceTarget::Primary,
        PreviewTarget::Named(name) => TraceTarget::Named(name.clone()),
    };
    let mut finished = trace.events.iter().filter_map(|event| match event {
        TraceEvent::ScopeFinished {
            scope,
            candidates,
            produced,
            ..
        } if is_root(&target, scope) => Some((*candidates, *produced)),
        _ => None,
    });
    let counts = finished.next()?;
    finished.next().is_none().then_some(counts)
}
