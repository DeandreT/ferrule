use ir::SchemaKind;

use super::super::{ProgramValidationError, SchemaCursor, SourceCatalog};
use crate::{IterationSource, TargetConstruction, TargetScope};

fn invalid(reason: &'static str) -> ProgramValidationError {
    ProgramValidationError::InvalidXmlBoundary {
        reason: reason.into(),
    }
}

#[derive(Clone)]
enum Frame<'a> {
    Item(SchemaCursor<'a>),
    Members(SchemaCursor<'a>),
    Wrapper(String, SchemaCursor<'a>),
}

impl Frame<'_> {
    fn owns(&self, path: &[String]) -> bool {
        match self {
            Self::Item(cursor) => cursor.owns_first(path),
            Self::Members(_) => path.is_empty(),
            Self::Wrapper(name, _) => path.first().is_none_or(|first| first == name),
        }
    }
}

/// Mirror the ordinary walk's frame selection: grouped members are a Repeated
/// value, not a group exposing their item fields to first-field selection.
fn walk<'a>(active: &[Frame<'a>], path: &[String]) -> Option<(SchemaCursor<'a>, Vec<Frame<'a>>)> {
    let mut candidates = Vec::new();
    let mut definite = false;
    for frame in active.iter().rev().filter(|frame| frame.owns(path)) {
        candidates.push(frame);
        // A named grouping wrapper always contains its member field. Ordinary
        // item fields can be absent, including schema-declared XML groups.
        if path.is_empty() || matches!(frame, Frame::Wrapper(..)) {
            definite = true;
            break;
        }
    }
    if !definite {
        candidates.push(active.last()?);
    }
    let mut selected: Option<(SchemaCursor<'a>, Vec<Frame<'a>>)> = None;
    for base in candidates {
        let Some(candidate) = walk_from(base, path) else {
            continue;
        };
        if let Some((current, frames)) = &selected {
            if !std::ptr::eq(current.node(), candidate.0.node())
                || frames.len() != candidate.1.len()
                || !frames
                    .iter()
                    .zip(&candidate.1)
                    .all(|(left, right)| match (left, right) {
                        (Frame::Item(left), Frame::Item(right)) => {
                            std::ptr::eq(left.node(), right.node())
                        }
                        _ => false,
                    })
            {
                return None;
            }
        } else {
            selected = Some(candidate);
        }
    }
    selected
}

fn walk_from<'a>(base: &Frame<'a>, path: &[String]) -> Option<(SchemaCursor<'a>, Vec<Frame<'a>>)> {
    let mut frames = Vec::new();
    let (mut current, rest) = match base {
        Frame::Item(cursor) => (*cursor, path),
        Frame::Members(cursor) => {
            frames.push(Frame::Item(*cursor));
            (*cursor, path)
        }
        Frame::Wrapper(name, cursor) => {
            let (first, rest) = path.split_first()?;
            if first != name {
                return None;
            }
            frames.push(Frame::Item(*cursor));
            (*cursor, rest)
        }
    };
    for segment in rest {
        current = current.follow(std::slice::from_ref(segment))?;
        if current.node().repeating {
            frames.push(Frame::Item(current));
        }
    }
    if !rest.is_empty() && !current.node().repeating {
        frames.push(Frame::Item(current));
    }
    Some((current, frames))
}

pub(super) fn validate_drivers<'a>(
    scope: &TargetScope,
    sources: SourceCatalog<'a>,
    parent: Option<SchemaCursor<'a>>,
    collected: bool,
) -> Result<(), ProgramValidationError> {
    let active = parent.into_iter().map(Frame::Item).collect::<Vec<_>>();
    validate_in_context(scope, sources, &active, collected)
}

fn validate_in_context<'a>(
    scope: &TargetScope,
    sources: SourceCatalog<'a>,
    active: &[Frame<'a>],
    collected: bool,
) -> Result<(), ProgramValidationError> {
    if matches!(scope.construction, TargetConstruction::CollectDocuments) {
        let mut child_context = active.to_vec();
        if let Some(iteration) = &scope.iteration {
            let IterationSource::Source(source) = iteration.input() else {
                return Err(invalid(
                    "XML document collectors require ordinary source contexts",
                ));
            };
            let (current, mut frames) = walk(active, source.path())
                .ok_or_else(|| invalid("XML document collector context must resolve to a Group"))?;
            if !matches!(current.node().kind, SchemaKind::Group { .. }) {
                return Err(invalid(
                    "XML document collector context must resolve to a Group",
                ));
            }
            if iteration.grouping().is_some() {
                // Grouping replaces the last walked item with a named wrapper
                // (for nonempty paths) followed by its repeated member list.
                if frames.pop().is_none() {
                    return Err(invalid(
                        "XML grouped document collector requires a walked collection",
                    ));
                }
                if let Some(name) = source.path().last() {
                    frames.push(Frame::Wrapper(name.clone(), current));
                }
                frames.push(Frame::Members(current));
            }
            child_context.extend(frames);
        }
        if scope.children.is_empty() {
            return Err(invalid(
                "XML document collectors require document-producing children",
            ));
        }
        for child in &scope.children {
            validate_in_context(child, sources, &child_context, true)?;
        }
        return Ok(());
    }
    let dynamic = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.dynamic_document_iteration())
        .ok_or_else(|| {
            invalid("XML document collection leaf requires a dynamic-document iteration")
        })?;
    let path = dynamic.source().path();
    let driver = if collected {
        walk(active, path).map(|(cursor, _)| cursor)
    } else if path.is_empty() {
        None
    } else {
        sources.root_schema_at(path)
    };
    if !driver.is_some_and(|cursor| {
        cursor.node().repeating && matches!(cursor.node().kind, SchemaKind::Group { .. })
    }) {
        return Err(invalid(
            "dynamic XML output driver must end in a repeating Group",
        ));
    }
    Ok(())
}
