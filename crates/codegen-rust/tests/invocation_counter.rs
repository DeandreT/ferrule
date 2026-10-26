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
#[path = "../../../conformance/invocation-counter-expressions/expected.rs"]
mod expected;

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
fn generated_nested_counter_operands_are_ordered_without_memoization() -> Result<(), Box<dyn Error>>
{
    run_fixture(
        fixture::nested_project(),
        "counter_order",
        include_str!("../../../conformance/invocation-counter-expressions/OrderingHost.rs.txt"),
        "PASS ordered nested counters",
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
fn generated_counter_failures_retain_nested_operand_effects() -> Result<(), Box<dyn Error>> {
    run_fixture_harness(
        fixture::failure_project(),
        "counter_effects",
        FixtureHarness::Unit(include_str!(
            "../../../conformance/invocation-counter-expressions/RetainedEffects.rs.txt"
        )),
    )
}

enum FixtureHarness<'a> {
    Main { source: &'a str, expected: &'a str },
    Unit(&'a str),
}

fn run_fixture(
    project: mapping::Project,
    package_name: &str,
    host: &str,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    run_fixture_harness(
        project,
        package_name,
        FixtureHarness::Main {
            source: host,
            expected,
        },
    )
}

fn run_fixture_harness(
    project: mapping::Project,
    package_name: &str,
    harness: FixtureHarness<'_>,
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
    let artifacts = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: package_name.into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_string_lossy().into_owned(),
            ),
        },
    )?;
    for file in artifacts.files() {
        let path = root.0.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &file.contents)?;
    }
    // Supply decimal input directly to isolate operand checking from literal
    // emission and JSON boundary admission.
    let manifest_path = root.0.join("Cargo.toml");
    let mut manifest = std::fs::read_to_string(&manifest_path)?;
    manifest.push_str(&format!(
        "\n[dependencies.ir]\npath = {:?}\n",
        runtime.join("../ir")
    ));
    std::fs::write(manifest_path, manifest)?;
    let mut command = Command::new("cargo");
    match &harness {
        FixtureHarness::Main { source, .. } => {
            std::fs::write(root.0.join("src/main.rs"), source)?;
            command.args(["run", "--quiet", "--offline"]);
        }
        FixtureHarness::Unit(source) => {
            let path = root.0.join("src/lib.rs");
            let mut library = std::fs::read_to_string(&path)?;
            library.push('\n');
            library.push_str(source);
            std::fs::write(path, library)?;
            command.args(["test", "--quiet", "--offline", "--lib"]);
        }
    }
    let output = command
        .current_dir(&root.0)
        .generated_host_output(&root.0)?;
    std::fs::write(root.0.join("stdout.txt"), &output.stdout)?;
    std::fs::write(root.0.join("stderr.txt"), &output.stderr)?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match harness {
        FixtureHarness::Main { expected, .. } => {
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected)
        }
        FixtureHarness::Unit(_) => {
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"))
        }
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
        codegen_rust::emit_with_basic_owned_dynamic_loading,
        codegen_rust::emit_with_basic_owned_document_paths,
        codegen_rust::emit_with_basic_owned_dynamic_document_paths,
        codegen_rust::emit_with_basic_owned_xml_document_sets,
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
fn ordinary_counter_emission_accepts_both_companion_selections() {
    let program = codegen::lower(&fixture::project()).unwrap();
    let options = codegen_rust::Options {
        package_name: "counter_companions".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    for companions in [
        codegen::CompanionSelection::Complete,
        codegen::CompanionSelection::TypedOnly,
    ] {
        assert!(codegen_rust::emit_with_companions(&program, &options, companions).is_ok());
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
