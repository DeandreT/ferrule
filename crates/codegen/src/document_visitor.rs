//! Explicit admission for incremental delivery of one selected ordinary target.

use std::fmt;

use crate::{IterationOutput, Program, ProgramValidationError, TargetConstruction, TargetScope};

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
