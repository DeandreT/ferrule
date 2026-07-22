use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mapping::{Project, XbrlBoundaryMode};

mod graph;
mod join;
mod options;
mod schema;
mod scope;
mod user_function;

use graph::{validate_cycles, validate_graph};
use options::{
    validate_external_source_options, validate_structured_edi_source_options,
    validate_target_options, validate_wsdl_options, validate_xbrl_options, validate_xlsx_options,
};
use schema::{display_path, source_path_matches, validate_schema};
use scope::{ScopeSchemas, validate_scope};
use user_function::validate_user_functions;

/// One actionable problem found before a mapping is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub location: String,
    pub message: String,
}

impl ValidationIssue {
    pub(super) fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Checks graph integrity, source/target paths, scope references, builtin
/// names, and cycles without reading input data or evaluating expressions.
pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if project.root.output_path().is_some() && project.target_path.is_some() {
        issues.push(ValidationIssue::new(
            "target path",
            "a dynamic target path cannot be combined with a stored target path",
        ));
    }
    validate_xbrl_options(
        "source format options",
        &project.source_options,
        XbrlBoundaryMode::ExternalSource,
        &mut issues,
    );
    validate_external_source_options(
        "source format options",
        &project.source_options,
        true,
        &mut issues,
    );
    validate_structured_edi_source_options(
        "source format options",
        &project.source_options,
        &mut issues,
    );
    validate_xlsx_options(
        "source format options",
        &project.source_options,
        &project.source,
        true,
        &mut issues,
    );
    validate_wsdl_options(
        "source format options",
        &project.source_options,
        true,
        &mut issues,
    );
    validate_target_options(
        "target format options",
        &project.target_options,
        &mut issues,
    );
    validate_xlsx_options(
        "target format options",
        &project.target_options,
        &project.target,
        false,
        &mut issues,
    );
    validate_wsdl_options(
        "target format options",
        &project.target_options,
        false,
        &mut issues,
    );
    if let Some(layout) = &project.source_options.pdf
        && layout.schema() != project.source
    {
        issues.push(ValidationIssue::new(
            "source format options",
            "PDF extraction layout does not match the source schema",
        ));
    }
    validate_schema(
        "source schema",
        &project.source,
        &mut Vec::new(),
        &mut issues,
    );
    validate_schema(
        "target schema",
        &project.target,
        &mut Vec::new(),
        &mut issues,
    );
    let mut target_names = BTreeSet::new();
    for target in &project.extra_targets {
        let name = target.name.trim();
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                "extra target",
                "extra target name cannot be empty",
            ));
        } else if !target_names.insert(name) {
            issues.push(ValidationIssue::new(
                format!("extra target `{name}`"),
                "extra target name is duplicated",
            ));
        }
        validate_target_options(
            &format!("extra target `{name}` format options"),
            &target.options,
            &mut issues,
        );
        validate_xlsx_options(
            &format!("extra target `{name}` format options"),
            &target.options,
            &target.schema,
            false,
            &mut issues,
        );
        validate_wsdl_options(
            &format!("extra target `{name}` format options"),
            &target.options,
            false,
            &mut issues,
        );
        validate_schema(
            &format!("extra target `{name}` schema"),
            &target.schema,
            &mut Vec::new(),
            &mut issues,
        );
        if target.root.output_path().is_some() && target.path.is_some() {
            issues.push(ValidationIssue::new(
                format!("extra target `{name}` path"),
                "a dynamic target path cannot be combined with a stored target path",
            ));
        }
    }
    let mut source_names = BTreeSet::new();
    for source in &project.extra_sources {
        let name = source.name.trim();
        let location = format!("extra source `{name}`");
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                "extra source",
                "extra source name cannot be empty",
            ));
        } else if !source_names.insert(name) {
            issues.push(ValidationIssue::new(
                &location,
                "extra source name is duplicated",
            ));
        }
        validate_xbrl_options(
            &format!("{location} format options"),
            &source.options,
            XbrlBoundaryMode::ExternalSource,
            &mut issues,
        );
        validate_external_source_options(
            &format!("{location} format options"),
            &source.options,
            true,
            &mut issues,
        );
        validate_structured_edi_source_options(
            &format!("{location} format options"),
            &source.options,
            &mut issues,
        );
        validate_xlsx_options(
            &format!("{location} format options"),
            &source.options,
            &source.schema,
            true,
            &mut issues,
        );
        validate_wsdl_options(
            &format!("{location} format options"),
            &source.options,
            true,
            &mut issues,
        );
        if let Some(layout) = &source.options.pdf
            && layout.schema() != source.schema
        {
            issues.push(ValidationIssue::new(
                format!("{location} format options"),
                "PDF extraction layout does not match the extra-source schema",
            ));
        }
        validate_schema(
            &format!("{location} schema"),
            &source.schema,
            &mut Vec::new(),
            &mut issues,
        );
        if let Some(dynamic) = &source.dynamic_path {
            if !project.graph.nodes.contains_key(&dynamic.node) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "dynamic path expression references missing node {}",
                        dynamic.node
                    ),
                ));
            }
            if !source_path_matches(project, &dynamic.iteration, |_| true) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "dynamic path iteration `{}` matches no source path",
                        display_path(&dynamic.iteration)
                    ),
                ));
            }
        }
    }
    validate_user_functions(project, &mut issues);
    validate_graph(project, &mut issues);
    validate_cycles(&project.graph, &mut issues);
    validate_scope(
        project,
        &project.root,
        ScopeSchemas {
            target: Some(&project.target),
            parent_source: Some(&project.source),
        },
        &mut Vec::new(),
        &[],
        &mut BTreeMap::new(),
        &mut issues,
    );
    for target in &project.extra_targets {
        validate_scope(
            project,
            &target.root,
            ScopeSchemas {
                target: Some(&target.schema),
                parent_source: Some(&project.source),
            },
            &mut Vec::new(),
            &[],
            &mut BTreeMap::new(),
            &mut issues,
        );
    }
    issues
}

#[cfg(test)]
mod tests;
