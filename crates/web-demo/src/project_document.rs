//! Filesystem-free project JSON for the browser editor.

use mapping::Project;

#[derive(Debug)]
pub enum ProjectDocumentError {
    Serialize(serde_json::Error),
    Parse(serde_json::Error),
    Validation(Vec<engine::ValidationIssue>),
}

impl std::fmt::Display for ProjectDocumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialize(error) => {
                write!(formatter, "could not serialize project JSON: {error}")
            }
            Self::Parse(error) => write!(formatter, "could not parse project JSON: {error}"),
            Self::Validation(issues) => {
                write!(
                    formatter,
                    "project has {} validation issue{}",
                    issues.len(),
                    if issues.len() == 1 { "" } else { "s" }
                )?;
                if let Some(first) = issues.first() {
                    write!(formatter, ": {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ProjectDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(error) | Self::Parse(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

pub fn to_json(project: &Project) -> Result<String, ProjectDocumentError> {
    serde_json::to_string_pretty(project).map_err(ProjectDocumentError::Serialize)
}

pub fn parse_and_validate(json: &str) -> Result<Project, ProjectDocumentError> {
    let project: Project = serde_json::from_str(json).map_err(ProjectDocumentError::Parse)?;
    let issues = engine::validate(&project);
    if issues.is_empty() {
        Ok(project)
    } else {
        Err(ProjectDocumentError::Validation(issues))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};
    use mapping::{Binding, Graph, Scope};

    fn valid_project() -> Project {
        Project {
            source: SchemaNode::group(
                "source",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            ),
            target: SchemaNode::group(
                "target",
                vec![SchemaNode::scalar("result", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            graph: Graph::default(),
            root: Scope::default(),
        }
    }

    #[test]
    fn project_json_roundtrips_through_validation() {
        let project = valid_project();
        let json = to_json(&project).expect("valid project serializes");
        let parsed = parse_and_validate(&json).expect("roundtrip project validates");

        assert_eq!(
            serde_json::to_value(parsed).expect("parsed project serializes"),
            serde_json::to_value(project).expect("original project serializes")
        );
    }

    #[test]
    fn invalid_json_returns_a_parse_error() {
        let error = parse_and_validate(r#"{"source": }"#).expect_err("JSON should not parse");

        let ProjectDocumentError::Parse(source) = &error else {
            panic!("expected a parse error, got {error}");
        };
        assert!(source.line() > 0);
        assert!(
            error
                .to_string()
                .starts_with("could not parse project JSON:")
        );
    }

    #[test]
    fn validation_failure_returns_all_issues_without_a_project() {
        let mut project = valid_project();
        project.root.bindings.push(Binding {
            target_field: "missing-target".into(),
            node: 99,
        });
        let json = to_json(&project).expect("invalid projects remain serializable");
        let error = parse_and_validate(&json).expect_err("invalid project must be rejected");

        let ProjectDocumentError::Validation(issues) = &error else {
            panic!("expected validation issues, got {error}");
        };
        assert!(issues.len() >= 2);
        assert!(issues.iter().any(|issue| issue.message.contains("node 99")));
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("missing-target"))
        );
        assert!(error.to_string().starts_with("project has "));
    }
}
