//! Modest Release consumer: real public APIs and actual MapForce completion.
use super::line_ending_fixture as fixture;
use super::*;
use mapping::X12SegmentLineEnding as Ending;

#[test]
fn selected_segment_line_endings_compile_and_run_release_consumers() -> Result<(), Box<dyn Error>> {
    let root = evidence()?;
    fs::create_dir(root.join("Harness"))?;
    fs::create_dir(root.join("src"))?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .display()
        .to_string()
        .replace('\\', "/");
    let mut manifest = format!(
        "[package]\nname = \"ending-host\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[[bin]]\nname = \"ending-host\"\npath = \"../src/main.rs\"\n[workspace]\n[dependencies]\ncodegen-runtime = {{ path = {runtime:?} }}\n"
    );
    let mut host = String::from(
        "fn main() -> Result<(), Box<dyn std::error::Error>> {\nlet context = codegen_runtime::ExecutionContext::new(std::path::Path::new(\"toy.mfd\")).with_current_datetime(\"2030-11-22T10:15:00Z\");\n",
    );
    for (name, envelope, ending, completion) in [
        (
            "ending_single_lf",
            X12EnvelopeProfile::SingleTransaction,
            Ending::Lf,
            false,
        ),
        (
            "ending_single_crlf",
            X12EnvelopeProfile::SingleTransaction,
            Ending::CrLf,
            false,
        ),
        (
            "ending_mapforce_lf",
            X12EnvelopeProfile::MapForceInterchanges,
            Ending::Lf,
            false,
        ),
        (
            "ending_mapforce_crlf",
            X12EnvelopeProfile::MapForceInterchanges,
            Ending::CrLf,
            false,
        ),
        (
            "ending_completion",
            X12EnvelopeProfile::MapForceInterchanges,
            Ending::CrLf,
            true,
        ),
    ] {
        let program = codegen::lower(&fixture::project(envelope))?;
        let copies = if envelope == X12EnvelopeProfile::MapForceInterchanges {
            2
        } else {
            1
        };
        let policy = fixture::policy(envelope, ending, completion);
        let options = Options {
            package_name: name.into(),
            runtime_dependency: RuntimeDependency::Path(runtime.clone()),
        };
        let artifacts = codegen_rust::Generator::new(&options)
            .with_manifest(codegen::ManifestSelection::Included)
            .emit_with_x12(&program, &policy)?;
        assert_eq!(
            artifacts,
            codegen_rust::Generator::new(&options)
                .with_manifest(codegen::ManifestSelection::Included)
                .emit_with_x12(&program, &policy)?
        );
        let record = artifacts
            .files()
            .iter()
            .find(|f| f.path.as_str() == "ferrule-manifest.json")
            .ok_or("manifest")?;
        let record =
            codegen::CompatibilityManifest::from_json(std::str::from_utf8(&record.contents)?)?;
        assert_eq!(record.runtime.package, "codegen-runtime");
        assert_eq!(
            record.runtime.binding,
            codegen::RuntimeBinding::Path {
                path: runtime.clone()
            }
        );
        assert_eq!(record.runtime.identity, codegen_rust::runtime_identity()?);
        for file in artifacts.files() {
            let path = root.join(name).join(file.path.as_str());
            fs::create_dir_all(path.parent().ok_or("parent")?)?;
            fs::write(path, &file.contents)?;
        }
        fs::write(
            root.join(format!("{name}.expected")),
            fixture::expected(ending.as_str()).repeat(copies),
        )?;
        manifest.push_str(&format!(
            "{name} = {{ path = {:?} }}\n",
            format!("../{name}")
        ));
        if completion {
            let mut values = fixture::values(envelope, false);
            let owners = values["Interchanges"].as_array_mut().unwrap();
            let second = owners[0].clone();
            owners.push(second);
            fs::write(root.join(format!("{name}.json")), values.to_string())?;
            host.push_str(&format!(r#"
{{ let input = include_str!("../{name}.json"); let before = input.as_bytes().to_vec(); let expected = include_str!("../{name}.expected");
assert!({name}::execute_json_to_x12(input).is_err());
assert!({name}::execute_json_to_x12_bytes(input.as_bytes()).is_err());
for _ in 0..2 {{
assert_eq!({name}::execute_json_to_x12_with_context(input, &context)?, expected);
assert_eq!({name}::execute_json_to_x12_bytes_with_context(input.as_bytes(), &context)?, expected.as_bytes());
}} assert_eq!(input.as_bytes(), before); }}
"#));
        } else {
            fs::write(
                root.join(format!("{name}.input")),
                fixture::expected("\n").repeat(copies),
            )?;
            host.push_str(&format!(r#"
{{ let input = include_str!("../{name}.input"); let expected = include_str!("../{name}.expected");
let document = {name}::parse_x12(input)?; let before = document.clone();
assert_eq!({name}::parse_x12_bytes(input.as_bytes())?, document);
for _ in 0..2 {{
assert_eq!({name}::serialize_x12(&document)?, expected);
assert_eq!({name}::serialize_x12_bytes(&document)?, expected.as_bytes());
assert_eq!({name}::serialize_x12_with_context(&document, &context)?, expected);
assert_eq!({name}::serialize_x12_bytes_with_context(&document, &context)?, expected.as_bytes());
assert_eq!({name}::execute_x12_to_x12(input)?, expected);
assert_eq!({name}::execute_x12_to_x12_bytes(input.as_bytes())?, expected.as_bytes());
assert_eq!({name}::execute_x12_to_x12_with_context(input, &context)?, expected);
assert_eq!({name}::execute_x12_to_x12_bytes_with_context(input.as_bytes(), &context)?, expected.as_bytes());
}} assert_eq!(document, before); }}
"#));
        }
    }
    host.push_str("println!(\"selected line-ending Release consumers completed\"); Ok(()) }\n");
    fs::write(root.join("src/main.rs"), host)?;
    fs::write(root.join("Harness/Cargo.toml"), manifest)?;
    let output = Command::new(env!("CARGO"))
        .args(["run", "--release", "--offline", "--jobs", "1"])
        .current_dir(root.join("Harness"))
        .generated_host_output(&root)?;
    fs::write(root.join("line-endings.stdout"), &output.stdout)?;
    fs::write(root.join("line-endings.stderr"), &output.stderr)?;
    assert!(
        output.status.success(),
        "Release consumer originals: {}",
        root.display()
    );
    Ok(())
}
