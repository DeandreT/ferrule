use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../../../conformance/invocation-counter-expressions/project.rs"]
mod fixture;
#[path = "../src/tests/host_target.rs"]
#[allow(dead_code)]
mod host_target;
use host_target::GeneratedHostCommand;

#[path = "../../../conformance/invocation-counter-expressions/admission.rs"]
mod admission;
#[path = "../../../conformance/distinct-text-map/project.rs"]
mod distinct_text_map;
#[path = "../../../conformance/document-body-clocks/project.rs"]
mod document_body_clocks;
#[path = "../../../conformance/document-clocks/project.rs"]
mod document_clocks;
#[path = "../../../conformance/document-rows/publication.rs"]
mod document_publication;
#[path = "../../../conformance/document-rows/project.rs"]
mod document_rows;
#[path = "../../../conformance/invocation-counter-expressions/expected.rs"]
mod expected;
#[path = "../../../conformance/mapforce-json5-documents/project.rs"]
mod json5_documents;
#[path = "../../../conformance/document-rows/named.rs"]
mod named_document_rows;
#[path = "../../../conformance/invocation-counter-expressions/optional_presence.rs"]
mod optional_presence;

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn generated_counter_documents_share_only_invocation_state() -> Result<(), Box<dyn Error>> {
    run_fixture(
        fixture::parameter_project(),
        "counter_fixture",
        include_str!("../../../conformance/invocation-counter-expressions/Host.rs.txt"),
        "PASS counter documents",
    )
}

#[test]
fn generated_counter_operand_errors_are_ordered_and_typed() -> Result<(), Box<dyn Error>> {
    run_fixture(
        fixture::source_project(),
        "counter_operands",
        include_str!("../../../conformance/invocation-counter-expressions/OperandsHost.rs.txt"),
        &expected::operands(),
    )
}

#[test]
fn generated_counter_rows_share_state_and_keep_restart_presence() -> Result<(), Box<dyn Error>> {
    run_fixture(
        fixture::rows_project(),
        "counter_rows",
        include_str!("../../../conformance/invocation-counter-expressions/RowsHost.rs.txt"),
        &expected::rows(),
    )
}

#[test]
fn generated_optional_presence_keeps_counter_evaluation_order() -> Result<(), Box<dyn Error>> {
    run_fixture(
        optional_presence::project(fixture::project()),
        "counter_optional_presence",
        include_str!(
            "../../../conformance/invocation-counter-expressions/OptionalPresenceHost.rs.txt"
        ),
        "PASS optional counter documents",
    )
}

#[test]
fn generated_document_rows_keep_grouped_files_and_counter_state() -> Result<(), Box<dyn Error>> {
    run_fixture(
        document_rows::project(),
        "document_rows_mapping",
        include_str!("../../../conformance/document-rows/Host.rs.txt"),
        "PASS document row sets",
    )
}

#[test]
fn generated_document_visitor_stops_before_later_bodies_and_preserves_accepted_documents()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        document_rows::project(),
        "document_visitor_mapping",
        include_str!("../../../conformance/document-rows/VisitorHost.rs.txt"),
        "PASS document visitors",
        Some((None, false)),
    )
}

#[test]
fn generated_document_visitor_selects_named_target_without_running_primary()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        named_document_rows::project(document_rows::project()),
        "document_visitor_mapping",
        include_str!("../../../conformance/document-rows/VisitorHost.rs.txt"),
        "PASS document visitors",
        Some((Some("selected"), false)),
    )
}

#[test]
fn generated_document_filename_clock_is_lazy_and_body_local() -> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        document_clocks::project(),
        "document_clock_mapping",
        include_str!("../../../conformance/document-clocks/Host.rs.txt"),
        "PASS document filename clocks",
        Some((None, true)),
    )
}

#[test]
fn generated_document_body_clocks_are_reached_and_distinct_from_filenames()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_options(
        document_body_clocks::project(document_clocks::project()),
        "document_body_clock_mapping",
        include_str!("../../../conformance/document-body-clocks/Host.rs.txt"),
        "PASS document body clocks",
        Some((None, true)),
        true,
    )
}

#[test]
fn generated_document_body_clocks_keep_the_unselected_primary_inactive()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_options(
        document_body_clocks::named(document_body_clocks::project(document_clocks::project())),
        "document_body_clock_mapping",
        include_str!("../../../conformance/document-body-clocks/Host.rs.txt"),
        "PASS document body clocks",
        Some((Some("selected"), true)),
        true,
    )
}

#[test]
fn generated_distinct_text_map_preserves_key_phase_and_mapped_effects() -> Result<(), Box<dyn Error>>
{
    run_fixture_with_options(
        distinct_text_map::project(document_clocks::project()),
        "distinct_text_mapping",
        include_str!("../../../conformance/distinct-text-map/Host.rs.txt"),
        "PASS distinct text map",
        Some((None, true)),
        true,
    )
}

#[test]
fn generated_csv_visitor_publishes_only_complete_accepted_documents() -> Result<(), Box<dyn Error>>
{
    run_fixture_with_visitor(
        document_publication::project(document_rows::project()),
        "document_publication_mapping",
        include_str!("../../../conformance/document-rows/PublicationHost.rs.txt"),
        "PASS document publication",
        Some((None, true)),
    )
}

fn run_fixture(
    project: mapping::Project,
    package_name: &str,
    host: &str,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(project, package_name, host, expected, None)
}

fn run_fixture_with_visitor(
    project: mapping::Project,
    package_name: &str,
    host: &str,
    expected: &str,
    visitor: Option<(Option<&str>, bool)>,
) -> Result<(), Box<dyn Error>> {
    run_fixture_with_options(project, package_name, host, expected, visitor, false)
}

fn run_fixture_with_options(
    project: mapping::Project,
    package_name: &str,
    host: &str,
    expected: &str,
    visitor: Option<(Option<&str>, bool)>,
    body_clock: bool,
) -> Result<(), Box<dyn Error>> {
    run_fixture_with_document(
        project,
        package_name,
        host,
        expected,
        visitor,
        body_clock,
        None,
    )
}

fn run_fixture_with_document(
    project: mapping::Project,
    package_name: &str,
    host: &str,
    expected: &str,
    visitor: Option<(Option<&str>, bool)>,
    body_clock: bool,
    filename: Option<mapping::NodeId>,
) -> Result<(), Box<dyn Error>> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = Temporary(std::env::temp_dir().join(format!(
        "ferrule_counter_rust_{}_{nonce}",
        std::process::id()
    )));
    std::fs::create_dir(&root.0)?;
    let program = codegen::lower(&project)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    let options = codegen_rust::Options {
        package_name: package_name.into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            runtime.to_string_lossy().into_owned(),
        ),
    };
    let artifacts = if let Some(filename) = filename {
        let document = codegen::lower_document(&mapping::DocumentProject::new(
            project,
            mapping::PrimarySourcePlan { filename },
        ))?;
        codegen_rust::emit_csv_to_mapforce_json5_documents(
            &document,
            &options,
            &codegen::CsvInputPolicy {
                has_headers: false,
                ..Default::default()
            },
            visitor.and_then(|v| v.0),
        )?
    } else if body_clock {
        codegen_rust::emit_with_document_clocks(&program, &options, visitor.and_then(|v| v.0))?
    } else if visitor == Some((None, true))
        && program.root.construction == codegen::TargetConstruction::DocumentRows
    {
        codegen_rust::emit_with_csv_document_set_visitor(
            &program,
            &options,
            &codegen::CsvOutputPolicy {
                delimiter: Some('|'),
                quote_disabled: true,
                has_headers: false,
                raw_delimited_crlf: true,
                ..Default::default()
            },
        )?
    } else if let Some((target, true)) = visitor {
        codegen_rust::emit_with_document_filename_clock(&program, &options, target)?
    } else if let Some((target, false)) = visitor {
        codegen_rust::emit_with_document_visitor(&program, &options, target)?
    } else if program.root.construction == codegen::TargetConstruction::DocumentRows {
        codegen_rust::emit_with_csv_document_set_output(
            &program,
            &options,
            &codegen::CsvOutputPolicy {
                delimiter: Some('|'),
                quote_disabled: true,
                has_headers: false,
                raw_delimited_crlf: true,
                ..Default::default()
            },
        )?
    } else {
        codegen_rust::emit(&program, &options)?
    };
    for file in artifacts.files() {
        let path = root.0.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &file.contents)?;
    }
    std::fs::write(root.0.join("src/main.rs"), host)?;
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&root.0)
        .generated_host_output(&root.0)?;
    std::fs::write(root.0.join("stdout.txt"), &output.stdout)?;
    std::fs::write(root.0.join("stderr.txt"), &output.stderr)?;
    if filename.is_some() && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some() {
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.0.join("target"));
        let products = root.0.join("qualified-host");
        std::fs::create_dir(&products)?;
        for extension in [std::env::consts::EXE_EXTENSION, "pdb"] {
            let name = if extension.is_empty() {
                package_name.into()
            } else {
                format!("{package_name}.{extension}")
            };
            let binary = target.join("debug").join(&name);
            if binary.exists() {
                std::fs::copy(binary, products.join(name))?;
            }
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    Ok(())
}

#[test]
fn generated_json5_documents_preserve_decimal_origins_and_clocks() -> Result<(), Box<dyn Error>> {
    for named in [false, true] {
        run_fixture_with_document(
            json5_documents::project(named),
            "json5_documents",
            include_str!("../../../conformance/mapforce-json5-documents/Host.rs.txt"),
            "PASS mapped JSON5 documents",
            Some((named.then_some("selected"), true)),
            true,
            Some(90),
        )?;
    }
    Ok(())
}

#[test]
fn owned_emitters_refuse_reachable_and_unused_counters() {
    let emitters = [
        codegen_rust::emit_with_basic_owned_sources,
        codegen_rust::emit_with_basic_owned_filters,
        codegen_rust::emit_with_basic_owned_sort,
        codegen_rust::emit_with_basic_owned_windows,
        codegen_rust::emit_with_basic_owned_row_failures,
        codegen_rust::emit_with_basic_owned_group_by,
        codegen_rust::emit_with_basic_owned_path_hierarchy,
        codegen_rust::emit_with_basic_owned_xml_serialize,
        codegen_rust::emit_with_basic_owned_sequence_item_at,
        codegen_rust::emit_with_basic_owned_group_adjacent_by,
        codegen_rust::emit_with_basic_owned_group_starting_with,
        codegen_rust::emit_with_basic_owned_group_ending_with,
        codegen_rust::emit_with_basic_owned_group_into_blocks,
        codegen_rust::emit_with_basic_owned_failure_sources,
        codegen_rust::emit_with_basic_owned_adjacency_tree,
        codegen_rust::emit_with_basic_owned_recursive_collect,
        codegen_rust::emit_with_basic_owned_sequence_exists,
        codegen_rust::emit_with_basic_owned_recursive_filter,
        codegen_rust::emit_with_basic_owned_lookup_find,
        codegen_rust::emit_with_basic_owned_dynamic_field,
        codegen_rust::emit_with_basic_owned_sequence_aggregate,
        codegen_rust::emit_with_basic_owned_collection_aggregates,
        codegen_rust::emit_with_basic_owned_joins,
        codegen_rust::emit_with_basic_owned_xml_mixed_content,
        codegen_rust::emit_with_basic_owned_xml_mixed_construction,
    ];
    let options = codegen_rust::Options {
        package_name: "closed_counter".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    for program in admission::owned_programs() {
        for emit in emitters {
            assert!(matches!(
                emit(&program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(
                    codegen::ProgramValidationError::OwnedSource { .. }
                ))
            ));
        }
    }
}

#[test]
fn unused_user_function_counter_is_refused_without_closing_scalar_functions() {
    let (mut program, expected) = admission::function_program();
    let options = codegen_rust::Options {
        package_name: "closed_counter".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    assert!(
        matches!(codegen_rust::emit(&program, &options), Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected)
    );
    admission::make_function_valid(&mut program);
    assert!(codegen_rust::emit(&program, &options).is_ok());
}
