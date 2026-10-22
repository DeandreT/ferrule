use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

#[path = "../../../conformance/invocation-counter-expressions/admission.rs"]
mod admission;
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
#[path = "../../../conformance/invocation-counter-expressions/project.rs"]
mod fixture;
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
        include_str!("../../../conformance/invocation-counter-expressions/Host.cs.txt"),
        "PASS counter documents",
    )
}

#[test]
fn generated_counter_operand_errors_are_ordered_and_typed() -> Result<(), Box<dyn Error>> {
    run_fixture(
        fixture::source_project(),
        include_str!("../../../conformance/invocation-counter-expressions/OperandsHost.cs.txt"),
        &expected::operands(),
    )
}

#[test]
fn generated_counter_rows_share_state_and_keep_restart_presence() -> Result<(), Box<dyn Error>> {
    run_fixture(
        fixture::rows_project(),
        include_str!("../../../conformance/invocation-counter-expressions/RowsHost.cs.txt"),
        &expected::rows(),
    )
}

#[test]
fn generated_optional_presence_keeps_counter_evaluation_order() -> Result<(), Box<dyn Error>> {
    run_fixture(
        optional_presence::project(fixture::project()),
        include_str!(
            "../../../conformance/invocation-counter-expressions/OptionalPresenceHost.cs.txt"
        ),
        "PASS optional counter documents",
    )
}

#[test]
fn generated_document_rows_keep_grouped_files_and_counter_state() -> Result<(), Box<dyn Error>> {
    run_fixture(
        document_rows::project(),
        include_str!("../../../conformance/document-rows/Host.cs.txt"),
        "PASS document row sets",
    )
}

#[test]
fn generated_document_visitor_stops_before_later_bodies_and_preserves_accepted_documents()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        document_rows::project(),
        include_str!("../../../conformance/document-rows/VisitorHost.cs.txt"),
        "PASS document visitors",
        Some((None, false)),
    )
}

#[test]
fn generated_document_visitor_selects_named_target_without_running_primary()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        named_document_rows::project(document_rows::project()),
        include_str!("../../../conformance/document-rows/VisitorHost.cs.txt"),
        "PASS document visitors",
        Some((Some("selected"), false)),
    )
}

#[test]
fn generated_document_filename_clock_is_lazy_and_body_local() -> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(
        document_clocks::project(),
        include_str!("../../../conformance/document-clocks/Host.cs.txt"),
        "PASS document filename clocks",
        Some((None, true)),
    )
}

#[test]
fn generated_document_body_clocks_are_reached_and_distinct_from_filenames()
-> Result<(), Box<dyn Error>> {
    run_fixture_with_options(
        document_body_clocks::project(document_clocks::project()),
        include_str!("../../../conformance/document-body-clocks/Host.cs.txt"),
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
        include_str!("../../../conformance/document-body-clocks/Host.cs.txt"),
        "PASS document body clocks",
        Some((Some("selected"), true)),
        true,
    )
}

#[test]
fn generated_csv_visitor_publishes_only_complete_accepted_documents() -> Result<(), Box<dyn Error>>
{
    run_fixture_with_visitor(
        document_publication::project(document_rows::project()),
        include_str!("../../../conformance/document-rows/PublicationHost.cs.txt"),
        "PASS document publication",
        Some((None, true)),
    )
}

fn run_fixture(
    project: mapping::Project,
    host: &str,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    run_fixture_with_visitor(project, host, expected, None)
}

fn run_fixture_with_visitor(
    project: mapping::Project,
    host: &str,
    expected: &str,
    visitor: Option<(Option<&str>, bool)>,
) -> Result<(), Box<dyn Error>> {
    run_fixture_with_options(project, host, expected, visitor, false)
}

fn run_fixture_with_options(
    project: mapping::Project,
    host: &str,
    expected: &str,
    visitor: Option<(Option<&str>, bool)>,
    body_clock: bool,
) -> Result<(), Box<dyn Error>> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = Temporary(std::env::temp_dir().join(format!(
        "ferrule_counter_csharp_{}_{nonce}",
        std::process::id()
    )));
    std::fs::create_dir(&root.0)?;
    let program = codegen::lower(&project)?;
    let artifacts = if body_clock {
        codegen_csharp::emit_with_document_clocks(&program, visitor.and_then(|v| v.0))?
    } else if visitor == Some((None, true))
        && program.root.construction == codegen::TargetConstruction::DocumentRows
    {
        codegen_csharp::emit_with_csv_document_set_visitor(
            &program,
            &codegen::CsvOutputPolicy {
                delimiter: Some('|'),
                quote_disabled: true,
                has_headers: false,
                raw_delimited_crlf: true,
                ..Default::default()
            },
        )?
    } else if let Some((target, true)) = visitor {
        codegen_csharp::emit_with_document_filename_clock(&program, target)?
    } else if let Some((target, false)) = visitor {
        codegen_csharp::emit_with_document_visitor(&program, target)?
    } else if program.root.construction == codegen::TargetConstruction::DocumentRows {
        codegen_csharp::emit_with_csv_document_set_output(
            &program,
            &codegen::CsvOutputPolicy {
                delimiter: Some('|'),
                quote_disabled: true,
                has_headers: false,
                raw_delimited_crlf: true,
                ..Default::default()
            },
        )?
    } else {
        codegen_csharp::emit(&program)?
    };
    for file in artifacts.files() {
        let path = root.0.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &file.contents)?;
    }
    std::fs::create_dir(root.0.join("Harness"))?;
    std::fs::write(
        root.0.join("Harness/Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(root.0.join("Harness/Program.cs"), host)?;
    let output = Command::new("dotnet")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
        ])
        .current_dir(&root.0)
        .output()?;
    std::fs::write(root.0.join("stdout.txt"), &output.stdout)?;
    std::fs::write(root.0.join("stderr.txt"), &output.stderr)?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // Console framing is platform-native; payload bytes remain exact hex.
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn owned_emitters_refuse_reachable_and_unused_counters() {
    let emitters = [
        codegen_csharp::emit_with_basic_owned_sources,
        codegen_csharp::emit_with_basic_owned_filters,
        codegen_csharp::emit_with_basic_owned_sort,
        codegen_csharp::emit_with_basic_owned_windows,
        codegen_csharp::emit_with_basic_owned_row_failures,
        codegen_csharp::emit_with_basic_owned_group_by,
        codegen_csharp::emit_with_basic_owned_path_hierarchy,
        codegen_csharp::emit_with_basic_owned_xml_serialize,
        codegen_csharp::emit_with_basic_owned_sequence_item_at,
        codegen_csharp::emit_with_basic_owned_group_adjacent_by,
        codegen_csharp::emit_with_basic_owned_group_starting_with,
        codegen_csharp::emit_with_basic_owned_group_ending_with,
        codegen_csharp::emit_with_basic_owned_group_into_blocks,
        codegen_csharp::emit_with_basic_owned_failure_sources,
        codegen_csharp::emit_with_basic_owned_adjacency_tree,
        codegen_csharp::emit_with_basic_owned_recursive_collect,
        codegen_csharp::emit_with_basic_owned_sequence_exists,
        codegen_csharp::emit_with_basic_owned_recursive_filter,
        codegen_csharp::emit_with_basic_owned_lookup_find,
        codegen_csharp::emit_with_basic_owned_dynamic_field,
        codegen_csharp::emit_with_basic_owned_sequence_aggregate,
        codegen_csharp::emit_with_basic_owned_collection_aggregates,
        codegen_csharp::emit_with_basic_owned_joins,
        codegen_csharp::emit_with_basic_owned_xml_mixed_content,
        codegen_csharp::emit_with_basic_owned_xml_mixed_construction,
    ];
    for program in admission::owned_programs() {
        for emit in emitters {
            assert!(matches!(
                emit(&program),
                Err(codegen_csharp::EmitError::ProgramValidation(
                    codegen::ProgramValidationError::OwnedSource { .. }
                ))
            ));
        }
    }
}

#[test]
fn unused_user_function_counter_is_refused_without_closing_scalar_functions() {
    let (mut program, expected) = admission::function_program();
    assert!(
        matches!(codegen_csharp::emit(&program), Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected)
    );
    admission::make_function_valid(&mut program);
    assert!(codegen_csharp::emit(&program).is_ok());
}
