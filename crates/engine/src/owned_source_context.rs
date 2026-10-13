//! Borrowed endpoint roots for the first owner-aware interpreter foundation.
//!
//! This module is test-scoped until execution admission is implemented. Inputs
//! are the existing host values before construction of the legacy wrapper.

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{NamedSource, OwnedSourceField, Project, SourceOwner};

/// Identity comes from the declaration, independently of equal input values.
#[derive(Debug, Clone, Copy)]
pub(super) enum DeclaredSource<'a> {
    Primary {
        schema: &'a SchemaNode,
    },
    Named {
        index: usize,
        declaration: &'a NamedSource,
    },
}

impl PartialEq for DeclaredSource<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Primary { schema: left }, Self::Primary { schema: right }) => {
                std::ptr::eq(*left, *right)
            }
            (
                Self::Named {
                    index: left_index,
                    declaration: left,
                },
                Self::Named {
                    index: right_index,
                    declaration: right,
                },
            ) => left_index == right_index && std::ptr::eq(*left, *right),
            _ => false,
        }
    }
}

impl Eq for DeclaredSource<'_> {}

impl<'a> DeclaredSource<'a> {
    fn schema(self) -> &'a SchemaNode {
        match self {
            Self::Primary { schema } => schema,
            Self::Named { declaration, .. } => &declaration.schema,
        }
    }
}

/// Absence is independent of a supplied scalar Null.
#[derive(Debug, Clone, Copy)]
pub(super) struct RootEndpoint<'a> {
    pub(super) declaration: DeclaredSource<'a>,
    pub(super) root: Option<&'a Instance>,
}

impl PartialEq for RootEndpoint<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.declaration == other.declaration
            && match (self.root, other.root) {
                (None, None) => true,
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                _ => false,
            }
    }
}

impl Eq for RootEndpoint<'_> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceResolutionError<'a, 'request> {
    UnknownOwner {
        owner: &'request SourceOwner,
    },
    AmbiguousOwner {
        owner: &'request SourceOwner,
        first_index: usize,
        second_index: usize,
    },
    AmbiguousInput {
        declaration: DeclaredSource<'a>,
        first_index: usize,
        second_index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepetitionOrigin {
    Schema,
    Instance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnsupportedSourceDomain {
    DynamicSource,
    DocumentSet,
    MappedSequence,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RootReadError<'a, 'request> {
    Owner {
        request: &'request OwnedSourceField,
        error: SourceResolutionError<'a, 'request>,
    },
    NonRootFrame {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
    },
    UnsupportedProvenance {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
        prefix_len: usize,
        domain: UnsupportedSourceDomain,
    },
    RepeatingPath {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
        prefix_len: usize,
        origin: RepetitionOrigin,
    },
    UnknownScalarPath {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
        prefix_len: usize,
    },
    NonScalarPath {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
        prefix_len: usize,
    },
    MissingEndpoint {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
    },
    InvalidInstanceShape {
        declaration: DeclaredSource<'a>,
        request: &'request OwnedSourceField,
        prefix_len: usize,
        value: &'a Instance,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RootScalar<'a, 'request> {
    pub(super) endpoint: RootEndpoint<'a>,
    /// Retains None versus an explicit empty root frame without normalization.
    pub(super) request: &'request OwnedSourceField,
    /// A declared field can be absent; a present Null is Some(Value::Null).
    pub(super) value: Option<&'a Value>,
}

#[derive(Debug)]
pub(super) struct OwnedSourceContext<'a> {
    project: &'a Project,
    primary: Option<&'a Instance>,
    named: &'a [(String, Instance)],
}

impl<'a> OwnedSourceContext<'a> {
    pub(super) fn new(
        project: &'a Project,
        primary: Option<&'a Instance>,
        named: &'a [(String, Instance)],
    ) -> Self {
        Self {
            project,
            primary,
            named,
        }
    }

    pub(super) fn resolve<'request>(
        &self,
        owner: &'request SourceOwner,
    ) -> Result<RootEndpoint<'a>, SourceResolutionError<'a, 'request>> {
        let SourceOwner::Named { name } = owner else {
            return Ok(RootEndpoint {
                declaration: DeclaredSource::Primary {
                    schema: &self.project.source,
                },
                root: self.primary,
            });
        };
        let mut declarations = self
            .project
            .extra_sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == *name);
        let Some((index, declaration)) = declarations.next() else {
            return Err(SourceResolutionError::UnknownOwner { owner });
        };
        if let Some((second_index, _)) = declarations.next() {
            return Err(SourceResolutionError::AmbiguousOwner {
                owner,
                first_index: index,
                second_index,
            });
        }
        let declaration = DeclaredSource::Named { index, declaration };
        let mut inputs = self
            .named
            .iter()
            .enumerate()
            .filter(|(_, (input_name, _))| input_name == name);
        let first = inputs.next();
        if let (Some((first_index, _)), Some((second_index, _))) = (first, inputs.next()) {
            return Err(SourceResolutionError::AmbiguousInput {
                declaration,
                first_index,
                second_index,
            });
        }
        Ok(RootEndpoint {
            declaration,
            root: first.map(|(_, (_, value))| value),
        })
    }

    pub(super) fn scalar<'request>(
        &self,
        request: &'request OwnedSourceField,
    ) -> Result<RootScalar<'a, 'request>, RootReadError<'a, 'request>> {
        let endpoint = self
            .resolve(&request.source.owner)
            .map_err(|error| RootReadError::Owner { request, error })?;
        let declaration = endpoint.declaration;
        if request
            .frame
            .as_ref()
            .is_some_and(|frame| !frame.is_empty())
        {
            return Err(RootReadError::NonRootFrame {
                declaration,
                request,
            });
        }
        if matches!(declaration, DeclaredSource::Named { declaration, .. } if declaration.dynamic_path.is_some())
        {
            return Err(RootReadError::UnsupportedProvenance {
                declaration,
                request,
                prefix_len: 0,
                domain: UnsupportedSourceDomain::DynamicSource,
            });
        }
        // Validate the whole declared path before missing instance data can
        // hide repetition. Schema/endpoint names are never path prefixes.
        let mut schema = declaration.schema();
        for (prefix_len, segment) in request.source.path.iter().enumerate() {
            reject_repeating_schema(declaration, request, schema, prefix_len)?;
            let SchemaKind::Group { children, .. } = &schema.kind else {
                return Err(RootReadError::NonScalarPath {
                    declaration,
                    request,
                    prefix_len,
                });
            };
            schema = children.iter().find(|child| child.name == *segment).ok_or(
                RootReadError::UnknownScalarPath {
                    declaration,
                    request,
                    prefix_len: prefix_len + 1,
                },
            )?;
        }
        let end = request.source.path.len();
        reject_repeating_schema(declaration, request, schema, end)?;
        if !schema.is_scalar() {
            return Err(RootReadError::NonScalarPath {
                declaration,
                request,
                prefix_len: end,
            });
        }
        let mut current = endpoint.root.ok_or(RootReadError::MissingEndpoint {
            declaration,
            request,
        })?;
        for (prefix_len, segment) in request.source.path.iter().enumerate() {
            reject_instance_domain(declaration, request, current, prefix_len)?;
            let Instance::Group(fields) = current else {
                return Err(RootReadError::InvalidInstanceShape {
                    declaration,
                    request,
                    prefix_len,
                    value: current,
                });
            };
            let Some((_, value)) = fields.iter().find(|(name, _)| name == segment) else {
                return Ok(RootScalar {
                    endpoint,
                    request,
                    value: None,
                });
            };
            current = value;
        }
        reject_instance_domain(declaration, request, current, end)?;
        let Instance::Scalar(value) = current else {
            return Err(RootReadError::InvalidInstanceShape {
                declaration,
                request,
                prefix_len: end,
                value: current,
            });
        };
        Ok(RootScalar {
            endpoint,
            request,
            value: Some(value),
        })
    }
}

fn reject_repeating_schema<'a, 'request>(
    declaration: DeclaredSource<'a>,
    request: &'request OwnedSourceField,
    schema: &SchemaNode,
    prefix_len: usize,
) -> Result<(), RootReadError<'a, 'request>> {
    if schema.repeating {
        Err(RootReadError::RepeatingPath {
            declaration,
            request,
            prefix_len,
            origin: RepetitionOrigin::Schema,
        })
    } else {
        Ok(())
    }
}

fn reject_instance_domain<'a, 'request>(
    declaration: DeclaredSource<'a>,
    request: &'request OwnedSourceField,
    value: &Instance,
    prefix_len: usize,
) -> Result<(), RootReadError<'a, 'request>> {
    let domain = match value {
        Instance::Scalar(_) | Instance::Group(_) => return Ok(()),
        Instance::Repeated(_) => {
            return Err(RootReadError::RepeatingPath {
                declaration,
                request,
                prefix_len,
                origin: RepetitionOrigin::Instance,
            });
        }
        Instance::DocumentSet(_) => UnsupportedSourceDomain::DocumentSet,
        Instance::MappedSequence(_) => UnsupportedSourceDomain::MappedSequence,
    };
    Err(RootReadError::UnsupportedProvenance {
        declaration,
        request,
        prefix_len,
        domain,
    })
}

use std::num::NonZeroUsize;
use std::rc::Rc;

use mapping::{OwnedPosition, OwnedSourceRef};

/// Raw source, post-sort selection, and compact filtered/window output indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PositionPhase {
    Structural,
    Selection,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameProvenance {
    Basic,
    Generated,
    Function,
    Join,
    Document,
    Dynamic,
}

/// Metadata only. Every item and parent occurrence borrows the registered input.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SourceFrame<'a> {
    pub(super) endpoint: RootEndpoint<'a>,
    pub(super) collection: Vec<String>,
    pub(super) item: &'a Instance,
    pub(super) parent: Option<Rc<SourceFrame<'a>>>,
    pub(super) structural: NonZeroUsize,
    pub(super) selection: NonZeroUsize,
    pub(super) output: Option<NonZeroUsize>,
    pub(super) provenance: FrameProvenance,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SourceWalk<'a> {
    pub(super) endpoint: RootEndpoint<'a>,
    pub(super) frames: Vec<Rc<SourceFrame<'a>>>,
    pub(super) item: &'a Instance,
    /// Identity re-entry introduces no frame and must not renumber an outer scope.
    pub(super) introduced: Option<usize>,
}

impl SourceWalk<'_> {
    /// Sort changes selection numbering, never structural ancestry or outer frames.
    pub(super) fn sorted(mut self, position: NonZeroUsize) -> Self {
        if let Some(frame) = self.introduced.and_then(|index| self.frames.get_mut(index)) {
            let frame = Rc::make_mut(frame);
            frame.selection = position;
            frame.output = None;
        }
        self
    }

    /// Filtered/window output bindings have compact indexes independent of selection.
    pub(super) fn produced(mut self, position: NonZeroUsize) -> Self {
        if let Some(frame) = self.introduced.and_then(|index| self.frames.get_mut(index)) {
            Rc::make_mut(frame).output = Some(position);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextRequest<'request> {
    Walk(&'request OwnedSourceRef),
    Scalar(&'request OwnedSourceField),
    Position(&'request OwnedPosition),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ContextFault<'a, 'request> {
    Owner(SourceResolutionError<'a, 'request>),
    MissingEndpoint {
        declaration: DeclaredSource<'a>,
    },
    UnknownPath {
        declaration: DeclaredSource<'a>,
        prefix_len: usize,
    },
    NonCollection {
        declaration: DeclaredSource<'a>,
    },
    NonScalar {
        declaration: DeclaredSource<'a>,
    },
    UnsupportedDomain {
        declaration: DeclaredSource<'a>,
        prefix_len: usize,
        domain: UnsupportedSourceDomain,
    },
    UnsupportedFrame {
        frame: Rc<SourceFrame<'a>>,
    },
    InvalidFrame {
        frame: Rc<SourceFrame<'a>>,
    },
    InactiveFrame {
        declaration: DeclaredSource<'a>,
    },
    PathOutsideFrame {
        frame: Rc<SourceFrame<'a>>,
    },
    UniteratedCollection {
        declaration: DeclaredSource<'a>,
        prefix_len: usize,
    },
    InvalidShape {
        declaration: DeclaredSource<'a>,
        prefix_len: usize,
        value: &'a Instance,
    },
    PhaseUnavailable {
        frame: Rc<SourceFrame<'a>>,
        phase: PositionPhase,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ContextError<'a, 'request> {
    pub(super) request: ContextRequest<'request>,
    pub(super) fault: ContextFault<'a, 'request>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FrameScalar<'a, 'request> {
    pub(super) endpoint: RootEndpoint<'a>,
    pub(super) request: &'request OwnedSourceField,
    pub(super) frame: Option<Rc<SourceFrame<'a>>>,
    pub(super) value: Option<&'a Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SourcePosition<'a, 'request> {
    pub(super) endpoint: RootEndpoint<'a>,
    pub(super) request: &'request OwnedPosition,
    pub(super) frame: Rc<SourceFrame<'a>>,
    pub(super) phase: PositionPhase,
    pub(super) index: NonZeroUsize,
}

impl<'a> OwnedSourceContext<'a> {
    pub(super) fn walk<'request>(
        &self,
        source: &'request OwnedSourceRef,
        active: &[Rc<SourceFrame<'a>>],
    ) -> Result<Vec<SourceWalk<'a>>, ContextError<'a, 'request>> {
        let request = ContextRequest::Walk(source);
        let endpoint = self.context_endpoint(&source.owner, request)?;
        let schema = context_schema(endpoint.declaration, &source.path, request)?;
        if !source.path.is_empty() && !schema.repeating {
            return Err(ContextError {
                request,
                fault: ContextFault::NonCollection {
                    declaration: endpoint.declaration,
                },
            });
        }
        let parent = self.active_prefix(endpoint, &source.path, active, request)?;
        let (base, consumed) = match &parent {
            Some(frame) => (frame.item, frame.collection.len()),
            None => (
                endpoint.root.ok_or(ContextError {
                    request,
                    fault: ContextFault::MissingEndpoint {
                        declaration: endpoint.declaration,
                    },
                })?,
                0,
            ),
        };
        let mut output = Vec::new();
        walk_source(
            endpoint,
            source,
            base,
            consumed,
            parent,
            active,
            &mut output,
        )?;
        Ok(output)
    }

    pub(super) fn framed_scalar<'request>(
        &self,
        source: &'request OwnedSourceField,
        active: &[Rc<SourceFrame<'a>>],
    ) -> Result<FrameScalar<'a, 'request>, ContextError<'a, 'request>> {
        let request = ContextRequest::Scalar(source);
        let endpoint = self.context_endpoint(&source.source.owner, request)?;
        let schema = context_schema(endpoint.declaration, &source.source.path, request)?;
        if !schema.is_scalar() {
            return Err(ContextError {
                request,
                fault: ContextFault::NonScalar {
                    declaration: endpoint.declaration,
                },
            });
        }
        let frame = match &source.frame {
            Some(path) if path.is_empty() => {
                // [] pins this exact owner root. A repeated root needs its
                // real active occurrence; a nonrepeating root needs no seed.
                match active.iter().rev().find(|frame| {
                    frame.endpoint.declaration == endpoint.declaration
                        && frame.collection.is_empty()
                }) {
                    Some(frame) => {
                        Self::valid_frame(endpoint, frame, request)?;
                        Some(Rc::clone(frame))
                    }
                    None => {
                        let root = endpoint
                            .root
                            .expect("context endpoint presence was checked");
                        if endpoint.declaration.schema().repeating
                            || matches!(root, Instance::Repeated(_))
                        {
                            return Err(ContextError {
                                request,
                                fault: ContextFault::InactiveFrame {
                                    declaration: endpoint.declaration,
                                },
                            });
                        }
                        reject_context_domain(endpoint.declaration, root, 0, request)?;
                        None
                    }
                }
            }
            Some(path) => Some(self.exact_frame(endpoint, path, active, request)?),
            None => self.active_prefix(endpoint, &source.source.path, active, request)?,
        };
        let mut occurrence_schema = endpoint.declaration.schema();
        for prefix_len in 0..=source.source.path.len() {
            if occurrence_schema.repeating
                && !frame
                    .as_ref()
                    .is_some_and(|frame| prefix_len <= frame.collection.len())
            {
                return Err(ContextError {
                    request,
                    fault: ContextFault::UniteratedCollection {
                        declaration: endpoint.declaration,
                        prefix_len,
                    },
                });
            }
            if let Some(segment) = source.source.path.get(prefix_len) {
                let SchemaKind::Group { children, .. } = &occurrence_schema.kind else {
                    unreachable!("complete scalar path was validated")
                };
                occurrence_schema = children
                    .iter()
                    .find(|child| child.name == *segment)
                    .expect("complete scalar path was validated");
            }
        }
        let (mut value, consumed) = match &frame {
            Some(frame) => {
                if !source.source.path.starts_with(&frame.collection) {
                    return Err(ContextError {
                        request,
                        fault: ContextFault::PathOutsideFrame {
                            frame: Rc::clone(frame),
                        },
                    });
                }
                (frame.item, frame.collection.len())
            }
            None => (
                endpoint.root.ok_or(ContextError {
                    request,
                    fault: ContextFault::MissingEndpoint {
                        declaration: endpoint.declaration,
                    },
                })?,
                0,
            ),
        };
        for (offset, segment) in source.source.path[consumed..].iter().enumerate() {
            reject_context_domain(endpoint.declaration, value, consumed + offset, request)?;
            let Instance::Group(fields) = value else {
                return Err(ContextError {
                    request,
                    fault: ContextFault::InvalidShape {
                        declaration: endpoint.declaration,
                        prefix_len: consumed + offset,
                        value,
                    },
                });
            };
            let Some((_, child)) = fields.iter().find(|(name, _)| name == segment) else {
                return Ok(FrameScalar {
                    endpoint,
                    request: source,
                    frame,
                    value: None,
                });
            };
            value = child;
        }
        reject_context_domain(
            endpoint.declaration,
            value,
            source.source.path.len(),
            request,
        )?;
        let Instance::Scalar(value) = value else {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidShape {
                    declaration: endpoint.declaration,
                    prefix_len: source.source.path.len(),
                    value,
                },
            });
        };
        Ok(FrameScalar {
            endpoint,
            request: source,
            frame,
            value: Some(value),
        })
    }

    pub(super) fn position<'request>(
        &self,
        source: &'request OwnedPosition,
        active: &[Rc<SourceFrame<'a>>],
        phase: PositionPhase,
    ) -> Result<SourcePosition<'a, 'request>, ContextError<'a, 'request>> {
        let request = ContextRequest::Position(source);
        let endpoint = self.context_endpoint(&source.owner, request)?;
        let frame = match &source.collection {
            Some(path) => self.exact_frame(endpoint, path, active, request)?,
            None => {
                let frame = active
                    .iter()
                    .rev()
                    .find(|frame| frame.endpoint.declaration == endpoint.declaration)
                    .ok_or(ContextError {
                        request,
                        fault: ContextFault::InactiveFrame {
                            declaration: endpoint.declaration,
                        },
                    })?;
                Self::valid_frame(endpoint, frame, request)?;
                Rc::clone(frame)
            }
        };
        let index = match phase {
            PositionPhase::Structural => frame.structural,
            PositionPhase::Selection => frame.selection,
            PositionPhase::Output => frame.output.ok_or_else(|| ContextError {
                request,
                fault: ContextFault::PhaseUnavailable {
                    frame: Rc::clone(&frame),
                    phase,
                },
            })?,
        };
        Ok(SourcePosition {
            endpoint,
            request: source,
            frame,
            phase,
            index,
        })
    }

    fn context_endpoint<'request>(
        &self,
        owner: &'request SourceOwner,
        request: ContextRequest<'request>,
    ) -> Result<RootEndpoint<'a>, ContextError<'a, 'request>> {
        let endpoint = self.resolve(owner).map_err(|error| ContextError {
            request,
            fault: ContextFault::Owner(error),
        })?;
        if matches!(endpoint.declaration, DeclaredSource::Named { declaration, .. } if declaration.dynamic_path.is_some())
        {
            return Err(ContextError {
                request,
                fault: ContextFault::UnsupportedDomain {
                    declaration: endpoint.declaration,
                    prefix_len: 0,
                    domain: UnsupportedSourceDomain::DynamicSource,
                },
            });
        }
        if endpoint.root.is_none() {
            return Err(ContextError {
                request,
                fault: ContextFault::MissingEndpoint {
                    declaration: endpoint.declaration,
                },
            });
        }
        Ok(endpoint)
    }

    fn exact_frame<'request>(
        &self,
        endpoint: RootEndpoint<'a>,
        path: &[String],
        active: &[Rc<SourceFrame<'a>>],
        request: ContextRequest<'request>,
    ) -> Result<Rc<SourceFrame<'a>>, ContextError<'a, 'request>> {
        let frame = active
            .iter()
            .rev()
            .find(|frame| {
                frame.endpoint.declaration == endpoint.declaration && frame.collection == path
            })
            .ok_or(ContextError {
                request,
                fault: ContextFault::InactiveFrame {
                    declaration: endpoint.declaration,
                },
            })?;
        Self::valid_frame(endpoint, frame, request)?;
        Ok(Rc::clone(frame))
    }

    fn active_prefix<'request>(
        &self,
        endpoint: RootEndpoint<'a>,
        path: &[String],
        active: &[Rc<SourceFrame<'a>>],
        request: ContextRequest<'request>,
    ) -> Result<Option<Rc<SourceFrame<'a>>>, ContextError<'a, 'request>> {
        let frame = active
            .iter()
            .filter(|frame| {
                frame.endpoint.declaration == endpoint.declaration
                    && path.starts_with(&frame.collection)
            })
            .max_by_key(|frame| frame.collection.len());
        frame
            .map(|frame| {
                Self::valid_frame(endpoint, frame, request)?;
                Ok(Rc::clone(frame))
            })
            .transpose()
    }

    fn valid_frame<'request>(
        endpoint: RootEndpoint<'a>,
        frame: &Rc<SourceFrame<'a>>,
        request: ContextRequest<'request>,
    ) -> Result<(), ContextError<'a, 'request>> {
        if frame.provenance != FrameProvenance::Basic {
            return Err(ContextError {
                request,
                fault: ContextFault::UnsupportedFrame {
                    frame: Rc::clone(frame),
                },
            });
        }
        if frame.endpoint != endpoint {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidFrame {
                    frame: Rc::clone(frame),
                },
            });
        }
        if !context_schema(endpoint.declaration, &frame.collection, request)
            .is_ok_and(|schema| frame.collection.is_empty() || schema.repeating)
        {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidFrame {
                    frame: Rc::clone(frame),
                },
            });
        }
        let (mut value, consumed) = if let Some(parent) = &frame.parent {
            Self::valid_frame(endpoint, parent, request)?;
            if !frame.collection.starts_with(&parent.collection)
                || frame.collection == parent.collection
            {
                return Err(ContextError {
                    request,
                    fault: ContextFault::InvalidFrame {
                        frame: Rc::clone(frame),
                    },
                });
            }
            (parent.item, parent.collection.len())
        } else {
            (
                endpoint.root.ok_or(ContextError {
                    request,
                    fault: ContextFault::MissingEndpoint {
                        declaration: endpoint.declaration,
                    },
                })?,
                0,
            )
        };
        for segment in &frame.collection[consumed..] {
            let Instance::Group(fields) = value else {
                return Err(ContextError {
                    request,
                    fault: ContextFault::InvalidFrame {
                        frame: Rc::clone(frame),
                    },
                });
            };
            let Some((_, child)) = fields.iter().find(|(name, _)| name == segment) else {
                return Err(ContextError {
                    request,
                    fault: ContextFault::InvalidFrame {
                        frame: Rc::clone(frame),
                    },
                });
            };
            value = child;
        }
        let matches = match value {
            Instance::Repeated(items) => {
                context_schema(endpoint.declaration, &frame.collection, request)
                    .is_ok_and(|schema| schema.repeating)
                    && items
                        .get(frame.structural.get() - 1)
                        .is_some_and(|item| std::ptr::eq(item, frame.item))
            }
            Instance::Group(_) | Instance::Scalar(_) => {
                frame.collection.is_empty()
                    && frame.parent.is_none()
                    && frame.structural.get() == 1
                    && std::ptr::eq(value, frame.item)
            }
            Instance::DocumentSet(_) | Instance::MappedSequence(_) => false,
        };
        if !matches {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidFrame {
                    frame: Rc::clone(frame),
                },
            });
        }
        Ok(())
    }
}

fn context_schema<'a, 'request>(
    declaration: DeclaredSource<'a>,
    path: &[String],
    request: ContextRequest<'request>,
) -> Result<&'a SchemaNode, ContextError<'a, 'request>> {
    let mut schema = declaration.schema();
    for (index, segment) in path.iter().enumerate() {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return Err(ContextError {
                request,
                fault: ContextFault::UnknownPath {
                    declaration,
                    prefix_len: index,
                },
            });
        };
        schema = children
            .iter()
            .find(|child| child.name == *segment)
            .ok_or(ContextError {
                request,
                fault: ContextFault::UnknownPath {
                    declaration,
                    prefix_len: index + 1,
                },
            })?;
    }
    Ok(schema)
}

fn reject_context_domain<'a, 'request>(
    declaration: DeclaredSource<'a>,
    value: &'a Instance,
    prefix_len: usize,
    request: ContextRequest<'request>,
) -> Result<(), ContextError<'a, 'request>> {
    let domain = match value {
        Instance::Scalar(_) | Instance::Group(_) => return Ok(()),
        Instance::Repeated(_) => {
            return Err(ContextError {
                request,
                fault: ContextFault::UniteratedCollection {
                    declaration,
                    prefix_len,
                },
            });
        }
        Instance::DocumentSet(_) => UnsupportedSourceDomain::DocumentSet,
        Instance::MappedSequence(_) => UnsupportedSourceDomain::MappedSequence,
    };
    Err(ContextError {
        request,
        fault: ContextFault::UnsupportedDomain {
            declaration,
            prefix_len,
            domain,
        },
    })
}

fn walk_source<'a, 'request>(
    endpoint: RootEndpoint<'a>,
    source: &'request OwnedSourceRef,
    value: &'a Instance,
    consumed: usize,
    parent: Option<Rc<SourceFrame<'a>>>,
    active: &[Rc<SourceFrame<'a>>],
    output: &mut Vec<SourceWalk<'a>>,
) -> Result<(), ContextError<'a, 'request>> {
    let request = ContextRequest::Walk(source);
    if let Instance::Repeated(items) = value {
        // Every repeated occurrence, including root [], must also repeat
        // in the exact owning endpoint declaration.
        let schema = context_schema(endpoint.declaration, &source.path[..consumed], request)?;
        if !schema.repeating {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidShape {
                    declaration: endpoint.declaration,
                    prefix_len: consumed,
                    value,
                },
            });
        }
        for (index, item) in items.iter().enumerate() {
            reject_context_domain(endpoint.declaration, item, consumed, request)?;
            let structural = NonZeroUsize::new(index + 1).expect("enumeration is one based");
            let frame = Rc::new(SourceFrame {
                endpoint,
                collection: source.path[..consumed].to_vec(),
                item,
                parent: parent.as_ref().map(Rc::clone),
                structural,
                selection: structural,
                output: None,
                provenance: FrameProvenance::Basic,
            });
            let mut frames = active.to_vec();
            frames.push(Rc::clone(&frame));
            if consumed == source.path.len() {
                let introduced = Some(frames.len() - 1);
                output.push(SourceWalk {
                    endpoint,
                    frames,
                    item,
                    introduced,
                });
            } else {
                walk_source(
                    endpoint,
                    source,
                    item,
                    consumed,
                    Some(frame),
                    &frames,
                    output,
                )?;
            }
        }
        return Ok(());
    }
    reject_context_domain(endpoint.declaration, value, consumed, request)?;
    if consumed == source.path.len() {
        if !source.path.is_empty()
            && !parent
                .as_ref()
                .is_some_and(|frame| frame.collection == source.path)
        {
            return Err(ContextError {
                request,
                fault: ContextFault::InvalidShape {
                    declaration: endpoint.declaration,
                    prefix_len: consumed,
                    value,
                },
            });
        }
        // Root identity iterations retain their existing frame rather than
        // pushing an untracked duplicate. Explicit root collection [] gets a
        // position-one identity only when no occurrence is active yet.
        let mut frames = active.to_vec();
        let mut introduced = None;
        if parent.is_none() && source.path.is_empty() {
            let one = NonZeroUsize::new(1).expect("one is nonzero");
            introduced = Some(frames.len());
            frames.push(Rc::new(SourceFrame {
                endpoint,
                collection: vec![],
                item: value,
                parent: None,
                structural: one,
                selection: one,
                output: None,
                provenance: FrameProvenance::Basic,
            }));
        }
        output.push(SourceWalk {
            endpoint,
            frames,
            item: value,
            introduced,
        });
        return Ok(());
    }
    let Instance::Group(fields) = value else {
        return Err(ContextError {
            request,
            fault: ContextFault::InvalidShape {
                declaration: endpoint.declaration,
                prefix_len: consumed,
                value,
            },
        });
    };
    if let Some((_, child)) = fields
        .iter()
        .find(|(name, _)| name == &source.path[consumed])
    {
        walk_source(
            endpoint,
            source,
            child,
            consumed + 1,
            parent,
            active,
            output,
        )?;
    }
    Ok(())
}
