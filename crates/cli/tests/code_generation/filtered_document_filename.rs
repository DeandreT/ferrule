use super::*;

const INPUT: &str = "<Input><Branch><Label>North.xml</Label><Person><First>Aster</First><Last>A</Last></Person><Person><First>Birch</First><Last>B</Last></Person></Branch><Branch><Label></Label></Branch><Branch><Label>South.xml</Label><Person><First>Cedar</First><Last>C</Last></Person></Branch></Input>";

#[test]
fn filtered_document_filename_matches_native_and_generated_public_hosts() -> TestResult<()> {
    let mapping = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/filtered_document_filename/filtered.mfd");
    let project = mfd::import_with_profile(
        &mapping,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?
    .imported
    .project;
    let source = format_xml::from_str(INPUT, &project.source)?;
    let directory = TempDir::new("filtered_document_filename")?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("target"));
    super::reference_corpus_generic::document_outputs::run_case(
        &project,
        &source,
        &[],
        &mapping,
        &directory.0,
        &target,
    )?;
    let node = project.root.output_path().unwrap();
    let root = &directory.0;
    let rust = include_str!("filtered_document_filename/Failure.rs.txt")
        .replace("EXPECTED_NODE", &node.to_string());
    std::fs::write(root.join("rust-host/src/main.rs"), rust)?;
    let result = Command::new("cargo")
        .args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
        .arg(root)
        .current_dir(root.join("rust-host"))
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    std::fs::write(root.join("failure-rust.stdout"), &result.stdout)?;
    std::fs::write(root.join("failure-rust.stderr"), &result.stderr)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let csharp = include_str!("filtered_document_filename/Failure.cs.txt")
        .replace("EXPECTED_NODE", &node.to_string());
    std::fs::write(root.join("csharp/Harness/Program.cs"), csharp)?;
    let result = dotnet_command(&root.join("csharp"))
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--",
        ])
        .arg(root)
        .current_dir(root.join("csharp"))
        .isolated_output()?;
    std::fs::write(root.join("failure-csharp.stdout"), &result.stdout)?;
    std::fs::write(root.join("failure-csharp.stderr"), &result.stderr)?;
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
