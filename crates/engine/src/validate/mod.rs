use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use mapping::{Project, XbrlBoundaryMode};

mod graph;
mod join;
mod options;
mod owner;
mod schema;
mod scope;
mod user_function;

use graph::{validate_cycles, validate_graph};
use options::{
    validate_external_source_options, validate_json5_options, validate_structured_edi_options,
    validate_target_options, validate_wsdl_options, validate_xbrl_options, validate_xlsx_options,
};
use schema::{display_path, source_path_matches, validate_schema};
use scope::{ScopeSchemas, validate_scope};
use user_function::validate_user_functions;

pub use owner::{
    ValidationEndpoint, ValidationOwner, ValidationSchemaLocation, ValidationSchemaStep,
    ValidationScopeLocation, ValidationScopeStep,
};

/// One actionable problem found before a mapping is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub location: String,
    pub message: String,
    /// Optional machine-readable owner; display text remains unchanged.
    pub owner: Option<ValidationOwner>,
}

impl ValidationIssue {
    pub(super) fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
            owner: None,
        }
    }

    pub(super) fn with_owner(mut self, owner: ValidationOwner) -> Self {
        self.owner = Some(owner);
        self
    }
}

/// Apply an enclosing owner without replacing a more specific nested owner.
pub(super) fn own_issues(issues: &mut [ValidationIssue], owner: ValidationOwner) {
    for issue in issues {
        if issue.owner.is_none() {
            issue.owner = Some(owner.clone());
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

pub(super) fn validate_builtin_call(
    location: &str,
    function: &str,
    argument_count: usize,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(definition) = functions::builtin(function) else {
        issues.push(ValidationIssue::new(
            location,
            format!("unknown function `{function}`"),
        ));
        return;
    };
    if definition.accepts_arity(argument_count) {
        return;
    }

    let arity = definition.arity;
    let expected = match (arity.maximum(), arity.step()) {
        (Some(maximum), _) if arity.minimum() == maximum => {
            format!("exactly {} argument(s)", arity.minimum())
        }
        (Some(maximum), _) => format!("{} to {maximum} argument(s)", arity.minimum()),
        (None, Some(1)) => format!("at least {} argument(s)", arity.minimum()),
        (None, Some(step)) => format!(
            "{} argument(s), then complete groups of {step}",
            arity.minimum()
        ),
        (None, None) => format!("at least {} argument(s)", arity.minimum()),
    };
    issues.push(ValidationIssue::new(
        location,
        format!("function `{function}` expects {expected}, got {argument_count}"),
    ));
}

pub(super) fn validate_runtime_parameter_name(
    location: &str,
    name: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if name.is_empty() {
        issues.push(ValidationIssue::new(
            location,
            "runtime parameter name cannot be empty",
        ));
    } else if name.contains('\0') {
        issues.push(ValidationIssue::new(
            location,
            "runtime parameter name cannot contain NUL",
        ));
    } else if name.len() > mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "runtime parameter name exceeds {} UTF-8 bytes",
                mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES
            ),
        ));
    }
}

/// Checks graph integrity, source/target paths, scope references, builtin
/// names and arities, and cycles without reading input data or evaluating
/// expressions.
pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if project.root.output_path().is_some() && project.target_path.is_some() {
        issues.push(
            ValidationIssue::new(
                "target path",
                "a dynamic target path cannot be combined with a stored target path",
            )
            .with_owner(ValidationOwner::Endpoint(ValidationEndpoint::Target)),
        );
    }
    let source_options_start = issues.len();
    validate_json5_options(
        "source format options",
        &project.source_options,
        &mut issues,
    );
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
    validate_structured_edi_options(
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
    own_issues(
        &mut issues[source_options_start..],
        ValidationOwner::Endpoint(ValidationEndpoint::Source),
    );
    let target_options_start = issues.len();
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
    own_issues(
        &mut issues[target_options_start..],
        ValidationOwner::Endpoint(ValidationEndpoint::Target),
    );
    if let Some(layout) = &project.source_options.pdf
        && layout.schema() != project.source
    {
        issues.push(
            ValidationIssue::new(
                "source format options",
                "PDF extraction layout does not match the source schema",
            )
            .with_owner(ValidationOwner::Endpoint(ValidationEndpoint::Source)),
        );
    }
    validate_schema(
        "source schema",
        &project.source,
        &ValidationSchemaLocation::root(ValidationEndpoint::Source),
        &mut Vec::new(),
        &mut issues,
    );
    validate_schema(
        "target schema",
        &project.target,
        &ValidationSchemaLocation::root(ValidationEndpoint::Target),
        &mut Vec::new(),
        &mut issues,
    );
    let mut target_names = BTreeSet::new();
    for (index, target) in project.extra_targets.iter().enumerate() {
        let start = issues.len();
        let endpoint = ValidationEndpoint::NamedTarget {
            index,
            name: target.name.clone(),
        };
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
            &ValidationSchemaLocation::root(endpoint.clone()),
            &mut Vec::new(),
            &mut issues,
        );
        if target.root.output_path().is_some() && target.path.is_some() {
            issues.push(ValidationIssue::new(
                format!("extra target `{name}` path"),
                "a dynamic target path cannot be combined with a stored target path",
            ));
        }
        own_issues(&mut issues[start..], ValidationOwner::Endpoint(endpoint));
    }
    let mut source_names = BTreeSet::new();
    for (index, source) in project.extra_sources.iter().enumerate() {
        let start = issues.len();
        let endpoint = ValidationEndpoint::NamedSource {
            index,
            name: source.name.clone(),
        };
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
        validate_json5_options(
            &format!("{location} format options"),
            &source.options,
            &mut issues,
        );
        validate_external_source_options(
            &format!("{location} format options"),
            &source.options,
            true,
            &mut issues,
        );
        validate_structured_edi_options(
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
            &ValidationSchemaLocation::root(endpoint.clone()),
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
        own_issues(&mut issues[start..], ValidationOwner::Endpoint(endpoint));
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
            owner: &ValidationScopeLocation::root(ValidationEndpoint::Target),
        },
        &mut Vec::new(),
        &[],
        &mut BTreeMap::new(),
        &mut issues,
    );
    for (index, target) in project.extra_targets.iter().enumerate() {
        validate_scope(
            project,
            &target.root,
            ScopeSchemas {
                target: Some(&target.schema),
                parent_source: Some(&project.source),
                owner: &ValidationScopeLocation::root(ValidationEndpoint::NamedTarget {
                    index,
                    name: target.name.clone(),
                }),
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

#[cfg(test)]
mod owner_tests;
