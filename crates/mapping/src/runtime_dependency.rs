use std::fmt;

use crate::{EdiConfigDependency, FormatOptions, Project};

/// One mapping boundary whose runtime behavior depends on an external resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeBoundary {
    PrimarySource,
    PrimaryTarget,
    NamedSource(String),
    NamedTarget(String),
}

impl fmt::Display for RuntimeBoundary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrimarySource => formatter.write_str("primary source"),
            Self::PrimaryTarget => formatter.write_str("primary target"),
            Self::NamedSource(name) => write!(formatter, "named source `{name}`"),
            Self::NamedTarget(name) => write!(formatter, "named target `{name}`"),
        }
    }
}

/// An external resource required before a project can execute faithfully.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeDependency {
    EdiConfiguration {
        boundary: RuntimeBoundary,
        dependency: EdiConfigDependency,
    },
}

impl RuntimeDependency {
    pub fn boundary(&self) -> &RuntimeBoundary {
        match self {
            Self::EdiConfiguration { boundary, .. } => boundary,
        }
    }

    pub fn edi_config(&self) -> &EdiConfigDependency {
        match self {
            Self::EdiConfiguration { dependency, .. } => dependency,
        }
    }

    pub fn reference(&self) -> Option<&str> {
        match self {
            Self::EdiConfiguration { dependency, .. } => dependency.reference(),
        }
    }
}

impl fmt::Display for RuntimeDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EdiConfiguration {
                boundary,
                dependency: EdiConfigDependency::ExternalReference(reference),
            } => {
                write!(
                    formatter,
                    "{boundary} requires unresolved external EDI configuration `{reference}`"
                )
            }
            Self::EdiConfiguration {
                boundary,
                dependency: EdiConfigDependency::MissingConfiguration,
            } => write!(
                formatter,
                "{boundary} requires an EDI configuration because its imported entry-tree schema \
                 is untyped"
            ),
        }
    }
}

impl Project {
    /// Returns external resources that must be supplied before boundary I/O is executable.
    pub fn runtime_dependencies(&self) -> Vec<RuntimeDependency> {
        let mut dependencies = Vec::new();
        collect_edi_dependency(
            &mut dependencies,
            RuntimeBoundary::PrimarySource,
            &self.source_options,
        );
        collect_edi_dependency(
            &mut dependencies,
            RuntimeBoundary::PrimaryTarget,
            &self.target_options,
        );
        for source in &self.extra_sources {
            collect_edi_dependency(
                &mut dependencies,
                RuntimeBoundary::NamedSource(source.name.clone()),
                &source.options,
            );
        }
        for target in &self.extra_targets {
            collect_edi_dependency(
                &mut dependencies,
                RuntimeBoundary::NamedTarget(target.name.clone()),
                &target.options,
            );
        }
        dependencies
    }
}

fn collect_edi_dependency(
    dependencies: &mut Vec<RuntimeDependency>,
    boundary: RuntimeBoundary,
    options: &FormatOptions,
) {
    if let Some(dependency) = &options.edi_config_reference {
        dependencies.push(RuntimeDependency::EdiConfiguration {
            boundary,
            dependency: dependency.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use ir::SchemaNode;

    use super::*;
    use crate::{Graph, NamedSource, NamedTarget, Scope};

    fn project() -> Project {
        Project {
            source: SchemaNode::group("source", Vec::new()),
            target: SchemaNode::group("target", Vec::new()),
            source_path: None,
            target_path: None,
            source_options: FormatOptions {
                edi_config_reference: Some("X12/850.Config".into()),
                ..FormatOptions::default()
            },
            target_options: FormatOptions::default(),
            extra_sources: vec![NamedSource {
                name: "catalog".into(),
                path: String::new(),
                schema: SchemaNode::group("catalog", Vec::new()),
                options: FormatOptions::default(),
                dynamic_path: None,
            }],
            extra_targets: vec![NamedTarget {
                name: "ack".into(),
                path: None,
                schema: SchemaNode::group("ack", Vec::new()),
                options: FormatOptions {
                    edi_config_reference: Some("X12/997.Config".into()),
                    ..FormatOptions::default()
                },
                root: Scope::default(),
            }],
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope::default(),
        }
    }

    #[test]
    fn project_reports_unresolved_edi_dependencies_in_boundary_order() {
        let dependencies = project().runtime_dependencies();
        assert_eq!(
            dependencies,
            vec![
                RuntimeDependency::EdiConfiguration {
                    boundary: RuntimeBoundary::PrimarySource,
                    dependency: "X12/850.Config".into(),
                },
                RuntimeDependency::EdiConfiguration {
                    boundary: RuntimeBoundary::NamedTarget("ack".into()),
                    dependency: "X12/997.Config".into(),
                },
            ]
        );
        assert_eq!(
            dependencies[0].to_string(),
            "primary source requires unresolved external EDI configuration `X12/850.Config`"
        );
        assert_eq!(dependencies[0].reference(), Some("X12/850.Config"));
    }

    #[test]
    fn project_reports_missing_edi_configuration_without_a_fake_reference() {
        let mut project = project();
        project.source_options.edi_config_reference =
            Some(EdiConfigDependency::MissingConfiguration);

        let dependencies = project.runtime_dependencies();

        assert_eq!(dependencies[0].reference(), None);
        assert_eq!(
            dependencies[0].to_string(),
            "primary source requires an EDI configuration because its imported entry-tree schema is untyped"
        );
    }
}
