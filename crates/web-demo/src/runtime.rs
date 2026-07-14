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
    XbrlSourceNotExecutable,
    XbrlTargetNotExecutable,
    XbrlExtraSourceNotExecutable { name: String },
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
            Self::XbrlSourceNotExecutable => formatter.write_str(
                "XBRL source input is not executable in the web demo; native XBRL reading is not supported",
            ),
            Self::XbrlTargetNotExecutable => formatter.write_str(
                "XBRL target output is not executable in the web demo; native XBRL writing is not supported",
            ),
            Self::XbrlExtraSourceNotExecutable { name } => write!(
                formatter,
                "extra source `{name}` is an XBRL boundary and is not executable in the web demo"
            ),
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
    if project.source_options.xbrl.is_some() {
        return Err(RuntimeError::XbrlSourceNotExecutable);
    }
    match format {
        DataFormat::Xml => {
            format_xml::from_str(text, &project.source).map_err(|error| RuntimeError::Parse {
                format,
                message: error.to_string(),
            })
        }
        DataFormat::Json => {
            let parsed = if project.source_options.json_lines {
                format_json::from_lines(text, &project.source)
            } else {
                format_json::from_str(text, &project.source)
            };
            parsed.map_err(|error| RuntimeError::Parse {
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
    if project.target_options.xbrl.is_some() {
        return Err(RuntimeError::XbrlTargetNotExecutable);
    }
    match format {
        DataFormat::Xml => format_xml::to_string(&project.target, target).map_err(|error| {
            RuntimeError::Serialize {
                format,
                message: error.to_string(),
            }
        }),
        DataFormat::Json => {
            let serialized = if project.target_options.json_lines {
                format_json::to_lines(&project.target, target)
            } else {
                format_json::to_string(&project.target, target)
            };
            serialized.map_err(|error| RuntimeError::Serialize {
                format,
                message: error.to_string(),
            })
        }
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
    reject_xbrl_boundaries(project)?;
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

fn reject_xbrl_boundaries(project: &Project) -> Result<(), RuntimeError> {
    if project.source_options.xbrl.is_some() {
        return Err(RuntimeError::XbrlSourceNotExecutable);
    }
    if project.target_options.xbrl.is_some() {
        return Err(RuntimeError::XbrlTargetNotExecutable);
    }
    if let Some(source) = project
        .extra_sources
        .iter()
        .find(|source| source.options.xbrl.is_some())
    {
        return Err(RuntimeError::XbrlExtraSourceNotExecutable {
            name: source.name.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};
    use mapping::{
        Binding, FormatOptions, Graph, NamedSource, Node, Scope, ScopeIteration,
        XbrlBoundaryOptions,
    };

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
    fn json_lines_options_apply_to_browser_input_and_output() {
        let mut project = scalar_project(true);
        project.source_options.json_lines = true;
        project.target_options.json_lines = true;

        let output = run(
            &project,
            "{\"name\":\"Ada\",\"age\":37}\n{\"name\":\"Grace\",\"age\":42}\n",
            DataFormat::Json,
            DataFormat::Json,
        )
        .unwrap();

        assert_eq!(
            output,
            "{\"name\":\"Ada\",\"age\":37}\n{\"name\":\"Grace\",\"age\":42}\n"
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

    #[test]
    fn xbrl_source_rejects_before_ui_selected_source_parsing()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = scalar_project(false);
        project.source_options.xbrl = Some(XbrlBoundaryOptions::external_source("taxonomy.xsd")?);

        assert_eq!(
            parse_source(&project, "not,csv", DataFormat::Csv),
            Err(RuntimeError::XbrlSourceNotExecutable)
        );
        assert_eq!(
            run(&project, "not,csv", DataFormat::Csv, DataFormat::Json,),
            Err(RuntimeError::XbrlSourceNotExecutable)
        );
        Ok(())
    }

    #[test]
    fn xbrl_target_rejects_before_source_parsing_or_target_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut project = scalar_project(false);
        project.target_options.xbrl = Some(XbrlBoundaryOptions::external_target(
            "taxonomy.xsd",
            Some("table.sps"),
        )?);

        assert_eq!(
            serialize_target(&project, &Instance::Group(Vec::new()), DataFormat::Xml),
            Err(RuntimeError::XbrlTargetNotExecutable)
        );
        assert_eq!(
            run(&project, "not valid XML", DataFormat::Xml, DataFormat::Csv,),
            Err(RuntimeError::XbrlTargetNotExecutable)
        );
        Ok(())
    }

    #[test]
    fn xbrl_extra_source_rejects_before_execution() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = scalar_project(false);
        project.extra_sources.push(NamedSource {
            name: "filing".to_owned(),
            path: "filing.xbrl".to_owned(),
            schema: SchemaNode::group("Filing", Vec::new()),
            options: FormatOptions {
                xbrl: Some(XbrlBoundaryOptions::external_source("taxonomy.xsd")?),
                ..FormatOptions::default()
            },
        });

        assert_eq!(
            run(
                &project,
                "not valid JSON",
                DataFormat::Json,
                DataFormat::Xml,
            ),
            Err(RuntimeError::XbrlExtraSourceNotExecutable {
                name: "filing".to_owned(),
            })
        );
        Ok(())
    }
}
