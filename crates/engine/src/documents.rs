//! Incremental delivery of complete ordinary target documents.
//!
//! This layer does not open files or claim physical publication. A consumer
//! accepts a document with `Continue` or returns its own stop reason with
//! `Break`. Existing materializing entry points remain unchanged.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use ir::{DocumentMember, Instance};
use mapping::{IterationOutput, Node, Project, Scope, ScopeIteration};
use thiserror::Error;

use crate::{EngineError, ExecutionContext, TargetSelection, TraceScope};

/// Explicit host clock for reached date-time reads in a document filename.
/// Body expressions keep the stable date-time in `ExecutionContext`.
pub trait DocumentFilenameClock {
    fn sample_datetime(&self) -> Result<String, String>;
}

impl<F: Fn() -> Result<String, String>> DocumentFilenameClock for F {
    fn sample_datetime(&self) -> Result<String, String> {
        self()
    }
}

/// Reached body-clock reads, selected separately from stable and filename clocks.
pub trait DocumentBodyClock {
    fn sample_datetime(&self) -> Result<String, String>;
}

impl<F: Fn() -> Result<String, String>> DocumentBodyClock for F {
    fn sample_datetime(&self) -> Result<String, String> {
        self()
    }
}

#[derive(Clone, Copy, Default)]
pub struct DocumentVisitOptions<'a> {
    filename_clock: Option<&'a dyn DocumentFilenameClock>,
    body_clock: Option<&'a dyn DocumentBodyClock>,
}

impl<'a> DocumentVisitOptions<'a> {
    pub fn with_filename_clock(mut self, clock: &'a dyn DocumentFilenameClock) -> Self {
        self.filename_clock = Some(clock);
        self
    }

    /// Sample each reached body date-time expression without changing the
    /// stored execution timestamp or the separately selected filename clock.
    pub fn with_body_clock(mut self, clock: &'a dyn DocumentBodyClock) -> Self {
        self.body_clock = Some(clock);
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "distinguish complete delivery from a consumer-requested stop"]
pub enum DocumentVisit<B> {
    Complete { accepted: usize },
    Stopped { accepted: usize, reason: B },
}

/// An evaluation failure does not revoke documents already accepted by the
/// consumer. No failed or stopped document is counted as accepted.
#[derive(Debug, Error, PartialEq)]
#[error("document evaluation failed after {accepted} accepted documents: {source}")]
pub struct DocumentVisitError {
    pub accepted: usize,
    #[source]
    pub source: EngineError,
}

fn check_scope(scope: &Scope, stored_path: bool) -> Result<(), EngineError> {
    if !matches!(scope.iteration, ScopeIteration::DynamicDocuments { .. })
        || scope.iteration_output != IterationOutput::Repeated
        || stored_path
    {
        return Err(EngineError::DocumentStreamUnsupported {
            reason: "the selected target requires ordinary dynamic documents without a stored path",
        });
    }
    let mut pending = scope.children.iter().collect::<Vec<_>>();
    pending.extend(scope.dynamic_children.iter().map(|child| &child.scope));
    while let Some(child) = pending.pop() {
        if child.output_path().is_some() {
            return Err(EngineError::DocumentStreamUnsupported {
                reason: "nested document-output scopes are not supported",
            });
        }
        pending.extend(child.children.iter());
        pending.extend(child.dynamic_children.iter().map(|child| &child.scope));
        if let Some(sequence) = child.concatenated() {
            pending.extend(sequence.iter());
        }
    }
    Ok(())
}

fn check_clock_path(project: &Project, root: &Scope) -> Result<(), EngineError> {
    let mut pending = vec![root.output_path().expect("document scope checked")];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let node = project
            .graph
            .nodes
            .get(&id)
            .ok_or(EngineError::MissingNode(id))?;
        match node {
            Node::Const { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::RuntimeParameterDefault { .. }
            | Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::Position { .. }
            | Node::Call { .. }
            | Node::If { .. }
            | Node::Aggregate { .. }
            | Node::DistinctTextMapLast { .. } => {}
            _ => {
                return Err(EngineError::DocumentStreamUnsupported {
                    reason: "filename clock sampling is limited to ordinary scalar graph expressions",
                });
            }
        }
        pending.extend(node.dependencies());
    }
    Ok(())
}

fn check_body_clock(project: &Project) -> Result<(), EngineError> {
    if !project.user_functions.is_empty() || !project.failure_rules.is_empty() {
        return Err(EngineError::DocumentStreamUnsupported {
            reason: "body clocks require a separate profile for user functions and failure rules",
        });
    }
    for node in project.graph.nodes.values() {
        if !matches!(
            node,
            Node::Const { .. }
                | Node::RuntimeValue { .. }
                | Node::RuntimeParameter { .. }
                | Node::RuntimeParameterDefault { .. }
                | Node::SourceField { .. }
                | Node::SourceDocumentPath
                | Node::Position { .. }
                | Node::Call { .. }
                | Node::If { .. }
                | Node::Aggregate { .. }
                | Node::InvocationCounter { .. }
                | Node::DistinctTextMapLast { .. }
        ) {
            return Err(EngineError::DocumentStreamUnsupported {
                reason: "body clock sampling is limited to ordinary scalar graph expressions",
            });
        }
    }
    Ok(())
}

/// Visits complete documents of one selected target in evaluation order.
///
/// A document's body and then its filename are evaluated before the consumer
/// runs. Returning `Break` prevents evaluation of later document bodies and
/// paths. Mapping-wide preparation (such as grouping) still follows the normal
/// interpreter contract. One fresh invocation counter table covers the entire
/// visit, including all documents, and is discarded when the call ends.
///
/// This initially admits only ordinary sources. It does not open an owned-source
/// streaming profile, alter filename clock sampling, or enable MFD import.
pub fn visit_target_documents<B>(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
    selection: TargetSelection<'_>,
    consumer: impl FnMut(DocumentMember) -> ControlFlow<B>,
) -> Result<DocumentVisit<B>, DocumentVisitError> {
    visit_target_documents_with_options(
        project,
        source,
        extras,
        execution,
        selection,
        DocumentVisitOptions::default(),
        consumer,
    )
}

/// Like `visit_target_documents`, with separately selected body and filename clocks.
/// A filename clock is reached only after its complete body. A body clock is
/// called at each reached date-time expression in the admitted ordinary graph;
/// neither option changes the default stable-clock entry points.
pub fn visit_target_documents_with_options<B>(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
    selection: TargetSelection<'_>,
    options: DocumentVisitOptions<'_>,
    mut consumer: impl FnMut(DocumentMember) -> ControlFlow<B>,
) -> Result<DocumentVisit<B>, DocumentVisitError> {
    let before = |source| DocumentVisitError {
        accepted: 0,
        source,
    };
    let named = crate::required_sources::selected_target(project, selection).map_err(before)?;
    let admission =
        crate::source_ownership::admit_project(project, Some(selection)).map_err(before)?;
    if admission.used_owned {
        return Err(before(EngineError::DocumentStreamUnsupported {
            reason: "owned-source document streaming requires separate qualification",
        }));
    }
    let (root, schema, path, trace) = match named {
        None => (
            &project.root,
            &project.target,
            project.target_path.is_some(),
            TraceScope::primary(),
        ),
        Some(target) => {
            let index = project
                .extra_targets
                .iter()
                .position(|candidate| std::ptr::eq(candidate, target))
                .expect("selected target belongs to the project");
            (
                &target.root,
                &target.schema,
                target.path.is_some(),
                TraceScope::named(&target.name, index),
            )
        }
    };
    check_scope(root, path).map_err(before)?;
    if root.construction == mapping::ScopeConstruction::DocumentRows {
        crate::eval_scope::check_document_rows(root, Some(schema), &trace).map_err(before)?;
    }
    if options.filename_clock.is_some() {
        check_clock_path(project, root).map_err(before)?;
    }
    if options.body_clock.is_some() {
        check_body_clock(project).map_err(before)?;
    }
    let mut accepted = 0;
    let mut stopped = None;
    let result = {
        let mut offer = |document| match consumer(document) {
            ControlFlow::Continue(()) => {
                accepted += 1;
                true
            }
            ControlFlow::Break(reason) => {
                stopped = Some(reason);
                false
            }
        };
        crate::evaluate_run(
            project,
            source,
            extras,
            Some(execution),
            false,
            |program, context| {
                crate::eval_scope::eval_scope_with_document_visitor(
                    program
                        .with_document_filename_clock(options.filename_clock)
                        .with_document_body_clock(options.body_clock),
                    root,
                    Some(schema),
                    context,
                    &[],
                    &project.extra_sources,
                    execution.dynamic_source_loader,
                    &trace,
                    Some(&mut offer),
                )
                .map(|_| ())
            },
        )
    };
    result.map_err(|source| DocumentVisitError { accepted, source })?;
    Ok(match stopped {
        Some(reason) => DocumentVisit::Stopped { accepted, reason },
        None => DocumentVisit::Complete { accepted },
    })
}
