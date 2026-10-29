use crate::{FormatOptions, Project, Scope};

pub const XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY: &str = "xml-target-scalar-output-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlScalarOutputAdmissionError {
    pub owner: String,
    pub reason: String,
}

impl std::fmt::Display for XmlScalarOutputAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.owner, self.reason)
    }
}
impl std::error::Error for XmlScalarOutputAdmissionError {}

impl Project {
    pub fn uses_xml_scalar_output(&self) -> bool {
        std::iter::once(&self.source_options)
            .chain(std::iter::once(&self.target_options))
            .chain(self.extra_sources.iter().map(|source| &source.options))
            .chain(self.extra_targets.iter().map(|target| &target.options))
            .any(|options| options.xml_scalar_output.is_some())
    }

    /// Targeted admission, including invalid and unresolved placements.
    pub fn validate_xml_scalar_output(
        &self,
    ) -> Result<Option<ir::XmlScalarPreservation<'_>>, XmlScalarOutputAdmissionError> {
        let fail = |owner: &str, reason: &str| XmlScalarOutputAdmissionError {
            owner: owner.into(),
            reason: reason.into(),
        };
        if self.source_options.xml_scalar_output.is_some() {
            return Err(fail("source", "XML scalar output is target-only"));
        }
        for source in &self.extra_sources {
            if source.options.xml_scalar_output.is_some() {
                return Err(fail(
                    &format!("source {}", source.name),
                    "XML scalar output is target-only",
                ));
            }
        }
        for target in &self.extra_targets {
            if target.options.xml_scalar_output.is_some() {
                return Err(fail(
                    &format!("target {}", target.name),
                    "XML scalar output v1 does not support named targets",
                ));
            }
        }
        let Some(policy) = &self.target_options.xml_scalar_output else {
            return Ok(None);
        };
        if self
            .graph
            .nodes
            .values()
            .any(|node| node.owned_source_kind().is_some())
            || self
                .failure_rules
                .iter()
                .any(|rule| rule.iteration.owned_source().is_some())
            || crate::first_project_owned_source_scope(self).is_some()
        {
            return Err(fail(
                "target",
                "XML scalar output v1 does not support owned execution routes",
            ));
        }
        if !self.extra_sources.is_empty() || !self.extra_targets.is_empty() {
            return Err(fail(
                "target",
                "XML scalar output v1 requires a single target and no named sources",
            ));
        }
        self.target_options
            .validate_xml_scalar_output_options()
            .map_err(|reason| fail("target", &reason))?;
        if matches!(
            policy,
            ir::XmlScalarOutputPolicy::SuppressCastInDocumentV1 {}
        ) {
            if !ordinary_document_scope(&self.root, 0) {
                return Err(fail(
                    "target",
                    "XML scalar output requires ordinary constructed document scopes",
                ));
            }
            return policy
                .validate_schema(&self.target)
                .map(Some)
                .map_err(|reason| fail("target", &reason.to_string()));
        }
        if !self.root.children.is_empty()
            || !self.root.dynamic_children.is_empty()
            || !matches!(self.root.iteration, crate::ScopeIteration::None)
            || !matches!(
                self.root.construction,
                crate::ScopeConstruction::Constructed
            )
        {
            return Err(fail(
                "target",
                "XML scalar output v1 requires ordinary flat construction",
            ));
        }
        let mut root = self.root.clone();
        root.bindings.clear();
        if root != Scope::default() {
            return Err(fail(
                "target",
                "XML scalar output v1 requires ordinary flat construction",
            ));
        }
        policy
            .validate_schema(&self.target)
            .map(Some)
            .map_err(|reason| fail("target", &reason.to_string()))
    }
}

/// Every scope constructs one group through the ordinary item evaluator, the
/// only path that applies the policy to each binding.
fn ordinary_document_scope(scope: &Scope, depth: usize) -> bool {
    depth <= ir::MAX_XML_SCALAR_OUTPUT_DOCUMENT_DEPTH
        && matches!(scope.construction, crate::ScopeConstruction::Constructed)
        && !scope.produces_documents()
        && scope.dynamic_children.is_empty()
        && scope.concatenated().is_none()
        && scope.owned_recursive_filter().is_none()
        && scope
            .children
            .iter()
            .all(|child| ordinary_document_scope(child, depth + 1))
}

impl FormatOptions {
    pub fn validate_xml_scalar_output_options(&self) -> Result<(), String> {
        if self.xml_scalar_output.is_none() {
            return Ok(());
        }
        let accepted = Self {
            xml_document: true,
            xml_scalar_output: self.xml_scalar_output.clone(),
            ..Default::default()
        };
        if *self != accepted {
            return Err(
                "XML scalar output v1 requires xml_document and no other format options".into(),
            );
        }
        Ok(())
    }
}
