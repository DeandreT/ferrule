//! In-memory format dispatch and mapping execution for browser hosts.

use std::fmt;

use ir::Instance;
use mapping::Project;

/// Instance document formats supported by the browser runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    Xml,
    Json,
    Csv,
}

impl fmt::Display for DataFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Xml => "XML",
            Self::Json => "JSON",
            Self::Csv => "CSV",
        })
    }
}

/// A failure from one stage of browser-side mapping execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidProject(Vec<String>),
    Parse { format: DataFormat, message: String },
    Execute(String),
    Serialize { format: DataFormat, message: String },
    CsvTargetNotRepeated,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProject(issues) => {
                write!(
                    formatter,
                    "project has {} validation issue(s)",
                    issues.len()
                )
            }
            Self::Parse { format, message } => {
                write!(formatter, "could not parse {format} source: {message}")
            }
            Self::Execute(message) => write!(formatter, "mapping failed: {message}"),
            Self::Serialize { format, message } => {
                write!(formatter, "could not serialize {format} target: {message}")
            }
            Self::CsvTargetNotRepeated => {
                formatter.write_str("mapping did not produce a repeating row set for a CSV target")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Parses one source document entirely in memory using the project's source
/// schema and format options.
pub fn parse_source(
    project: &Project,
    text: &str,
    format: DataFormat,
) -> Result<Instance, RuntimeError> {
    match format {
        DataFormat::Xml => {
            format_xml::from_str(text, &project.source).map_err(|error| RuntimeError::Parse {
                format,
                message: error.to_string(),
            })
        }
        DataFormat::Json => {
            format_json::from_str(text, &project.source).map_err(|error| RuntimeError::Parse {
                format,
                message: error.to_string(),
            })
        }
        DataFormat::Csv => format_csv::from_str(
            text,
            &project.source,
            project.source_options.delimiter,
            project.source_options.has_header_row.unwrap_or(true),
        )
        .map(Instance::Repeated)
        .map_err(|error| RuntimeError::Parse {
            format,
            message: error.to_string(),
        }),
    }
}

/// Serializes one target document entirely in memory using the project's
/// target schema and format options.
pub fn serialize_target(
    project: &Project,
    target: &Instance,
    format: DataFormat,
) -> Result<String, RuntimeError> {
    match format {
        DataFormat::Xml => format_xml::to_string(&project.target, target).map_err(|error| {
            RuntimeError::Serialize {
                format,
                message: error.to_string(),
            }
        }),
        DataFormat::Json => format_json::to_string(&project.target, target).map_err(|error| {
            RuntimeError::Serialize {
                format,
                message: error.to_string(),
            }
        }),
        DataFormat::Csv => {
            let rows = target
                .as_repeated()
                .ok_or(RuntimeError::CsvTargetNotRepeated)?;
            format_csv::to_string(
                &project.target,
                rows,
                project.target_options.delimiter,
                project.target_options.has_header_row.unwrap_or(true),
            )
            .map_err(|error| RuntimeError::Serialize {
                format,
                message: error.to_string(),
            })
        }
    }
}

/// Validates and runs a project against source text, returning target text.
pub fn run(
    project: &Project,
    source_text: &str,
    source_format: DataFormat,
    target_format: DataFormat,
) -> Result<String, RuntimeError> {
    let issues: Vec<String> = engine::validate(project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect();
    if !issues.is_empty() {
        return Err(RuntimeError::InvalidProject(issues));
    }
    let source = parse_source(project, source_text, source_format)?;
    let target =
        engine::run(project, &source).map_err(|error| RuntimeError::Execute(error.to_string()))?;
    serialize_target(project, &target, target_format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};
    use mapping::{Binding, FormatOptions, Graph, Node, Scope, ScopeIteration};

    fn scalar_project(iterate_rows: bool) -> Project {
        let source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        );
        let target = SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        );
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["name".into()],
                frame: None,
            },
        );
        graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["age".into()],
                frame: None,
            },
        );
        Project {
            source,
            target,
            source_path: None,
            target_path: None,
            source_options: FormatOptions::default(),
            target_options: FormatOptions::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                iteration: if iterate_rows {
                    ScopeIteration::Source(Vec::new())
                } else {
                    ScopeIteration::None
                },
                bindings: vec![
                    Binding {
                        target_field: "name".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "age".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            },
        }
    }

    #[test]
    fn runs_xml_source_to_json_target() {
        let project = scalar_project(false);

        let output = run(
            &project,
            "<Source><name>Ada</name><age>37</age></Source>",
            DataFormat::Xml,
            DataFormat::Json,
        )
        .unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            serde_json::json!({"name": "Ada", "age": 37})
        );
    }

    #[test]
    fn csv_options_and_repeated_rows_apply_to_both_sides() {
        let mut project = scalar_project(true);
        project.source_options.delimiter = Some(';');
        project.target_options.delimiter = Some('|');
        project.target_options.has_header_row = Some(false);

        let output = run(
            &project,
            "name;age\nAda;37\nGrace;42\n",
            DataFormat::Csv,
            DataFormat::Csv,
        )
        .unwrap();

        assert_eq!(output, "Ada|37\nGrace|42\n");
    }

    #[test]
    fn csv_target_requires_a_repeated_row_set() {
        let project = scalar_project(false);

        let error = run(
            &project,
            r#"{"name":"Ada","age":37}"#,
            DataFormat::Json,
            DataFormat::Csv,
        )
        .unwrap_err();

        assert_eq!(error, RuntimeError::CsvTargetNotRepeated);
    }
}
