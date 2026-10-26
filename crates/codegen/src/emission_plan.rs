//! Boundary emission prepared once per emission request.
//!
//! Complete Program validation proves the XML document adapter's
//! classification. [`prepare_boundary_emission`] keeps that proof beside the
//! explicitly requested companion selection, so both language emitters render
//! from the same immutable facts instead of re-deriving them.

use crate::{NamedTargetProgram, Program, ProgramValidationError, XmlBoundaryProgram};

/// Output cardinality proved by the complete XML boundary and mapping validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlOutputMode {
    SingleDocument,
    DynamicPrimaryDocuments,
    /// Dynamic named XML inputs and optional statics feed a primary document list.
    DynamicNamedInputDynamicPrimaryDocuments,
    /// Static named Structured inputs and one ordered primary document list.
    StaticNamedInputsDynamicPrimaryDocuments,
    /// One static primary document followed by each named target's ordered members.
    StaticPrimaryDynamicNamedDocuments,
    /// Static named Structured inputs, a static primary and ordered named document lists.
    StaticNamedInputsStaticPrimaryDynamicNamedDocuments,
    /// Dynamic named Structured inputs, optional statics and ordered named document lists.
    DynamicNamedInputStaticPrimaryDynamicNamedDocuments,
    /// One static named XML document and one or more path-bearing named document lists.
    StaticPrimaryMixedNamedXmlOutputs,
}

/// API families an emitter renders beside the typed instance entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompanionSelection {
    /// The existing inventory: schema-shaped JSON companions and, when the
    /// Program declares one, its XML document adapter.
    #[default]
    Complete,
    /// Typed instance entry points only. No JSON or XML companion API is
    /// rendered and no companion schema descriptor is admitted. The Program,
    /// including any XML boundary policy it carries, is still validated.
    TypedOnly,
}

/// How the XML adapter accepts its primary and named input documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlDocumentInputs {
    /// One observed root-view primary document and no named inputs.
    RootViewPrimary,
    /// An observed root-view primary beside static named Structured inputs.
    RootViewWithNamedInputs,
    /// Structured primary and named inputs, including per-driver dynamic ones.
    Structured,
}

/// Whether one XML output endpoint publishes a single document or an ordered
/// list of path-bearing documents charged against its own document-set budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlDocumentCardinality {
    Single,
    List,
}

/// The proved XML adapter of one Program and the policies it renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlEmissionPlan<'a> {
    mode: XmlOutputMode,
    inputs: XmlDocumentInputs,
    named_outputs: Vec<XmlDocumentCardinality>,
    policy: &'a XmlBoundaryProgram,
    selected_inputs: Option<crate::NamedInputAdmissionPlan<'a>>,
}

impl<'a> XmlEmissionPlan<'a> {
    /// Static source requirements for isolated physical byte output. This route
    /// is only proved for ordinary Structured inputs and single-document outputs.
    pub fn selected_inputs(&self) -> Option<&crate::NamedInputAdmissionPlan<'a>> {
        self.selected_inputs.as_ref()
    }

    pub const fn mode(&self) -> XmlOutputMode {
        self.mode
    }

    pub const fn inputs(&self) -> XmlDocumentInputs {
        self.inputs
    }

    /// The validated primary and named policies, in exact declaration order.
    pub const fn policy(&self) -> &'a XmlBoundaryProgram {
        self.policy
    }

    /// Document cardinality of the primary output.
    pub const fn primary_output(&self) -> XmlDocumentCardinality {
        match self.mode {
            XmlOutputMode::DynamicPrimaryDocuments
            | XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments
            | XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments => {
                XmlDocumentCardinality::List
            }
            XmlOutputMode::SingleDocument
            | XmlOutputMode::StaticPrimaryDynamicNamedDocuments
            | XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments
            | XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments
            | XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs => XmlDocumentCardinality::Single,
        }
    }

    /// Document cardinality of each named output, in declaration order.
    pub fn named_outputs(&self) -> &[XmlDocumentCardinality] {
        &self.named_outputs
    }

    /// Pairs each named target with its proved document cardinality.
    pub fn named_targets<'p>(
        &'p self,
        program: &'p Program,
    ) -> impl Iterator<Item = (&'p NamedTargetProgram, XmlDocumentCardinality)> + 'p {
        program
            .extra_targets
            .iter()
            .zip(self.named_outputs.iter().copied())
    }
}

/// Immutable boundary facts for one emission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryEmissionPlan<'a> {
    companions: CompanionSelection,
    xml: Option<XmlEmissionPlan<'a>>,
}

impl<'a> BoundaryEmissionPlan<'a> {
    pub const fn companions(&self) -> CompanionSelection {
        self.companions
    }

    /// Whether schema-shaped JSON companions are rendered and their
    /// descriptors admitted.
    pub const fn json_companions(&self) -> bool {
        matches!(self.companions, CompanionSelection::Complete)
    }

    /// The XML adapter to render; absent without a boundary or when the
    /// selection omits companions.
    pub const fn xml(&self) -> Option<&XmlEmissionPlan<'a>> {
        self.xml.as_ref()
    }
}

/// Validates `program` once and prepares its boundary emission.
///
/// Refusals are exactly those of [`crate::validate_program`], in the same
/// order, for every selection.
pub fn prepare_boundary_emission(
    program: &Program,
    companions: CompanionSelection,
) -> Result<BoundaryEmissionPlan<'_>, ProgramValidationError> {
    let proved = crate::validate::validate_program_proving_xml_boundary(program)?;
    let xml = match (companions, proved, &program.xml_boundary) {
        (CompanionSelection::Complete, Some(proved), Some(policy)) => Some(XmlEmissionPlan {
            selected_inputs: (proved.mode == XmlOutputMode::SingleDocument
                && proved.inputs == XmlDocumentInputs::Structured
                && program
                    .extra_sources
                    .iter()
                    .all(|source| source.dynamic.is_none()))
            .then(|| crate::owned_admission::admission_plan(program)),
            mode: proved.mode,
            inputs: proved.inputs,
            named_outputs: proved.named_outputs,
            policy,
        }),
        _ => None,
    };
    Ok(BoundaryEmissionPlan { companions, xml })
}

impl Program {
    /// Returns a proved XML adapter mode, or no adapter, without changing its policy ABI.
    /// An invalid program returns its original typed validation error.
    ///
    /// Emitters read the mode from [`prepare_boundary_emission`] instead; this
    /// query validates the complete Program on every call.
    pub fn xml_output_mode(&self) -> Result<Option<XmlOutputMode>, ProgramValidationError> {
        Ok(crate::validate::validate_program_proving_xml_boundary(self)?.map(|proved| proved.mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<(String, Program)> {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures");
        let mut paths = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .filter_map(|path| {
                let project: mapping::Project =
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                let program = crate::lower(&project).ok()?;
                Some((path.display().to_string(), program))
            })
            .collect()
    }

    #[test]
    fn complete_plan_keeps_the_validated_xml_classification() {
        let programs = fixtures();
        assert!(
            programs
                .iter()
                .filter(|(_, program)| program.xml_boundary.is_some())
                .count()
                >= 20
        );
        for (name, program) in &programs {
            let plan = prepare_boundary_emission(program, CompanionSelection::Complete).unwrap();
            assert!(plan.json_companions(), "{name}");
            let mode = program.xml_output_mode().unwrap();
            assert_eq!(plan.xml().map(XmlEmissionPlan::mode), mode, "{name}");
            let Some(xml) = plan.xml() else {
                continue;
            };
            assert_eq!(
                xml.selected_inputs().is_some(),
                xml.mode() == XmlOutputMode::SingleDocument
                    && xml.inputs() == XmlDocumentInputs::Structured
                    && program
                        .extra_sources
                        .iter()
                        .all(|source| source.dynamic.is_none()),
                "{name}"
            );
            assert!(std::ptr::eq(
                xml.policy(),
                program.xml_boundary.as_ref().unwrap()
            ));
            assert_eq!(
                xml.named_outputs().len(),
                program.extra_targets.len(),
                "{name}"
            );
            for (target, documents) in xml.named_targets(program) {
                let dynamic = target
                    .root
                    .iteration
                    .as_ref()
                    .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some());
                assert_eq!(documents == XmlDocumentCardinality::List, dynamic, "{name}");
            }
            let dynamic_primary = program
                .root
                .iteration
                .as_ref()
                .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some());
            assert_eq!(
                xml.primary_output() == XmlDocumentCardinality::List,
                dynamic_primary,
                "{name}"
            );
            let expected_inputs = match xml.policy().input.profile() {
                Some(crate::XmlInputProfile::Structured) => XmlDocumentInputs::Structured,
                _ if program.extra_sources.is_empty() => XmlDocumentInputs::RootViewPrimary,
                _ => XmlDocumentInputs::RootViewWithNamedInputs,
            };
            assert_eq!(xml.inputs(), expected_inputs, "{name}");
        }
    }

    #[test]
    fn typed_only_plan_selects_no_companions_and_keeps_every_refusal() {
        for (name, mut program) in fixtures() {
            let plan = prepare_boundary_emission(&program, CompanionSelection::TypedOnly).unwrap();
            assert_eq!(plan.companions(), CompanionSelection::TypedOnly);
            assert!(!plan.json_companions() && plan.xml().is_none(), "{name}");
            let Some(policy) = program.xml_boundary.as_mut() else {
                continue;
            };
            policy.input.allow_inactive_root_type_members = !policy.input.root_view_policy;
            let expected = crate::validate_program(&program).unwrap_err();
            for companions in [CompanionSelection::Complete, CompanionSelection::TypedOnly] {
                assert_eq!(
                    prepare_boundary_emission(&program, companions),
                    Err(expected.clone()),
                    "{name}"
                );
            }
            assert_eq!(program.xml_output_mode(), Err(expected), "{name}");
        }
    }
}
