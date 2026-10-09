//! Source proposal for the bounded scalar-sequence memory study.
#[cfg(target_os = "linux")]
#[path = "../../../examples/performance/scalar-sequences/rust_support.rs"]
mod support;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
use mapping::Project;
#[cfg(target_os = "linux")]
use serde_json::json;
#[cfg(target_os = "linux")]
use support::{Result, require};

// Only the native host links engine. Preserve its typed causes without adding
// that dependency to the separately measured generated Rust host.
#[cfg(target_os = "linux")]
fn native_feature(cause: &(dyn std::error::Error + 'static)) -> Option<serde_json::Value> {
    use engine::EngineError;
    match cause.downcast_ref::<EngineError>()? {
        EngineError::FilterMapRuntime { boundary, .. } => Some(json!({
            "type":"EngineError", "kind":"FilterMapRuntime", "boundary":{
                "item":boundary.item, "phase":format!("{:?}", boundary.phase),
                "capture_index":boundary.capture_index, "source_position":boundary.source_position,
                "function":boundary.function.map(mapping::FunctionId::get), "node":boundary.node,
                "kind":format!("{:?}", boundary.kind)}})),
        EngineError::FilterMapValueType { expected, found } => Some(json!({
            "type":"EngineError", "kind":"FilterMapValueType",
            "expected":format!("{expected:?}"), "found":support::scalar(found)})),
        EngineError::FilterMapNonFinite { bits } => Some(json!({
            "type":"EngineError", "kind":"FilterMapNonFinite", "bits":format!("{bits:016x}")})),
        EngineError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => Some(json!({
            "type":"EngineError", "kind":"FilterMapBudget", "budget_kind":format!("{kind:?}"),
            "used":used.to_string(), "requested":requested.to_string(), "max":max.to_string()})),
        EngineError::FilterMapCancelled => {
            Some(json!({"type":"EngineError", "kind":"FilterMapCancelled"}))
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn native_error_original(cause: &(dyn std::error::Error + 'static)) -> serde_json::Value {
    support::error_with(cause, native_feature)
}

#[cfg(target_os = "linux")]
fn outcome<T: std::fmt::Debug, E: std::error::Error + 'static>(
    result: &std::result::Result<T, E>,
) -> serde_json::Value {
    match result {
        Ok(_) => support::outcome(result),
        Err(cause) => native_error_original(cause),
    }
}

#[cfg(target_os = "linux")]
fn project(path: &Path, dir: &Path) -> Result<Project> {
    let row = support::retain_file(path, &dir.join("project.original.json"))?;
    support::retain(&dir.join("project-identity.original.json"), &row)?;
    let text = support::read_text(&dir.join("project.original.json"))?;
    let result = mapping::project_file::decode_str(&text);
    support::retain(
        &dir.join("project-codec-result.original.json"),
        &outcome(&result),
    )?;
    let project = result?;
    support::write(
        &dir.join("project-reencoded.original.json"),
        mapping::project_file::encode_pretty(&project)?.as_bytes(),
    )?;
    require(row["stable"] == true, "project changed while retained")?;
    Ok(project)
}

#[cfg(target_os = "linux")]
fn native_json(project: &Project, text: &str) -> Result<String> {
    // These typed values are local: the returned String survives into host file writing.
    let source = format_json::from_str(text, &project.source)?;
    let target = engine::run(project, &source)?;
    Ok(format_json::to_string(&project.target, &target)?)
}

#[cfg(target_os = "linux")]
fn measure(args: &[String]) -> Result<()> {
    require(args.len() == 4, "measure MODE PROJECT INPUT FRESH_DIR")?;
    support::mode(&args[0])?;
    let paths = support::paths(&args[1..])?;
    let dir = &paths[2];
    support::fresh_dir(dir)?;
    let started = Instant::now();
    // Outer owner has already retained/pinned complete Project/input bytes.
    let text = support::read_text(&paths[1])?;
    let project = mapping::project_file::decode_str(&support::read_text(&paths[0])?)?;
    support::phase("before-call", started)?;
    let actual = native_json(&project, &text);
    match actual {
        Ok(document) => {
            support::phase("after-call", started)?;
            require(
                document.len() < support::CEILING,
                "output exceeds study ceiling",
            )?;
            support::write(&dir.join("actual.json"), document.as_bytes())?;
            support::phase("after-write", started)?;
        }
        Err(error) => {
            support::retain(
                &dir.join("error.original.json"),
                &native_error_original(error.as_ref()),
            )?;
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify(args: &[String]) -> Result<()> {
    require(
        args.len() == 8,
        "verify MODE COUNT WIDTH PROJECT INPUT EXPECTED TIMED FRESH_DIR",
    )?;
    support::mode(&args[0])?;
    let count: usize = args[1].parse()?;
    let width: usize = args[2].parse()?;
    support::dimension(count, width)?;
    let paths = support::paths(&args[3..])?;
    let dir = &paths[4];
    support::fresh_dir(dir)?;
    let project = project(&paths[0], dir)?;
    let input_row = support::retain_file(&paths[1], &dir.join("input.original.json"))?;
    support::retain(&dir.join("input-identity.original.json"), &input_row)?;
    let text = support::read_text(&dir.join("input.original.json"))?;
    let source = format_json::from_str(&text, &project.source);
    support::retain(&dir.join("source-result.original.json"), &outcome(&source))?;
    if let Ok(value) = &source {
        support::retain(
            &dir.join("source-typed.original.json"),
            &support::instance(value),
        )?;
    }
    let expected_source = support::expected_source(count, width);
    let expected_output = support::expected_output(&args[0], count, width);
    support::retain(
        &dir.join("expected-source.original.json"),
        &support::instance(&expected_source),
    )?;
    support::retain(
        &dir.join("expected-output.original.json"),
        &support::instance(&expected_output),
    )?;
    let actual = source
        .as_ref()
        .ok()
        .map(|value| engine::run(&project, value));
    if let Some(result) = &actual {
        support::retain(&dir.join("execute-result.original.json"), &outcome(result))?;
        if let Ok(value) = result {
            support::retain(
                &dir.join("execute-typed.original.json"),
                &support::instance(value),
            )?;
        }
    } else {
        support::retain(
            &dir.join("execute-result.original.json"),
            &json!({"outcome":"NotReached", "cause":"source parse failed"}),
        )?;
    }
    let bytes_equal = support::verify_bytes(&paths[2], &paths[3], dir)?;
    let source_equal = source.as_ref().is_ok_and(|value| value == &expected_source);
    let output_equal = actual
        .as_ref()
        .is_some_and(|result| result.as_ref().is_ok_and(|value| value == &expected_output));
    support::retain(
        &dir.join("verification.original.json"),
        &json!({"source_equal":source_equal,
        "output_equal":output_equal, "bytes_equal":bytes_equal, "input_stable":input_row["stable"]}),
    )?;
    require(
        source_equal && output_equal && bytes_equal && input_row["stable"] == true,
        "complete retained native typed/byte verification failed",
    )
}

#[cfg(target_os = "linux")]
fn artifacts(directory: &Path, set: &codegen::ArtifactSet) -> Result<()> {
    fs::create_dir(directory)?;
    let mut rows = Vec::new();
    for file in set.files() {
        let path = directory.join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("artifact parent missing")?)?;
        support::write(&path, &file.contents)?;
        rows.push(json!({"path":file.path.as_str(), "bytes":file.contents.len()}));
    }
    support::retain(
        &directory.join("artifact-order.original.json"),
        &json!(rows),
    )
}

#[cfg(target_os = "linux")]
fn namespace(bytes: &[u8], mode: &str) -> Result<String> {
    let source = std::str::from_utf8(bytes)?;
    let old = "namespace Ferrule.Generated;";
    require(
        source.starts_with(old) && source.matches(old).count() == 1,
        "one exact leading generated namespace required",
    )?;
    Ok(source.replacen(old, &format!("namespace Ferrule.Generated.{mode};"), 1))
}

#[cfg(target_os = "linux")]
fn rust_workspace_member(set: &codegen::ArtifactSet) -> Result<codegen::ArtifactSet> {
    let mut files = set.files().to_vec();
    let manifest = files
        .iter_mut()
        .find(|file| file.path.as_str() == "Cargo.toml")
        .ok_or("generated Rust manifest missing")?;
    let text = std::str::from_utf8(&manifest.contents)?;
    require(
        text.matches("[workspace]").count() == 1,
        "one standalone workspace marker required",
    )?;
    manifest.contents = text
        .strip_suffix("\n[workspace]\n")
        .ok_or("exact terminal standalone workspace marker required")?
        .as_bytes()
        .to_vec();
    Ok(codegen::ArtifactSet::new(files)?)
}

#[cfg(target_os = "linux")]
fn prepare(args: &[String]) -> Result<()> {
    require(
        args.len() == 3,
        "prepare FIXTURE_DIR FRESH_SOURCE_DIR WORKSPACE_ROOT",
    )?;
    let paths = support::paths(args)?;
    let fixtures = &paths[0];
    let out = &paths[1];
    let workspace = &paths[2];
    support::fresh_dir(out)?;
    let emitted = out.join("emitted-originals");
    fs::create_dir(&emitted)?;
    let rust = out.join("rust");
    let csharp = out.join("csharp");
    fs::create_dir(&rust)?;
    fs::create_dir(&csharp)?;
    let runtime = workspace.join("crates/codegen-runtime");
    let mut csharp_sets = Vec::new();
    let mut schemas = Vec::new();
    for mode in ["numeric", "capture"] {
        let originals = emitted.join(mode);
        fs::create_dir(&originals)?;
        let project = project(&fixtures.join(format!("project-{mode}.json")), &originals)?;
        let issues = engine::validate(&project);
        support::retain(
            &originals.join("validation.original.json"),
            &json!({"complete_issues_debug":format!("{issues:#?}")}),
        )?;
        require(issues.is_empty(), "Project static admission failed")?;
        let program = codegen::lower(&project);
        support::retain(&originals.join("lower.original.json"), &outcome(&program))?;
        let program = program?;
        let options = codegen_rust::Options {
            package_name: format!("scalar_sequence_{mode}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_str().ok_or("non-UTF8 runtime path")?.to_owned(),
            ),
        };
        let result = codegen_rust::emit(&program, &options);
        if let Err(error) = &result {
            support::retain(
                &originals.join("rust-error.original.json"),
                &native_error_original(error),
            )?;
        }
        let set = result?;
        artifacts(&originals.join("rust"), &set)?;
        artifacts(&rust.join(mode), &rust_workspace_member(&set)?)?;
        let result = codegen_csharp::emit(&program);
        if let Err(error) = &result {
            support::retain(
                &originals.join("csharp-error.original.json"),
                &native_error_original(error),
            )?;
        }
        let set = result?;
        artifacts(&originals.join("csharp"), &set)?;
        csharp_sets.push(set);
        schemas.push(codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?);
    }
    require(schemas[0] == schemas[1], "matched input schemas differ")?;
    support::retain(
        &out.join("rust-packaging.original.json"),
        &json!({"members":["numeric", "capture"], "path":"Cargo.toml",
        "change":"remove sole exact terminal standalone workspace marker from composed members"}),
    )?;
    // The raw two complete emitted sets above remain authority; only packaging is transformed.
    let original = &csharp_sets[0];
    let second = &csharp_sets[1];
    require(
        original.files().len() == second.files().len(),
        "runtime file inventories differ",
    )?;
    let mut packaging = Vec::new();
    for (left, right) in original.files().iter().zip(second.files()) {
        require(left.path == right.path, "runtime file names/order differ")?;
        let name = left.path.as_str();
        if matches!(name, "GeneratedMapping.cs" | "GeneratedTargetBuilder.cs") {
            for (mode, bytes) in [("Numeric", &left.contents), ("Capture", &right.contents)] {
                let directory = csharp.join(mode);
                fs::create_dir_all(&directory)?;
                let transformed = namespace(bytes, mode)?;
                support::write(&directory.join(name), transformed.as_bytes())?;
                packaging.push(json!({"mode":mode, "path":name,
                    "change":"sole leading namespace Ferrule.Generated -> Ferrule.Generated.MODE"}));
            }
        } else {
            require(
                left.contents == right.contents,
                "shared runtime bytes differ",
            )?;
            if name != "Ferrule.Generated.csproj" {
                let path = csharp.join(name);
                fs::create_dir_all(path.parent().ok_or("runtime parent missing")?)?;
                support::write(&path, &left.contents)?;
            }
        }
    }
    support::retain(
        &out.join("csharp-packaging.original.json"),
        &json!(packaging),
    )?;
    fs::create_dir(rust.join("host"))?;
    fs::create_dir(rust.join("host/src"))?;
    support::write(
        &rust.join("host/src/main.rs"),
        include_bytes!("../../../examples/performance/scalar-sequences/rust_host.rs"),
    )?;
    support::write(
        &rust.join("host/src/support.rs"),
        include_bytes!("../../../examples/performance/scalar-sequences/rust_support.rs"),
    )?;
    support::write(
        &rust.join("host/src/source-schema.json"),
        schemas[0].as_bytes(),
    )?;
    support::write(&csharp.join("source-schema.json"), schemas[0].as_bytes())?;
    support::write(
        &csharp.join("Host.cs"),
        include_bytes!("../../../examples/performance/scalar-sequences/CSharpHost.cs"),
    )?;
    support::write(
        &rust.join("Cargo.toml"),
        b"[workspace]\nresolver = \"3\"\nmembers = [\"numeric\", \"capture\", \"host\"]\n",
    )?;
    let quote = |path: &Path| {
        serde_json::to_string(path.to_str().ok_or("non-UTF8 dependency path")?)
            .map_err(Box::<dyn std::error::Error>::from)
    };
    let manifest = format!(
        "[package]\nname = \"scalar_sequence_host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nnumeric = {{ package = \"scalar_sequence_numeric\", path = \"../numeric\" }}\ncapture = {{ package = \"scalar_sequence_capture\", path = \"../capture\" }}\ncodegen-runtime = {{ path = {} }}\nir = {{ path = {} }}\nserde_json = {{ version = \"=1.0.150\", features = [\"preserve_order\", \"float_roundtrip\"] }}\nsha2 = \"=0.10.9\"\n",
        quote(&runtime)?,
        quote(&workspace.join("crates/ir"))?
    );
    support::write(&rust.join("host/Cargo.toml"), manifest.as_bytes())?;
    support::write(&csharp.join("Host.csproj"), b"<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net10.0</TargetFramework>\n    <OutputType>Exe</OutputType>\n    <AssemblyName>ScalarSequenceHost</AssemblyName>\n    <ImplicitUsings>enable</ImplicitUsings>\n    <Nullable>enable</Nullable>\n    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>\n    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n    <InvariantGlobalization>true</InvariantGlobalization>\n    <Deterministic>true</Deterministic>\n  </PropertyGroup>\n  <ItemGroup>\n    <Compile Include=\"Host.cs\" />\n    <Compile Include=\"Numeric/*.cs\" />\n    <Compile Include=\"Capture/*.cs\" />\n    <Compile Include=\"Runtime/**/*.cs\" />\n    <None Include=\"source-schema.json\" CopyToOutputDirectory=\"PreserveNewest\" />\n  </ItemGroup>\n</Project>\n")?;
    support::retain(
        &out.join("preparation.original.json"),
        &json!({"status":"PUBLIC_CODEC_STATIC_LOWER_AND_EMISSION_COMPLETE_HOST_BUILD_UNRUN",
        "projects":2, "rust_modules":2, "csharp_modules":2,
        "artifacts_and_namespace_only_packaging_retained":true}),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.first().map(String::as_str) {
        Some("measure") => measure(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("prepare") => prepare(&args[1..]),
        _ => Err(std::io::Error::other("measure, verify or prepare required").into()),
    };
    if let Err(error) = result {
        eprintln!("{}", native_error_original(error.as_ref()));
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "scalar_sequence_memory requires Linux for Unix file metadata and /proc/self/status measurements."
    );
    std::process::exit(1);
}
