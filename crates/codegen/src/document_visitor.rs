//! Explicit admission for incremental delivery of one selected ordinary target.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{
    Expression, IterationOutput, Program, ProgramValidationError, TargetConstruction, TargetScope,
};

#[derive(Debug)]
pub enum DocumentVisitorBoundaryError {
    InvalidProgram(ProgramValidationError),
    UnknownTarget(String),
    Unsupported(&'static str),
}

impl fmt::Display for DocumentVisitorBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(error) => error.fmt(f),
            Self::UnknownTarget(name) => write!(f, "unknown document visitor target {name:?}"),
            Self::Unsupported(reason) => write!(f, "document visitor: {reason}"),
        }
    }
}

impl std::error::Error for DocumentVisitorBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidProgram(error) => Some(error),
            _ => None,
        }
    }
}

/// Select at emission time: `None` is the primary target, `Some` an exact name.
/// This does not admit owned-source streaming, nested documents or publication.
pub fn validate_document_visitor(
    program: &Program,
    target: Option<&str>,
) -> Result<Option<usize>, DocumentVisitorBoundaryError> {
    crate::validate_program(program).map_err(DocumentVisitorBoundaryError::InvalidProgram)?;
    if crate::validate::has_owned(program) {
        return Err(DocumentVisitorBoundaryError::Unsupported(
            "owned-source streaming requires separate qualification",
        ));
    }
    let index = target
        .map(|name| {
            program
                .extra_targets
                .iter()
                .position(|target| target.name == name)
                .ok_or_else(|| DocumentVisitorBoundaryError::UnknownTarget(name.into()))
        })
        .transpose()?;
    let root = index.map_or(&program.root, |index| &program.extra_targets[index].root);
    if root.iteration.as_ref().is_none_or(|iteration| {
        iteration.dynamic_document_iteration().is_none()
            || iteration.output() != IterationOutput::Repeated
    }) {
        return Err(DocumentVisitorBoundaryError::Unsupported(
            "the selected target must use ordinary dynamic documents",
        ));
    }
    if nested_documents(root) {
        return Err(DocumentVisitorBoundaryError::Unsupported(
            "nested document-output scopes are not supported",
        ));
    }
    Ok(index)
}

/// Admit filename-only clocks without opening UDF, owned or private producers.
pub fn validate_document_filename_clock(
    program: &Program,
    target: Option<&str>,
) -> Result<Option<usize>, DocumentVisitorBoundaryError> {
    let index = validate_document_visitor(program, target)?;
    let root = index.map_or(&program.root, |index| &program.extra_targets[index].root);
    let path = root
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.dynamic_document_iteration())
        .expect("validated document iteration")
        .output_path();
    let expressions = program
        .expressions
        .iter()
        .map(|node| (node.id, &node.expression))
        .collect::<BTreeMap<_, _>>();
    let mut pending = vec![path];
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        let expression = expressions[&node];
        if !matches!(
            expression,
            Expression::Const { .. }
                | Expression::RuntimeValue { .. }
                | Expression::RuntimeParameter { .. }
                | Expression::RuntimeParameterDefault { .. }
                | Expression::SourceField { .. }
                | Expression::SourceDocumentPath
                | Expression::SourceRowDocumentPath { .. }
                | Expression::Position { .. }
                | Expression::Call { .. }
                | Expression::If { .. }
                | Expression::Aggregate { .. }
                | Expression::CollectionFind { .. }
                | Expression::SequenceItemAt {
                    sequence: crate::GeneratedSequence::TokenizeRegex { .. },
                    ..
                }
                | Expression::DistinctTextMapLast { .. }
        ) {
            return Err(DocumentVisitorBoundaryError::Unsupported(
                "filename clock sampling is limited to ordinary scalar graph expressions",
            ));
        }
        pending.extend(crate::validate::graph_dependencies::of(expression));
    }
    Ok(index)
}

/// Both clocks are explicit and separate. Ordinary entry points keep the
/// execution timestamp, while this bounded profile samples reached body reads.
pub fn validate_document_clocks(
    program: &Program,
    target: Option<&str>,
) -> Result<Option<usize>, DocumentVisitorBoundaryError> {
    validate_body_clocks(program, target, false)
}

/// Separate composition for document bodies with ordinary static-message errors.
/// Error messages containing source reads, clocks or calls remain outside it.
pub fn validate_document_clocks_with_static_errors(
    program: &Program,
    target: Option<&str>,
) -> Result<Option<usize>, DocumentVisitorBoundaryError> {
    validate_body_clocks(program, target, true)
}

fn validate_body_clocks(
    program: &Program,
    target: Option<&str>,
    static_errors: bool,
) -> Result<Option<usize>, DocumentVisitorBoundaryError> {
    let index = validate_document_filename_clock(program, target)?;
    if !program.user_functions.is_empty() || !program.failure_rules.is_empty() {
        return Err(DocumentVisitorBoundaryError::Unsupported(
            "body clocks require a separate profile for user functions and failure rules",
        ));
    }
    let static_messages = program
        .expressions
        .iter()
        .filter_map(|node| {
            matches!(
                node.expression,
                Expression::Const {
                    value: ir::Value::String(_)
                }
            )
            .then_some(node.id)
        })
        .collect::<BTreeSet<_>>();
    for node in &program.expressions {
        let static_error = static_errors
            && matches!(node.expression, Expression::Raise { message } if message.is_none_or(|id| static_messages.contains(&id)));
        if !static_error
            && !matches!(
                node.expression,
                Expression::Const { .. }
                    | Expression::RuntimeValue { .. }
                    | Expression::RuntimeParameter { .. }
                    | Expression::RuntimeParameterDefault { .. }
                    | Expression::SourceField { .. }
                    | Expression::SourceDocumentPath
                    | Expression::SourceRowDocumentPath { .. }
                    | Expression::Position { .. }
                    | Expression::Call { .. }
                    | Expression::If { .. }
                    | Expression::Aggregate { .. }
                    | Expression::CollectionFind { .. }
                    | Expression::SequenceItemAt {
                        sequence: crate::GeneratedSequence::TokenizeRegex { .. },
                        ..
                    }
                    | Expression::InvocationCounter { .. }
                    | Expression::DistinctTextMapLast { .. }
            )
        {
            return Err(DocumentVisitorBoundaryError::Unsupported(
                "body clock sampling is limited to ordinary scalar graph expressions",
            ));
        }
    }
    Ok(index)
}

fn nested_documents(root: &TargetScope) -> bool {
    let mut pending = root.children.iter().collect::<Vec<_>>();
    if let TargetConstruction::DynamicGroup { children, .. } = &root.construction {
        pending.extend(children.iter().map(|child| &child.scope));
    }
    while let Some(child) = pending.pop() {
        if child
            .iteration
            .as_ref()
            .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some())
        {
            return true;
        }
        pending.extend(child.children.iter());
        if let Some(sequence) = child
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.concatenated())
        {
            pending.extend(sequence.iter());
        }
        if let TargetConstruction::DynamicGroup { children, .. } = &child.construction {
            pending.extend(children.iter().map(|child| &child.scope));
        }
    }
    false
}

#[derive(Debug)]
pub enum DocumentVisitorEmitError<E> {
    Boundary(DocumentVisitorBoundaryError),
    Emit(E),
}

impl<E: fmt::Display> fmt::Display for DocumentVisitorEmitError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Emit(error) => error.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DocumentVisitorEmitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Boundary(error) => error,
            Self::Emit(error) => error,
        })
    }
}
