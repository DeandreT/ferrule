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
