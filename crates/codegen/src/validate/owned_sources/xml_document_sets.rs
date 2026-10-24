//! Admission of the owned XML document-set entry points. Each endpoint
//! arrives as an ordered list of members parsed through the structured XML
//! profile, so the Program needs that profile and static named sources. The
//! XML boundary validator already proves every named input Structured; the
//! parsed sets are then walked by the `document_sets` capability.
use super::*;
use crate::XmlInputProfile;
use crate::validate::XmlDocumentSetInputRefusal as Refusal;

impl OwnedCapabilities {
    /// Refuses a Program whose endpoints cannot all enter as XML document
    /// sets. Without the capability no entry point exists and nothing is checked.
    pub(super) fn check_xml_document_set_inputs(
        self,
        program: &Program,
    ) -> Result<(), ProgramValidationError> {
        if !self.xml_document_sets {
            return Ok(());
        }
        let refuse = |refusal| Err(ProgramValidationError::XmlDocumentSetInput { refusal });
        let Some(boundary) = &program.xml_boundary else {
            return refuse(Refusal::MissingXmlBoundary);
        };
        if boundary.input.profile() != Some(XmlInputProfile::Structured) {
            return refuse(Refusal::ObservedPrimaryInput);
        }
        if let Some((index, source)) = program
            .extra_sources
            .iter()
            .enumerate()
            .find(|(_, source)| source.dynamic.is_some())
        {
            return refuse(Refusal::DynamicSource {
                index,
                name: source.name.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Program {
        let project: mapping::Project = serde_json::from_str(include_str!(
            "../../tests/fixtures/static_named_inputs_dynamic_primary_xml_documents.json"
        ))
        .unwrap();
        crate::lower(&project).unwrap()
    }

    fn check(program: &Program) -> Result<(), ProgramValidationError> {
        OwnedPreset::XmlDocumentSets
            .capabilities()
            .check_xml_document_set_inputs(program)
    }

    #[test]
    fn only_structured_static_xml_boundaries_admit_document_set_entries() {
        let program = program();
        check(&program).unwrap();
        OwnedPreset::DocumentPaths
            .capabilities()
            .check_xml_document_set_inputs(&Program {
                xml_boundary: None,
                ..program.clone()
            })
            .unwrap();
        let refusal = |program: &Program| match check(program) {
            Err(ProgramValidationError::XmlDocumentSetInput { refusal }) => refusal,
            other => panic!("{other:?}"),
        };
        let mut missing = program.clone();
        missing.xml_boundary = None;
        assert_eq!(refusal(&missing), Refusal::MissingXmlBoundary);
        let mut observed = program;
        let policy = &mut observed.xml_boundary.as_mut().unwrap().input;
        policy.allow_inactive_root_type_members = true;
        policy.root_view_policy = true;
        assert_eq!(refusal(&observed), Refusal::ObservedPrimaryInput);
    }
}
