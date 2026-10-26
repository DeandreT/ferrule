use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Json5InterchangeDependency, Node, Project, Scope};

const REFUSAL: &str = "native JSON5 interchange";

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-json5-hosts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    let schema = SchemaNode::group(
        "Document",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("missing-input.json".into()),
        target_path: Some("output.json".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn refuse(error: impl std::fmt::Display) {
    assert!(error.to_string().contains(REFUSAL), "{error}");
}

struct NeverRead;
impl Read for NeverRead {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("refusal must precede stdin acquisition")
    }
}

#[test]
fn retained_dependencies_refuse_before_native_io_or_generated_publication() -> anyhow::Result<()> {
    for source in [true, false] {
        for (dependency, internal) in [
            (
                Json5InterchangeDependency::ComponentSetting { value: "1".into() },
                false,
            ),
            (Json5InterchangeDependency::InstanceExtension {}, false),
            (
                Json5InterchangeDependency::ComponentSetting { value: "1".into() },
                true,
            ),
        ] {
            let directory = Directory::new();
            let mut project = project();
            let options = if source {
                &mut project.source_options
            } else {
                &mut project.target_options
            };
            if internal {
                options.mfd_json5_string_dependencies.push(
                    mapping::Json5StringInterchangeDependency {
                        component: "parser".into(),
                        kind: mapping::Json5StringComponentKind::Parser,
                        dependency,
                        input_instance: Some("preview.json".into()),
                        output_instance: None,
                    },
                );
            } else {
                options.json5_interchange_dependency = Some(dependency);
            }
            let project_path = directory.0.join("project.json");
            std::fs::write(
                &project_path,
                mapping::project_file::encode_pretty(&project)?,
            )?;
            let project = cli::load_project(&project_path)?;
            let output = directory.0.join("output.json");
            std::fs::write(&output, b"keep existing output")?;
            let missing = directory.0.join("not-there.json");
            refuse(cli::run_project(&project_path, &missing, &output).unwrap_err());
            assert_eq!(std::fs::read(&output)?, b"keep existing output");
            let payload = cli::PayloadDocument::new(Path::new("input.json"), b"not JSON")?;
            let payload_options = cli::PayloadRunOptions::new(payload);
            refuse(
                cli::run_project_value_payloads(&project, &project_path, &payload_options)
                    .unwrap_err(),
            );
            refuse(
                cli::preview_project_value(
                    &project,
                    &project_path,
                    cli::ProjectPreviewInputs::Payloads(&payload_options),
                )
                .unwrap_err(),
            );
            let stream_options = cli::StandardIoRunOptions::new()
                .with_input_path(Path::new("-"))
                .with_output_path(Path::new("-"));
            refuse(
                cli::run_project_with_standard_streams(
                    &project_path,
                    &stream_options,
                    &mut NeverRead,
                )
                .unwrap_err(),
            );
            // The graph interpreter takes already typed values; format admission
            // belongs to validation and the hosts that acquire/encode documents.
            assert!(
                engine::validate(&project)
                    .iter()
                    .any(|issue| issue.message.contains(REFUSAL))
            );
            refuse(
                codegen::lower(&project)
                    .unwrap_err()
                    .diagnostics()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );

            for target in [
                cli::GenerateTarget::CSharp,
                cli::GenerateTarget::Rust {
                    runtime_path: directory.0.join("nonexistent-runtime"),
                },
            ] {
                for profile in [cli::GenerateProfile::Ordinary, cli::GenerateProfile::Json5] {
                    for existing in [false, true] {
                        let destination = directory.0.join(if existing {
                            "existing"
                        } else {
                            "absent/generated"
                        });
                        if existing {
                            std::fs::create_dir_all(&destination)?;
                            std::fs::write(destination.join("keep.txt"), "keep")?;
                        }
                        let error = cli::generate_project_with_profile(
                            &project_path,
                            &destination,
                            target.clone(),
                            profile,
                        )
                        .unwrap_err();
                        assert!(
                            format!("{error:#}").contains(REFUSAL)
                                || format!("{error:#}").contains("json5_interchange_dependency")
                                || format!("{error:#}").contains("mfd_json5_string_dependencies"),
                            "{error:#}"
                        );
                        if existing {
                            assert_eq!(
                                std::fs::read_to_string(destination.join("keep.txt"))?,
                                "keep"
                            );
                            assert_eq!(std::fs::read_dir(destination)?.count(), 1);
                        } else {
                            assert!(!destination.parent().unwrap().exists());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn named_boundary_dependencies_are_validated_even_without_active_json5_flag() -> anyhow::Result<()>
{
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/json5-boundary.mfd");
    let base = mfd::import(&fixture)?.project;
    for named_source in [true, false] {
        let mut project = base.clone();
        let options = if named_source {
            &mut project.extra_sources[0].options
        } else {
            &mut project.extra_targets[0].options
        };
        options.json5_interchange_dependency =
            Some(Json5InterchangeDependency::ComponentSetting { value: "0".into() });
        let project =
            mapping::project_file::decode_str(&mapping::project_file::encode_pretty(&project)?)?;
        let issues = engine::validate(&project);
        assert!(
            issues.iter().any(|issue| issue.message.contains(REFUSAL)),
            "{issues:?}"
        );
        let primary = cli::PayloadDocument::new(Path::new("source.json"), b"not JSON")?;
        let named = [cli::NamedPayloadInput::new(
            &project.extra_sources[0].name,
            cli::PayloadDocument::new(Path::new("auxiliary.json"), b"not JSON")?,
        )?];
        let inputs = cli::PayloadRunOptions::new(primary).with_extra_sources(&named);
        refuse(
            cli::run_project_value_payloads(&project, Path::new("/virtual/project.json"), &inputs)
                .unwrap_err(),
        );
        assert!(codegen::lower(&project).is_err());
        let directory = Directory::new();
        let saved = directory.0.join("project.json");
        std::fs::write(&saved, mapping::project_file::encode_pretty(&project)?)?;
        let options = cli::StandardIoRunOptions::new()
            .with_input_path(Path::new("-"))
            .with_output_path(Path::new("-"));
        refuse(
            cli::run_project_with_standard_streams(&saved, &options, &mut NeverRead).unwrap_err(),
        );
    }
    Ok(())
}

#[test]
fn primary_filename_plans_refuse_retained_dependencies_before_acquisition() -> anyhow::Result<()> {
    for source in [true, false] {
        let mut project = project();
        let options = if source {
            &mut project.source_options
        } else {
            &mut project.target_options
        };
        options.json_document = true;
        options.json5_interchange_dependency =
            Some(Json5InterchangeDependency::InstanceExtension {});
        project.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::String("missing-input.json".into()),
            },
        );
        let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(1));
        refuse(document.primary_route().unwrap_err());
        refuse(cli::check_primary_document(&document).unwrap_err());
        let directory = Directory::new();
        let design = directory.0.join("document.json");
        std::fs::write(
            &design,
            mapping::document_project_file::encode_pretty(&document)?,
        )?;
        let destination = directory.0.join("generated");
        refuse(
            cli::generate_project_with_profile(
                &design,
                &destination,
                cli::GenerateTarget::CSharp,
                cli::GenerateProfile::PrimaryDocument,
            )
            .unwrap_err(),
        );
        assert!(!destination.exists());
    }
    Ok(())
}

#[test]
fn executable_import_refuses_without_replacing_a_saved_project() -> anyhow::Result<()> {
    let directory = Directory::new();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    std::fs::copy(
        fixtures.join("json5-boundary.schema.json"),
        directory.0.join("json5-boundary.schema.json"),
    )?;
    let xml = std::fs::read_to_string(fixtures.join("json5-boundary.mfd"))?.replacen(
        "inputinstance=\"source.json\"",
        "inputinstance=\"source.json\" json5=\"1\"",
        1,
    );
    let design = directory.0.join("design.mfd");
    std::fs::write(&design, xml)?;
    let destination = directory.0.join("project.json");
    std::fs::write(&destination, b"keep saved project")?;
    let error = cli::import_mfd_with_profile(
        &design,
        &destination,
        Some(&directory.0),
        None,
        &[],
        &[],
        mfd::ImportProfile::Executable,
    )
    .unwrap_err();
    refuse(format!("{error:#}"));
    assert_eq!(std::fs::read(&destination)?, b"keep saved project");
    cli::import_mfd_with_profile(
        &design,
        &destination,
        Some(&directory.0),
        None,
        &[],
        &[],
        mfd::ImportProfile::BestEffort,
    )?;
    let project = cli::load_project(&destination)?;
    assert_eq!(
        project.source_options.json5_interchange_dependency,
        Some(Json5InterchangeDependency::ComponentSetting { value: "1".into() })
    );
    assert!(!project.source_options.json5);
    Ok(())
}
