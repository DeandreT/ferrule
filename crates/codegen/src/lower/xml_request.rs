//! The Project's XML document adapter request, classified once before graph
//! lowering. Program validation later proves the lowered boundary's mode.

use ir::SchemaKind;
use mapping::{FormatOptions, Project, ScopeConstruction, ScopeIteration};

use crate::{
    Diagnostic, LowerError, NamedXmlInputPolicy, NamedXmlOutputPolicy, XmlBoundaryProgram,
    XmlInputPolicy, XmlOutputPolicy,
};

/// Which XML document adapter, if any, a Project's format options request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XmlRequest {
    /// No adapter. Literal schema hints stay with the typed outputs; no
    /// generated XML document writer exists to emit them.
    None,
    /// An ordinary structured primary input whose named boundaries keep the
    /// adapter from forming. Literal hints remain admitted.
    OrdinaryInput,
    /// Ordinary structured format identity on every boundary. The adapter is
    /// optional: lowering drops it if the Program cannot prove it.
    Ordinary,
    RequiredScalarOutput,
    /// An observed root-view primary input on one admitted route.
    RootView(RootViewRoute),
}

/// The admitted named-boundary shape beside an observed root-view primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RootViewRoute {
    /// One primary input and output.
    Primary,
    /// Static named Structured inputs and one flat primary output.
    StaticInputs,
    /// Flat static named outputs and no named inputs.
    StaticOutputs,
    /// Static named Structured inputs beside flat static named outputs.
    StaticCombined,
}

impl XmlRequest {
    /// Classifies the request, refusing incompatible reader and output
    /// policies with their diagnostics in declaration order. Literal hints
    /// accompany every route; each generated XML writer emits its own.
    pub(super) fn classify(project: &Project) -> Result<Self, LowerError> {
        if project.uses_xml_scalar_output() {
            project.validate_xml_scalar_output().map_err(|error| {
                LowerError::new(vec![Diagnostic::Validation {
                    location: "XML scalar output".into(),
                    message: error.to_string(),
                }])
            })?;
            if project.source_options != structured_input_options()
                || !ir::xml_structured_document_input_is_supported(&project.source)
            {
                return Err(LowerError::new(vec![Diagnostic::Validation { location: "XML scalar output".into(), message: "explicit XML scalar output requires an ordinary structured XML source adapter".into() }]));
            }
            return Ok(Self::RequiredScalarOutput);
        }
        let primary_xml = project.source_options.xml_root_view_read_policy;
        // Ordinary XML format identity does not make adapter support mandatory.
        // Existing core-only generation survives an unproved ordinary schema.
        let ordinary_xml_input = !project.source_options.xml_allow_inactive_root_type_members
            && !primary_xml
            && project.source_options == structured_input_options()
            && ir::xml_structured_document_input_is_supported(&project.source);
        // Literal hints belong to the optional XML adapter. An unsuitable named
        // output removes that whole adapter without obstructing typed/JSON core.
        let ordinary_xml = ordinary_xml_input
            && project.extra_sources.len() < 4096
            && project.extra_sources.iter().all(|source| {
                source.options == structured_input_options()
                    && ir::xml_structured_document_input_is_supported(&source.schema)
            })
            && xml_document_output_options(&project.target_options)
            && project
                .extra_targets
                .iter()
                .all(|target| xml_document_output_options(&target.options));
        let mut reader_diagnostics: Vec<_> = project
            .extra_sources
            .iter()
            .filter(|source| source.options.xml_root_view_read_policy)
            .map(|source| Diagnostic::Validation {
                location: format!("extra source `{}` format options", source.name),
                message:
                    "code generation does not support named observed XML root-view input adapters"
                        .into(),
            })
            .collect();
        for (location, options) in
            std::iter::once(("source format options".to_owned(), &project.source_options)).chain(
                project.extra_sources.iter().map(|source| {
                    (
                        format!("extra source `{}` format options", source.name),
                        &source.options,
                    )
                }),
            )
        {
            if options.xml_allow_inactive_root_type_members != options.xml_root_view_read_policy {
                reader_diagnostics.push(Diagnostic::Validation {
                    location,
                    message: "generated XML input policy has incompatible observed profile flags"
                        .into(),
                });
            }
        }
        let route = if primary_xml {
            root_view_route(project)
        } else {
            None
        };
        if primary_xml
            && (!project.extra_sources.is_empty() || !project.extra_targets.is_empty())
            && route.is_none()
        {
            reader_diagnostics.push(Diagnostic::Validation {
                location: "source format options".into(),
                message: "generated XML document adapters require one primary input and output; named and dynamic boundaries are unsupported".into(),
            });
        }
        if primary_xml {
            let supported = FormatOptions {
                xml_document: project.target_options.xml_document,
                xml_schema_hints: project.target_options.xml_schema_hints.clone(),
                ..Default::default()
            };
            if project.target_options != supported {
                reader_diagnostics.push(Diagnostic::Validation {
                    location: "target format options".into(),
                    message:
                        "generated XML document output supports only primary XML format options"
                            .into(),
                });
            }
        }
        if !reader_diagnostics.is_empty() {
            return Err(LowerError::new(reader_diagnostics));
        }

        let request = if primary_xml {
            // Diagnostics above refuse every named boundary off the routes.
            Self::RootView(route.unwrap_or(RootViewRoute::Primary))
        } else if ordinary_xml {
            Self::Ordinary
        } else if ordinary_xml_input {
            Self::OrdinaryInput
        } else {
            Self::None
        };
        Ok(request)
    }

    /// Whether lowering drops the adapter when the Program cannot prove it.
    pub(super) fn optional(self) -> bool {
        matches!(self, Self::Ordinary)
    }

    /// The exact primary-then-declaration-order policies of the requested adapter.
    pub(super) fn boundary(self, project: &Project) -> Option<XmlBoundaryProgram> {
        if !matches!(
            self,
            Self::Ordinary | Self::RequiredScalarOutput | Self::RootView(_)
        ) {
            return None;
        }
        Some(XmlBoundaryProgram {
            input: XmlInputPolicy {
                allow_inactive_root_type_members: project
                    .source_options
                    .xml_allow_inactive_root_type_members,
                root_view_policy: matches!(self, Self::RootView(_)),
            },
            extra_inputs: project
                .extra_sources
                .iter()
                .map(|source| NamedXmlInputPolicy {
                    name: source.name.clone(),
                    input: XmlInputPolicy {
                        allow_inactive_root_type_members: source
                            .options
                            .xml_allow_inactive_root_type_members,
                        root_view_policy: source.options.xml_root_view_read_policy,
                    },
                })
                .collect(),
            output: XmlOutputPolicy {
                scalar_output: project.target_options.xml_scalar_output.clone(),
                // Generated writers emit document metadata only, never provenance.
                schema_hints: project
                    .target_options
                    .xml_schema_hints
                    .as_ref()
                    .map(ir::XmlSchemaHints::without_derivation),
                ..Default::default()
            },
            extra_outputs: project
                .extra_targets
                .iter()
                .map(|target| NamedXmlOutputPolicy {
                    name: target.name.clone(),
                    output: XmlOutputPolicy {
                        schema_hints: target
                            .options
                            .xml_schema_hints
                            .as_ref()
                            .map(ir::XmlSchemaHints::without_derivation),
                        ..Default::default()
                    },
                })
                .collect(),
        })
    }
}

/// The admitted route of an observed root-view primary, or none when its named
/// boundaries match no route. Each route carries its own complete proof.
fn root_view_route(project: &Project) -> Option<RootViewRoute> {
    if project.extra_sources.is_empty() && project.extra_targets.is_empty() {
        return Some(RootViewRoute::Primary);
    }
    // Preserve the historical static-input and static-output routes.
    // The combined route below has its own complete admission proof.
    let static_inputs = !project.extra_sources.is_empty()
        && project.extra_targets.is_empty()
        && matches!(project.root.iteration, ScopeIteration::None)
        && matches!(project.root.construction, ScopeConstruction::Constructed)
        && !project.target.repeating
        && project.extra_sources.iter().all(|source| {
            source.dynamic_path.is_none()
                && source.options == structured_input_options()
                && ir::xml_structured_document_input_is_supported(&source.schema)
        });
    if static_inputs {
        return Some(RootViewRoute::StaticInputs);
    }
    // The newly admitted plural route remains flat even when its expressions
    // contain no primary-root reader. Preserve the existing one-output route.
    let plural_roots_flat = project.extra_targets.len() == 1
        || std::iter::once((&project.target, &project.root))
            .chain(
                project
                    .extra_targets
                    .iter()
                    .map(|target| (&target.schema, &target.root)),
            )
            .all(|(schema, scope)| {
                (scope.target_field.is_empty() || scope.target_field == schema.name)
                    && scope.dynamic_bindings.is_empty()
                    && scope.filter.is_none()
                    && scope.post_group_filter.is_none()
                    && !scope.has_grouping()
                    && !scope.has_sort()
                    && scope.windows.is_empty()
                    && !scope.merge_dynamic_fields
                    && scope.children.is_empty()
                    && scope.dynamic_children.is_empty()
                    && scope.concatenated().is_none()
            });
    let static_outputs = project.extra_sources.is_empty()
        && !project.extra_targets.is_empty()
        && plural_roots_flat
        && matches!(project.root.iteration, ScopeIteration::None)
        && matches!(project.root.construction, ScopeConstruction::Constructed)
        && !project.target.repeating
        && matches!(project.target.kind, SchemaKind::Group { .. })
        && xml_document_output_options(&project.target_options)
        && project.extra_targets.iter().all(|target| {
            matches!(target.root.iteration, ScopeIteration::None)
                && matches!(target.root.construction, ScopeConstruction::Constructed)
                && !target.schema.repeating
                && matches!(target.schema.kind, SchemaKind::Group { .. })
                && xml_document_output_options(&target.options)
        });
    if static_outputs {
        return Some(RootViewRoute::StaticOutputs);
    }
    // Unlike the historical input-only/one-output routes, every combined root
    // must be flat even with one named output and no primary-root expressions.
    let static_combined = project.source_options
        == (FormatOptions {
            xml_document: true,
            xml_allow_inactive_root_type_members: true,
            xml_root_view_read_policy: true,
            ..Default::default()
        })
        && !project.extra_sources.is_empty()
        && !project.extra_targets.is_empty()
        && project.extra_sources.iter().all(|source| {
            source.dynamic_path.is_none()
                && source.options == structured_input_options()
                && ir::xml_structured_document_input_is_supported(&source.schema)
        })
        && std::iter::once((&project.target, &project.root, &project.target_options))
            .chain(
                project
                    .extra_targets
                    .iter()
                    .map(|target| (&target.schema, &target.root, &target.options)),
            )
            .all(|(schema, scope, options)| {
                !schema.repeating
                    && matches!(schema.kind, SchemaKind::Group { .. })
                    && (scope.target_field.is_empty() || scope.target_field == schema.name)
                    && matches!(scope.iteration, ScopeIteration::None)
                    && matches!(scope.construction, ScopeConstruction::Constructed)
                    && scope.dynamic_bindings.is_empty()
                    && scope.filter.is_none()
                    && scope.post_group_filter.is_none()
                    && !scope.has_grouping()
                    && !scope.has_sort()
                    && scope.windows.is_empty()
                    && !scope.merge_dynamic_fields
                    && scope.children.is_empty()
                    && scope.dynamic_children.is_empty()
                    && scope.concatenated().is_none()
                    && xml_document_output_options(options)
            });
    static_combined.then_some(RootViewRoute::StaticCombined)
}

fn structured_input_options() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}

fn xml_document_output_options(options: &FormatOptions) -> bool {
    *options
        == FormatOptions {
            xml_document: true,
            xml_schema_hints: options.xml_schema_hints.clone(),
            ..Default::default()
        }
}
