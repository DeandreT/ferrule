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
    Xbrl,
}

impl fmt::Display for DataFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Xml => "XML",
            Self::Json => "JSON",
            Self::Csv => "CSV",
            Self::Xbrl => "XBRL",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSide {
    Source,
    Target,
}

impl fmt::Display for DataSide {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Source => "source",
            Self::Target => "target",
        })
    }
}

/// A failure from one stage of browser-side mapping execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidProject(Vec<String>),
    XbrlFormatRequired { side: DataSide },
    XbrlBoundaryRequired { side: DataSide },
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
            Self::XbrlFormatRequired { side } => {
                write!(
                    formatter,
                    "the project {side} is XBRL; select the XBRL format"
                )
            }
            Self::XbrlBoundaryRequired { side } => write!(
                formatter,
                "the XBRL {side} format requires XBRL boundary metadata in the project"
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
    validate_xbrl_format(
        project.source_options.xbrl.is_some(),
        format,
        DataSide::Source,
    )?;
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
        DataFormat::Xbrl => {
            let options =
                project
                    .source_options
                    .xbrl
                    .as_ref()
                    .ok_or(RuntimeError::XbrlBoundaryRequired {
                        side: DataSide::Source,
                    })?;
            format_xbrl::from_str_with_options(text, &project.source, options).map_err(|error| {
                RuntimeError::Parse {
                    format,
                    message: error.to_string(),
                }
            })
        }
    }
}

/// Serializes one target document entirely in memory using the project's
/// target schema and format options.
pub fn serialize_target(
    project: &Project,
    target: &Instance,
    format: DataFormat,
) -> Result<String, RuntimeError> {
    validate_xbrl_format(
        project.target_options.xbrl.is_some(),
        format,
        DataSide::Target,
    )?;
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
        DataFormat::Xbrl => {
            let options =
                project
                    .target_options
                    .xbrl
                    .as_ref()
                    .ok_or(RuntimeError::XbrlBoundaryRequired {
                        side: DataSide::Target,
                    })?;
            format_xbrl::to_string(&project.target, target, options).map_err(|error| {
                RuntimeError::Serialize {
                    format,
                    message: error.to_string(),
                }
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
    reject_xbrl_extra_sources(project)?;
    validate_xbrl_format(
        project.source_options.xbrl.is_some(),
        source_format,
        DataSide::Source,
    )?;
    validate_xbrl_format(
        project.target_options.xbrl.is_some(),
        target_format,
        DataSide::Target,
    )?;
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

fn reject_xbrl_extra_sources(project: &Project) -> Result<(), RuntimeError> {
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

fn validate_xbrl_format(
    has_boundary: bool,
    format: DataFormat,
    side: DataSide,
) -> Result<(), RuntimeError> {
    match (has_boundary, format == DataFormat::Xbrl) {
        (true, false) => Err(RuntimeError::XbrlFormatRequired { side }),
        (false, true) => Err(RuntimeError::XbrlBoundaryRequired { side }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode, Value, XML_TEXT_FIELD};
    use mapping::{
        Binding, FormatOptions, Graph, NamedSource, Node, Scope, ScopeIteration,
        XbrlBoundaryOptions, XbrlNamespaceBinding,
    };

    const XBRLI: &str = "http://www.xbrl.org/2003/instance";

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
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
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

    fn xbrl_source_project() -> Result<Project, Box<dyn std::error::Error>> {
        let mut source_row = SchemaNode::group(
            "rows",
            vec![
                SchemaNode::group(
                    "period",
                    vec![SchemaNode::scalar("instant", ScalarType::String)],
                ),
                SchemaNode::scalar("Amount", ScalarType::Int),
            ],
        );
        source_row.repeating = true;
        let mut target_row =
            SchemaNode::group("row", vec![SchemaNode::scalar("Amount", ScalarType::Int)]);
        target_row.repeating = true;

        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["Amount".into()],
                frame: Some(vec!["rows".into()]),
            },
        );
        let mut project = scalar_project(false);
        project.source = SchemaNode::group("xbrl", vec![source_row]);
        project.target = SchemaNode::group("Result", vec![target_row]);
        project.source_options.xbrl = Some(
            XbrlBoundaryOptions::external_source("taxonomy.xsd")?.with_namespace_bindings(vec![
                XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI)?,
                XbrlNamespaceBinding::new(
                    vec!["rows".into(), "period".into(), "instant".into()],
                    XBRLI,
                )?,
                XbrlNamespaceBinding::new(vec!["rows".into(), "Amount".into()], "urn:facts")?,
            ])?,
        );
        project.graph = graph;
        project.root = Scope {
            children: vec![Scope {
                target_field: "row".into(),
                iteration: ScopeIteration::Source(vec!["rows".into()]),
                bindings: vec![Binding {
                    target_field: "Amount".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        Ok(project)
    }

    fn xbrl_target_project() -> Result<Project, Box<dyn std::error::Error>> {
        let mut source_row = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("Entity", ScalarType::String),
                SchemaNode::scalar("Instant", ScalarType::String),
                SchemaNode::scalar("Label", ScalarType::String),
            ],
        );
        source_row.repeating = true;
        let mut target_row = SchemaNode::group(
            "rows",
            vec![
                SchemaNode::group(
                    "identifier",
                    vec![
                        SchemaNode::scalar("scheme", ScalarType::String).attribute(),
                        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                    ],
                ),
                SchemaNode::group(
                    "period",
                    vec![SchemaNode::scalar("instant", ScalarType::String)],
                ),
                SchemaNode::scalar("Label", ScalarType::String),
            ],
        );
        target_row.repeating = true;

        let mut graph = Graph::default();
        for (id, field) in [(0, "Entity"), (1, "Instant"), (2, "Label")] {
            graph.nodes.insert(
                id,
                Node::SourceField {
                    path: vec![field.into()],
                    frame: Some(vec!["row".into()]),
                },
            );
        }
        graph.nodes.insert(
            3,
            Node::Const {
                value: Value::String("urn:web-demo".into()),
            },
        );

        let mut project = scalar_project(false);
        project.source = SchemaNode::group("Source", vec![source_row]);
        project.target = SchemaNode::group("xbrl", vec![target_row]);
        project.target_options.xbrl = Some(
            XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?.with_namespace_bindings(
                vec![
                    XbrlNamespaceBinding::new(vec!["rows".into(), "identifier".into()], XBRLI)?,
                    XbrlNamespaceBinding::new(vec!["rows".into(), "period".into()], XBRLI)?,
                    XbrlNamespaceBinding::new(vec!["rows".into(), "Label".into()], "urn:facts")?,
                ],
            )?,
        );
        project.graph = graph;
        project.root = Scope {
            children: vec![Scope {
                target_field: "rows".into(),
                iteration: ScopeIteration::Source(vec!["row".into()]),
                bindings: vec![Binding {
                    target_field: "Label".into(),
                    node: 2,
                }],
                children: vec![
                    Scope {
                        target_field: "identifier".into(),
                        bindings: vec![
                            Binding {
                                target_field: "scheme".into(),
                                node: 3,
                            },
                            Binding {
                                target_field: XML_TEXT_FIELD.into(),
                                node: 0,
                            },
                        ],
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "period".into(),
                        bindings: vec![Binding {
                            target_field: "instant".into(),
                            node: 1,
                        }],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        Ok(project)
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
    fn xbrl_boundaries_and_selected_formats_must_agree() -> Result<(), Box<dyn std::error::Error>> {
        let project = xbrl_source_project()?;

        assert_eq!(
            parse_source(&project, "not,csv", DataFormat::Csv),
            Err(RuntimeError::XbrlFormatRequired {
                side: DataSide::Source,
            })
        );
        let project = scalar_project(false);
        assert_eq!(
            parse_source(&project, "<xbrl/>", DataFormat::Xbrl),
            Err(RuntimeError::XbrlBoundaryRequired {
                side: DataSide::Source,
            })
        );
        let mut project = scalar_project(false);
        project.target_options.xbrl =
            Some(XbrlBoundaryOptions::external_target("taxonomy.xsd", None)?);
        assert_eq!(
            serialize_target(&project, &Instance::Group(Vec::new()), DataFormat::Xml),
            Err(RuntimeError::XbrlFormatRequired {
                side: DataSide::Target,
            })
        );
        project.target_options.xbrl = None;
        assert_eq!(
            serialize_target(&project, &Instance::Group(Vec::new()), DataFormat::Xbrl),
            Err(RuntimeError::XbrlBoundaryRequired {
                side: DataSide::Target,
            })
        );
        Ok(())
    }

    #[test]
    fn runs_xbrl_source_to_json_target() -> Result<(), Box<dyn std::error::Error>> {
        let project = xbrl_source_project()?;
        let source = r#"<xbrli:xbrl xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:f="urn:facts">
          <xbrli:context id="c"><xbrli:entity><xbrli:identifier scheme="urn:id">Entity</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context>
          <f:Amount contextRef="c">11</f:Amount>
        </xbrli:xbrl>"#;

        let output = run(&project, source, DataFormat::Xbrl, DataFormat::Json)?;

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output)?,
            serde_json::json!({"row": [{"Amount": 11}]})
        );
        Ok(())
    }

    #[test]
    fn runs_json_source_to_xbrl_target() -> Result<(), Box<dyn std::error::Error>> {
        let project = xbrl_target_project()?;
        let output = run(
            &project,
            r#"{"row":[{"Entity":"Entity","Instant":"2026-06-30","Label":"Ready"}]}"#,
            DataFormat::Json,
            DataFormat::Xbrl,
        )?;

        assert!(output.contains("<xbrli:identifier scheme=\"urn:web-demo\">Entity"));
        assert!(output.contains("<xbrli:instant>2026-06-30</xbrli:instant>"));
        assert!(output.contains("contextRef=\"c1\">Ready"));
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
            dynamic_path: None,
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
