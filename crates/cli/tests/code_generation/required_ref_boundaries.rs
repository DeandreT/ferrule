use super::*;
use mapping::ScopeConstruction;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

include!("required_ref_boundaries/tree.rs.txt");

// This finite cohort is retained after success and every failed attempt.
struct Evidence(PathBuf);

impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_CODEGEN_REQUIRED_REF_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("required-ref evidence parent must be absolute".into());
        }
        std::fs::create_dir_all(&parent)?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for attempt in 0..100 {
            let path = parent.join(format!(
                "ferrule-required-ref169-{label}-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    eprintln!("REQUIRED_REF169_ORIGINALS={}", path.display());
                    return Ok(Self(path));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("fresh evidence directory unavailable".into())
    }

    fn record(&self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}-ORIGINAL.txt")),
            format!("{original:#?}\n"),
        )?;
        Ok(())
    }

    fn command(&self, label: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{label}-COMMAND"), command)?;
        let original = command.isolated_output();
        self.record(&format!("{label}-COMMAND-RESULT"), &original)?;
        let output = original?;
        std::fs::write(self.0.join(format!("{label}-stdout.bin")), &output.stdout)?;
        std::fs::write(self.0.join(format!("{label}-stderr.bin")), &output.stderr)?;
        Ok(output)
    }
}

fn corpus(root: &Evidence) -> TestResult<Json> {
    let bytes = include_bytes!("required_ref_boundaries/cases.json");
    std::fs::write(root.0.join("cases.json"), bytes)?;
    let corpus: Json = serde_json::from_slice(bytes)?;
    if corpus["profiles"].as_array().map(Vec::len) != Some(9)
        || corpus["fixtures"].as_array().map(Vec::len) != Some(16)
        || corpus["cases"].as_array().map(Vec::len) != Some(66)
    {
        return Err("finite authored cohort count changed".into());
    }
    Ok(corpus)
}

fn expected_schema(profile: &Json) -> TestResult<SchemaNode> {
    let model = &profile["expected_model"];
    let mut note = string("note");
    note.nullable = true;
    let mut schema = SchemaNode::group("Envelope", vec![int("id"), note, string("optional")]);
    if !model["dynamic"].is_null() {
        schema = schema
            .with_dynamic_fields(
                string("*")
                    .json_any()
                    .ok_or("arbitrary dynamic string required")?,
            )
            .ok_or("dynamic object schema required")?;
    }
    let required = model["required"]
        .as_array()
        .ok_or("required order oracle required")?
        .iter()
        .map(|name| {
            name.as_str()
                .map(str::to_owned)
                .ok_or("required name required")
        })
        .collect::<Result<Vec<_>, _>>()?;
    schema = schema
        .with_required_fields(required)
        .ok_or("independent required schema invalid")?;
    schema.container_nullable = model["container_nullable"]
        .as_bool()
        .ok_or("nullable oracle required")?;
    Ok(schema)
}

fn expected_export(profile: &Json) -> Json {
    let model = &profile["expected_model"];
    let object = serde_json::json!({
        "type":"object", "required":model["required"],
        "properties":{"id":{"type":"integer"},"note":{"type":["string","null"]},"optional":{"type":"string"}},
        "additionalProperties":if model["dynamic"].is_null() { serde_json::json!(false) } else { serde_json::json!({}) },
    });
    if model["container_nullable"] == true {
        serde_json::json!({"title":"Envelope","anyOf":[object,{"type":"null"}]})
    } else {
        let mut map = serde_json::Map::new();
        map.insert("title".into(), serde_json::json!("Envelope"));
        map.extend(
            object
                .as_object()
                .expect("independent object literal")
                .clone(),
        );
        Json::Object(map)
    }
}

fn profiles(root: &Evidence, corpus: &Json) -> TestResult<Vec<SchemaNode>> {
    let profiles = corpus["profiles"].as_array().ok_or("profiles required")?;
    // Materialize all complete physical originals before the first importer call.
    for (index, profile) in profiles.iter().enumerate() {
        let directory = root.0.join(format!("schema-profile-{index}"));
        std::fs::create_dir(&directory)?;
        for (name, body) in profile["physical_files"]
            .as_object()
            .ok_or("physical files required")?
        {
            std::fs::write(
                directory.join(name),
                body.as_str().ok_or("physical body required")?,
            )?;
        }
        root.record(&format!("profile-{index}-COMPLETE-INDEPENDENT"), profile)?;
    }
    let originals = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let directory = root.0.join(format!("schema-profile-{index}"));
            format_json::json_schema::import_with_root(
                &directory.join(profile["entry_file"].as_str().expect("entry literal")),
                &directory,
            )
        })
        .collect::<Vec<_>>();
    root.record("ALL9-COMPLETE-PUBLIC-IMPORTS", &originals)?;
    let mut schemas = Vec::new();
    for (index, (original, profile)) in originals.into_iter().zip(profiles).enumerate() {
        let wanted = expected_schema(profile)?;
        root.record(&format!("profile-{index}-COMPLETE-MODEL-ORACLE"), &wanted)?;
        let schema = original?;
        if schema != wanted {
            return Err(format!("profile {index} whole imported schema differs").into());
        }
        let exported = format_json::json_schema::export(&schema);
        root.record(&format!("profile-{index}-EXPORT"), &exported)?;
        let exported = exported?;
        std::fs::write(
            root.0.join(format!("{index}-exported.schema.json")),
            &exported,
        )?;
        let actual: Json = serde_json::from_str(&exported)?;
        let oracle = expected_export(profile);
        root.record(&format!("profile-{index}-EXPORT-ORACLE"), &oracle)?;
        if actual != oracle {
            return Err(format!("profile {index} complete exported schema differs").into());
        }
        let reimported = format_json::json_schema::import_str(&exported);
        root.record(&format!("profile-{index}-REIMPORT"), &reimported)?;
        if reimported? != wanted {
            return Err(format!("profile {index} complete reimport differs").into());
        }
        // Public runtime controls use the complete independently constructed IR
        // schema, after whole equality. No actual result becomes expected truth.
        std::fs::write(
            root.0.join(format!("{index}-schema.json")),
            serde_json::to_vec(&wanted)?,
        )?;
        schemas.push(schema);
    }
    Ok(schemas)
}

fn project(schema: &SchemaNode, fixture: &Json, corpus: &Json) -> TestResult<Project> {
    let mut candidate = Project {
        source: schema.clone(),
        target: schema.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    if fixture["construction"] == "copy-current-source" {
        candidate.root.construction = ScopeConstruction::CopyCurrentSource;
    } else {
        let tree = match fixture.get("constant_tree") {
            Some(tree) => tree.clone(),
            None => corpus["cases"]
                .as_array()
                .ok_or("cases required")?
                .iter()
                .find(|case| {
                    case["fixture"] == fixture["fixture"]
                        && case["expected"].get("output_tree").is_some()
                })
                .ok_or("constant target oracle required")?["expected"]["output_tree"]
                .clone(),
        };
        let Instance::Group(fields) = manual_tree(&tree)? else {
            return Err("constant target must be a group".into());
        };
        for (index, (name, instance)) in fields.iter().enumerate() {
            let Instance::Scalar(value) = instance else {
                return Err("small constant must be scalar".into());
            };
            let node = u32::try_from(index)?;
            candidate.graph.nodes.insert(
                node,
                Node::Const {
                    value: value.clone(),
                },
            );
            candidate.root.bindings.push(Binding {
                target_field: name.clone(),
                node,
            });
        }
    }
    if candidate.source != *schema || candidate.target != *schema {
        return Err("source/target required schemas were not retained whole".into());
    }
    Ok(candidate)
}

fn native_error(error: &format_json::JsonFormatError) -> Json {
    use format_json::JsonFormatError;
    match error {
        JsonFormatError::MissingRequiredProperty { object, property } => {
            serde_json::json!({"category":"MissingRequiredProperty","object":object,"property":property})
        }
        JsonFormatError::UndeclaredProperty { object, property } => {
            serde_json::json!({"category":"UndeclaredProperty","object":object,"property":property})
        }
        JsonFormatError::Shape {
            name,
            expected,
            got,
        } => serde_json::json!({"category":"Shape","name":name,"expected":expected,"got":got}),
        JsonFormatError::Json(error) => {
            serde_json::json!({"category":"Json","class":format!("{:?}",error.classify()),"line":error.line(),"column":error.column(),"message":error.to_string()})
        }
        _ => {
            serde_json::json!({"category":"UNEXPECTED","complete":format!("{error:#?}"),"display":error.to_string()})
        }
    }
}

fn compare_native(root: &Evidence, corpus: &Json, schemas: &[SchemaNode]) -> TestResult<()> {
    let mut failures = Vec::new();
    let mut native_counts = [0_usize; 4];
    let fixtures = corpus["fixtures"].as_array().ok_or("fixtures required")?;
    for case in corpus["cases"].as_array().ok_or("cases required")? {
        let id = case["id"].as_str().ok_or("case id required")?;
        let Some(input) = case["input"].as_str() else {
            continue;
        };
        let schema = &schemas[case["profile"].as_u64().ok_or("profile required")? as usize];
        let original = format_json::from_str(input, schema);
        root.record(&format!("NATIVE-{id}-READ"), &original)?;
        native_counts[0] += 1;
        let expected = &case["expected"];
        if let Some(tree) = expected.get("input_tree") {
            let wanted = manual_tree(tree)?;
            match original {
                Ok(actual) => {
                    if complete_tree(&actual)? != *tree || actual != wanted {
                        failures.push(format!("{id}: complete native reader tree differs"));
                    }
                    if let Some(expected_text) = expected["native_roundtrip_utf8"].as_str() {
                        let rendered = format_json::to_string(schema, &wanted);
                        root.record(&format!("NATIVE-{id}-DIRECT-ROUNDTRIP"), &rendered)?;
                        native_counts[1] += 1;
                        if !matches!(&rendered, Ok(text) if text == expected_text) {
                            failures.push(format!(
                                "{id}: complete native reader/writer identity differs"
                            ));
                        }
                    }
                    let fixture = fixtures
                        .iter()
                        .find(|fixture| fixture["fixture"] == case["fixture"])
                        .ok_or("fixture missing")?;
                    let mapping = project(schema, fixture, corpus)?;
                    let mapped = engine::run(&mapping, &wanted);
                    root.record(&format!("NATIVE-{id}-ENGINE"), &mapped)?;
                    native_counts[2] += 1;
                    match mapped {
                        Ok(mapped) => {
                            if complete_tree(&mapped)? != expected["output_tree"] {
                                failures.push(format!("{id}: complete native mapped tree differs"));
                            }
                            let rendered = format_json::to_string(&mapping.target, &mapped);
                            root.record(&format!("NATIVE-{id}-MAPPED-WRITER"), &rendered)?;
                            native_counts[3] += 1;
                            if let Some(wanted) = expected["output_utf8"].as_str() {
                                if !matches!(&rendered, Ok(text) if text == wanted) {
                                    failures
                                        .push(format!("{id}: complete mapped native text differs"));
                                }
                            } else if !matches!(&rendered, Err(error) if native_error(error) == expected["native_error"])
                            {
                                failures.push(format!(
                                    "{id}: complete native normalized-output refusal differs"
                                ));
                            }
                        }
                        Err(_) => failures.push(format!("{id}: ordinary native mapping failed")),
                    }
                }
                Err(_) => failures.push(format!("{id}: valid native input refused")),
            }
        } else if !matches!(&original, Err(error) if native_error(error) == expected["native_error"])
        {
            failures.push(format!("{id}: complete native input refusal differs"));
        }
    }
    for case in corpus["typed_writer_cases"]
        .as_array()
        .ok_or("writers required")?
    {
        let id = case["id"].as_str().ok_or("writer id required")?;
        let schema = &schemas[case["profile"].as_u64().ok_or("writer profile required")? as usize];
        let value = manual_tree(&case["input_tree"])?;
        let original = format_json::to_string(schema, &value);
        root.record(&format!("NATIVE-{id}-TYPED-WRITER"), &original)?;
        native_counts[3] += 1;
        if let Some(wanted) = case["expected"]["output_utf8"].as_str() {
            if !matches!(&original, Ok(text) if text == wanted) {
                failures.push(format!("{id}: complete native typed output differs"));
            }
        } else if !matches!(&original, Err(error) if native_error(error) == case["expected"]["native_error"])
        {
            failures.push(format!(
                "{id}: complete native typed output refusal differs"
            ));
        }
    }
    let original = serde_json::json!({"counts":native_counts,"failures":failures,"native_UTF8_byte_public_route":"unavailable; formatter public route is from_str/to_string"});
    std::fs::write(
        root.0.join("NATIVE-COMPLETE-RESULT.json"),
        serde_json::to_vec_pretty(&original)?,
    )?;
    if native_counts != [65, 23, 30, 38] || !failures.is_empty() {
        return Err(
            "complete native required-reference comparison failed; originals retained".into(),
        );
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnixIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    links: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

#[cfg(unix)]
fn unix_identity(metadata: &std::fs::Metadata) -> UnixIdentity {
    use std::os::unix::fs::MetadataExt;
    UnixIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        links: metadata.nlink(),
        mtime: metadata.mtime(),
        mtime_ns: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_ns: metadata.ctime_nsec(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    path: PathBuf,
    bytes: usize,
    sha256: String,
    #[cfg(unix)]
    unix_metadata: UnixIdentity,
}

fn identity(path: &Path) -> TestResult<Identity> {
    let canonical = std::fs::canonicalize(path)?;
    let before = std::fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err("guarded input must be a regular non-link file".into());
    }
    let bytes = std::fs::read(path)?;
    let after = std::fs::symlink_metadata(path)?;
    if !after.file_type().is_file()
        || before.len() != bytes.len() as u64
        || after.len() != before.len()
        || std::fs::canonicalize(path)? != canonical
    {
        return Err("guarded original changed while retaining its whole body".into());
    }
    #[cfg(unix)]
    if unix_identity(&before) != unix_identity(&after) {
        return Err("guarded metadata changed while retaining original".into());
    }
    Ok(Identity {
        path: canonical,
        bytes: bytes.len(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        #[cfg(unix)]
        unix_metadata: unix_identity(&before),
    })
}

fn collect(root: &Path, paths: &mut Vec<PathBuf>) -> TestResult<()> {
    let mut entries = std::fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for path in entries {
        if matches!(
            path.file_name().and_then(OsStr::to_str),
            Some("target" | "bin" | "obj" | ".git" | ".tmp" | ".nuget-packages" | ".dotnet-home")
        ) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err("source guard refuses an unresolved symbolic link".into());
        }
        if metadata.is_dir() {
            collect(&path, paths)?;
        } else {
            paths.push(path);
        }
    }
    Ok(())
}

fn source_guard(root: &Evidence) -> TestResult<Vec<Identity>> {
    let mut paths = Vec::new();
    collect(&root.0, &mut paths)?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "cli",
        "codegen",
        "codegen-runtime",
        "codegen-schema",
        "codegen-rust",
        "codegen-csharp",
        "functions",
        "format-json",
        "ir",
        "mapping",
        "json-pattern",
    ] {
        collect(&workspace.join("crates").join(name), &mut paths)?;
    }
    collect(
        &workspace.join("runtime/csharp/Ferrule.Runtime"),
        &mut paths,
    )?;
    paths.extend([
        workspace.join("Cargo.toml"),
        workspace.join("Cargo.lock"),
        PathBuf::from(env!("CARGO_BIN_EXE_ferrule")),
    ]);
    paths.sort();
    paths.dedup();
    let originals = paths
        .iter()
        .map(|path| identity(path))
        .collect::<TestResult<Vec<_>>>()?;
    root.record("COMPLETE-SOURCE-GUARD-BEFORE", &originals)?;
    Ok(originals)
}

fn guard(root: &Evidence, before: &[Identity], label: &str) -> TestResult<()> {
    let originals = before
        .iter()
        .map(|wanted| identity(&wanted.path))
        .collect::<Vec<_>>();
    root.record(label, &originals)?;
    if !originals
        .iter()
        .zip(before)
        .all(|(actual, wanted)| matches!(actual, Ok(actual) if actual == wanted))
    {
        return Err("complete guarded input/library bytes changed; originals retained".into());
    }
    Ok(())
}

#[test]
fn required_ref_all9_native_schemas_and_complete_presence_outcomes() -> TestResult<()> {
    let root = Evidence::new("native")?;
    let corpus = corpus(&root)?;
    let schemas = profiles(&root, &corpus)?;
    let before = source_guard(&root)?;
    let original = compare_native(&root, &corpus, &schemas);
    root.record("COMPLETE-NATIVE-ORIGINAL-RESULT", &original)?;
    let after = guard(&root, &before, "COMPLETE-SOURCE-GUARD-AFTER");
    root.record("COMPLETE-SOURCE-GUARD-ORIGINAL-RESULT", &after)?;
    original?;
    after?;
    Ok(())
}

#[test]
#[ignore = "root-coordinated offline required-reference compiled hosts need bound shared target and disk reserve"]
fn required_ref_compiled_rust_and_csharp_all_complete_text_and_bytes() -> TestResult<()> {
    let root = Evidence::new("compiled")?;
    let target = PathBuf::from(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .ok_or("set coordinated FERRULE_CODEGEN_HOST_TARGET_DIR")?,
    );
    if !target.is_absolute() {
        return Err("shared generated-host target must be absolute".into());
    }
    let corpus = corpus(&root)?;
    let schemas = profiles(&root, &corpus)?;
    let native = compare_native(&root, &corpus, &schemas);
    root.record("COMPLETE-NATIVE-DEPENDENCY-ORIGINAL", &native)?;
    native?;
    for path in [
        "original-generated",
        "original-generated/rust",
        "original-generated/csharp",
        "rust",
        "csharp",
    ] {
        std::fs::create_dir(root.0.join(path))?;
    }
    let runtime =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"))?;
    let fixtures = corpus["fixtures"].as_array().ok_or("fixtures required")?;
    let mut dependencies = String::new();
    let mut csharp_projects = String::new();
    for fixture in fixtures {
        let name = fixture["fixture"].as_str().ok_or("fixture name required")?;
        let schema = &schemas[fixture["profile"]
            .as_u64()
            .ok_or("fixture profile required")? as usize];
        let project = project(schema, fixture, &corpus)?;
        let validation = engine::validate(&project);
        root.record(&format!("{name}-PROJECT-VALIDATION"), &validation)?;
        if !validation.is_empty() {
            return Err("small project must validate before emission".into());
        }
        let project_path = root.0.join(format!("{name}-project.json"));
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&project)?,
        )?;
        root.record(&format!("{name}-COMPLETE-PROJECT"), &project)?;
        for language in ["rust", "csharp"] {
            let original = root.0.join("original-generated").join(language).join(name);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            let outcome = generate_project(&project_path, &original, selected);
            root.record(&format!("{language}-{name}-PUBLIC-GENERATION"), &outcome)?;
            let outcome = outcome?;
            let artifacts = artifact_files(&original)?;
            root.record(&format!("{language}-{name}-COMPLETE-ARTIFACTS"), &artifacts)?;
            if outcome.output_directory != original || outcome.files_written != artifacts.len() {
                return Err("complete ordinary artifact census differs".into());
            }
            let copy = root.0.join(language).join(name);
            std::fs::create_dir(&copy)?;
            for (relative, bytes) in artifacts {
                let path = copy.join(relative);
                std::fs::create_dir_all(path.parent().ok_or("artifact parent required")?)?;
                std::fs::write(path, bytes)?;
            }
            if language == "rust" {
                let manifest = copy.join("Cargo.toml");
                let before = std::fs::read_to_string(&manifest)?;
                let literal = "name = \"ferrule-generated-mapping\"";
                if before.matches(literal).count() != 1 {
                    return Err("package rename fixture must be singular".into());
                }
                let package = format!("ferrule-required-ref169-{}", name.to_ascii_lowercase());
                std::fs::write(
                    &manifest,
                    before.replacen(literal, &format!("name = {package:?}"), 1),
                )?;
                dependencies.push_str(&format!(
                    "fixture_{} = {{ package = {package:?}, path = {:?} }}\n",
                    name.to_ascii_lowercase(),
                    format!("../{name}")
                ));
            } else {
                csharp_projects.push_str(&format!(
                    "    <Projects Include=\"{name}/Ferrule.Generated.csproj\" />\n"
                ));
            }
        }
    }
    let rust_host = root.0.join("rust/Host");
    std::fs::create_dir(&rust_host)?;
    std::fs::create_dir(rust_host.join("src"))?;
    std::fs::write(
        rust_host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ferrule-required-ref169-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncodegen-runtime = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n{dependencies}\n[workspace]\n",
            runtime.to_str().ok_or("runtime path must be UTF-8")?
        ),
    )?;
    std::fs::write(
        rust_host.join("src/main.rs"),
        include_str!("required_ref_boundaries/Host.rs.txt"),
    )?;
    std::fs::write(
        rust_host.join("src/tree.rs"),
        include_str!("required_ref_boundaries/tree.rs.txt"),
    )?;
    let csharp_host = root.0.join("csharp/Host");
    std::fs::create_dir(&csharp_host)?;
    std::fs::write(
        csharp_host.join("Host.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup></Project>
"#,
    )?;
    std::fs::write(
        csharp_host.join("Program.cs"),
        include_str!("required_ref_boundaries/Host.cs.txt"),
    )?;
    csharp_projects.push_str("    <Projects Include=\"Host/Host.csproj\" />\n");
    std::fs::write(
        root.0.join("csharp/Cohort.proj"),
        format!(
            "<Project>\n<ItemGroup>\n{csharp_projects}</ItemGroup>\n<Target Name=\"Build\"><MSBuild Projects=\"@(Projects)\" Targets=\"Restore;Build\" BuildInParallel=\"false\" /></Target>\n</Project>\n"
        ),
    )?;
    std::fs::write(
        root.0.join("csharp/NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    let before = source_guard(&root)?;
    let original = (|| -> TestResult<()> {
        let mut cargo = Command::new("cargo");
        cargo
            .args(["build", "--offline", "--jobs", "1"])
            .current_dir(&rust_host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Dwarnings");
        let rust_build = root.command("RUST-BUILD", &mut cargo);
        let mut dotnet = dotnet_command(&root.0.join("csharp"));
        dotnet
            .args([
                "msbuild",
                "Cohort.proj",
                "-t:Build",
                "-nologo",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:AutomaticallyUseReferenceAssemblyPackages=false",
            ])
            .current_dir(root.0.join("csharp"))
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1");
        let csharp_build = root.command("CSHARP-BUILD", &mut dotnet);
        root.record("BOTH-COMPLETE-BUILD-RESULTS", &(&rust_build, &csharp_build))?;
        if !rust_build?.status.success() || !csharp_build?.status.success() {
            return Err("compiled hosts failed with warnings denied; originals retained".into());
        }
        let rust_binary = target.join("debug").join(format!(
            "ferrule-required-ref169-host{}",
            std::env::consts::EXE_SUFFIX
        ));
        let mut libraries = vec![
            identity(&rust_binary)?,
            identity(&csharp_host.join("bin/Debug/net10.0/Host.dll"))?,
        ];
        for fixture in fixtures {
            libraries.push(identity(
                &root
                    .0
                    .join("csharp")
                    .join(fixture["fixture"].as_str().ok_or("fixture required")?)
                    .join("bin/Debug/net10.0/Ferrule.Generated.dll"),
            )?);
        }
        root.record("COMPLETE18-LIBRARY-GUARD-BEFORE", &libraries)?;
        let mut rust = Command::new(&rust_binary);
        rust.arg(&root.0).current_dir(&rust_host);
        let rust_result = root.command("RUST-HOST", &mut rust);
        let mut csharp = dotnet_command(&root.0.join("csharp"));
        csharp
            .arg(csharp_host.join("bin/Debug/net10.0/Host.dll"))
            .arg(&root.0)
            .current_dir(root.0.join("csharp"));
        let csharp_result = root.command("CSHARP-HOST", &mut csharp);
        let after = guard(&root, &libraries, "COMPLETE18-LIBRARY-GUARD-AFTER");
        root.record(
            "BOTH-COMPLETE-HOST-RESULTS",
            &(&rust_result, &csharp_result, &after),
        )?;
        after?;
        if !rust_result?.status.success() || !csharp_result?.status.success() {
            return Err(
                "compiled required-reference comparison failed; full originals retained".into(),
            );
        }
        for language in ["RUST", "CSHARP"] {
            let bytes = std::fs::read(root.0.join(format!("{language}-COMPLETE-RESULT.json")))?;
            root.record(&format!("{language}-COMPLETE-RESULT-BYTES"), &bytes)?;
            let result: Json = serde_json::from_slice(&bytes)?;
            if result["counts"] != serde_json::json!([131, 117, 30, 16, 3])
                || result["failures"] != serde_json::json!([])
            {
                return Err(
                    "all finite public/typed counts and complete outcomes must match".into(),
                );
            }
        }
        Ok(())
    })();
    root.record("COMPLETE-COMPILED-ORIGINAL-RESULT", &original)?;
    let after = guard(&root, &before, "COMPLETE-SOURCE-GUARD-AFTER");
    root.record("COMPLETE-SOURCE-GUARD-ORIGINAL-RESULT", &after)?;
    original?;
    after?;
    Ok(())
}
