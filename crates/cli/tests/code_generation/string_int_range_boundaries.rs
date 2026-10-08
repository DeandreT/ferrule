use super::*;
use serde_json::Value as Json;
use std::time::{SystemTime, UNIX_EPOCH};

// Retain every generated source and failed command; no Drop cleanup.
struct Evidence(PathBuf);
impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let parent = std::env::var_os("FERRULE_CODEGEN_STRING_INT_RANGE_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        if !parent.is_absolute() {
            return Err("evidence parent must be absolute".into());
        }
        std::fs::create_dir_all(&parent)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for attempt in 0..100 {
            let path = parent.join(format!(
                "ferrule-string-int101-{label}-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    eprintln!("STRING_INT101_ORIGINALS={}", path.display());
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
    let bytes = include_bytes!("string_int_range_boundaries/cases.json");
    std::fs::write(root.0.join("cases.json"), bytes)?;
    let original = serde_json::from_slice::<Json>(bytes);
    root.record("COMPLETE-INDEPENDENT-CORPUS", &original)?;
    let corpus = original?;
    if corpus["fixtures"].as_array().map(Vec::len) != Some(7)
        || corpus["cases"].as_array().map(Vec::len) != Some(38)
        || corpus["expected_counts"] != serde_json::json!([76, 46, 30])
    {
        return Err("finite authored cohort counts changed".into());
    }
    Ok(corpus)
}

fn project(root: &Evidence, fixture: &Json) -> TestResult<Project> {
    let name = fixture["name"].as_str().ok_or("fixture name required")?;
    let source = format_json::json_schema::import_str(
        fixture["source_schema"].as_str().ok_or("source required")?,
    );
    let target = format_json::json_schema::import_str(
        fixture["target_schema"].as_str().ok_or("target required")?,
    );
    root.record(&format!("{name}-COMPLETE-SOURCE-IMPORT"), &source)?;
    root.record(&format!("{name}-COMPLETE-TARGET-IMPORT"), &target)?;
    let source = source?;
    let target = target?;
    let expected_source: SchemaNode =
        serde_json::from_value(fixture["expected_source_model"].clone())?;
    let expected_target: SchemaNode =
        serde_json::from_value(fixture["expected_target_model"].clone())?;
    if source != expected_source || target != expected_target {
        return Err("whole independent model differs".into());
    }
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["value".into()],
            frame: None,
        },
    );
    let root_scope = Scope {
        bindings: vec![Binding {
            target_field: "value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    let mut project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: root_scope,
    };
    if fixture["named"] == true {
        let extra_source: SchemaNode =
            serde_json::from_value(fixture["expected_extra_source_model"].clone())?;
        let extra_target: SchemaNode =
            serde_json::from_value(fixture["expected_extra_target_model"].clone())?;
        project.extra_sources.push(mapping::NamedSource {
            name: "catalog".into(),
            path: "unused.json".into(),
            schema: extra_source,
            options: Default::default(),
            dynamic_path: None,
        });
        project.graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["catalog".into(), "value".into()],
                frame: None,
            },
        );
        project.root.bindings[0].node = 1;
        project.extra_targets.push(mapping::NamedTarget {
            name: "audit".into(),
            path: None,
            schema: extra_target,
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "value".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        });
    }
    let original = engine::validate(&project);
    root.record(&format!("{name}-COMPLETE-PROJECT-VALIDATION"), &original)?;
    if !original.is_empty() {
        return Err("mapping must validate before emission".into());
    }
    root.record(&format!("{name}-COMPLETE-PROJECT"), &project)?;
    Ok(project)
}

fn descriptor_witness(source: &str, language: &str, project: &Project) -> TestResult<()> {
    let source_model = serde_json::to_string(&project.source)?;
    let target_model = serde_json::to_string(&project.target)?;
    let (source_key, target_key) = if language == "rust" {
        (
            "const SOURCE_JSON_SCHEMA: &str = ",
            "const TARGET_JSON_SCHEMA: &str = ",
        )
    } else {
        (
            "private const string SourceJsonSchema = ",
            "private const string TargetJsonSchema = ",
        )
    };
    for (key, model) in [(source_key, source_model), (target_key, target_model)] {
        if !source.contains(&format!("{key}{model:?};")) {
            return Err("complete embedded primary model differs".into());
        }
    }
    for (rust_key, csharp_key, schemas) in [
        (
            "EXTRA_SOURCE_JSON_SCHEMAS",
            "ExtraSourceJsonSchemas",
            project
                .extra_sources
                .iter()
                .map(|extra| &extra.schema)
                .collect::<Vec<_>>(),
        ),
        (
            "EXTRA_TARGET_JSON_SCHEMAS",
            "ExtraTargetJsonSchemas",
            project
                .extra_targets
                .iter()
                .map(|extra| &extra.schema)
                .collect::<Vec<_>>(),
        ),
    ] {
        if schemas.is_empty() {
            continue;
        }
        let models = schemas
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?;
        let literal = if language == "rust" {
            format!(
                "const {rust_key}: &[&str] = &[{}];",
                models
                    .iter()
                    .map(|model| format!("{model:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "private static readonly string[] {csharp_key} = new string[]\n    {{\n{}    }};",
                models
                    .iter()
                    .map(|model| format!("        {model:?},\n"))
                    .collect::<String>()
            )
        };
        if !source.contains(&literal) {
            return Err("complete named descriptor array differs".into());
        }
    }
    Ok(())
}

fn emit(root: &Evidence, corpus: &Json, compiled: bool) -> TestResult<(String, String)> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut rust_dependencies = String::new();
    let mut csharp_references = String::new();
    for fixture in corpus["fixtures"].as_array().ok_or("fixtures required")? {
        let name = fixture["name"].as_str().ok_or("name required")?;
        let project = project(root, fixture)?;
        let file = root.0.join(format!("{name}-project.json"));
        std::fs::write(&file, mapping::project_file::encode_pretty(&project)?)?;
        for language in ["rust", "csharp"] {
            let directory = root.0.join("original-generated").join(language).join(name);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            let original = generate_project(&file, &directory, selected);
            root.record(&format!("{language}-{name}-PUBLIC-GENERATION"), &original)?;
            let original = original?;
            let files = artifact_files(&directory)?;
            if original.output_directory != directory || original.files_written != files.len() {
                return Err("complete artifact census differs".into());
            }
            let mapping_file = if language == "rust" {
                "src/lib.rs"
            } else {
                "GeneratedMapping.cs"
            };
            let text = std::fs::read_to_string(directory.join(mapping_file))?;
            descriptor_witness(&text, language, &project)?;
            if !compiled {
                continue;
            }
            let copy = root.0.join(language).join(name);
            std::fs::create_dir_all(&copy)?;
            for (relative, bytes) in files {
                let file = copy.join(relative);
                std::fs::create_dir_all(file.parent().ok_or("artifact parent required")?)?;
                std::fs::write(file, bytes)?;
            }
            let alias = format!("fixture_{}", name.to_ascii_lowercase());
            if language == "rust" {
                let path = copy.join("Cargo.toml");
                let before = std::fs::read_to_string(&path)?;
                let needle = "name = \"ferrule-generated-mapping\"";
                if before.matches(needle).count() != 1 {
                    return Err("singular package name required".into());
                }
                let package = format!("ferrule-string-int101-{}", name.to_ascii_lowercase());
                let after = before.replacen(needle, &format!("name = {package:?}"), 1);
                root.record(
                    &format!("{name}-RUST-COPY-MANIFEST-PRE-POST"),
                    &(&before, &after),
                )?;
                std::fs::write(path, after)?;
                rust_dependencies.push_str(&format!(
                    "{alias} = {{ package = {package:?}, path = {:?} }}\n",
                    format!("../{name}")
                ));
            } else {
                let path = copy.join("Ferrule.Generated.csproj");
                let before = std::fs::read_to_string(&path)?;
                let needle = "<AssemblyName>Ferrule.Generated</AssemblyName>";
                if before.matches(needle).count() != 1 {
                    return Err("singular assembly name required".into());
                }
                let after = before.replacen(
                    needle,
                    &format!("<AssemblyName>Ferrule.StringInt101.{name}</AssemblyName>"),
                    1,
                );
                root.record(
                    &format!("{name}-CSHARP-COPY-MANIFEST-PRE-POST"),
                    &(&before, &after),
                )?;
                std::fs::write(path, after)?;
                csharp_references.push_str(&format!("<ProjectReference Include=\"../{name}/Ferrule.Generated.csproj\" Aliases=\"{alias}\" />\n"));
            }
        }
    }
    Ok((rust_dependencies, csharp_references))
}

#[test]
fn string_int_ranges_emit_complete_primary_and_named_metadata() -> TestResult<()> {
    let root = Evidence::new("emission")?;
    let corpus = corpus(&root)?;
    let original = emit(&root, &corpus, false);
    root.record("COMPLETE-EMISSION-RESULT", &original)?;
    original?;
    Ok(())
}

#[test]
#[ignore = "root-coordinated offline compiled hosts require a shared Cargo target and disk reserve"]
fn string_int_ranges_compiled_rust_and_csharp_complete_text_and_bytes() -> TestResult<()> {
    let root = Evidence::new("compiled")?;
    let target = PathBuf::from(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .ok_or("set coordinated shared target")?,
    );
    if !target.is_absolute() {
        return Err("shared Cargo target must be absolute".into());
    }
    let corpus = corpus(&root)?;
    let (dependencies, references) = emit(&root, &corpus, true)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let rust = root.0.join("rust/Host");
    std::fs::create_dir_all(rust.join("src"))?;
    std::fs::write(
        rust.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ferrule-string-int101-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\ncodegen-runtime = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n{dependencies}\n[workspace]\n",
            runtime
        ),
    )?;
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("string_int_range_boundaries/Host.rs.txt"),
    )?;
    let csharp = root.0.join("csharp/Host");
    std::fs::create_dir_all(&csharp)?;
    std::fs::write(
        csharp.join("Host.csproj"),
        format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup>{references}</ItemGroup></Project>\n"
        ),
    )?;
    std::fs::write(
        csharp.join("Program.cs"),
        include_str!("string_int_range_boundaries/Host.cs.txt"),
    )?;
    std::fs::write(
        root.0.join("csharp/NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    // Bind complete generated/host source bytes before compiler-created bin/obj files exist.
    let mut inputs = vec![(
        root.0.join("cases.json"),
        std::fs::read(root.0.join("cases.json"))?,
    )];
    for directory in [root.0.join("rust"), root.0.join("csharp")] {
        inputs.extend(
            artifact_files(&directory)?
                .into_iter()
                .map(|(name, bytes)| (directory.join(name), bytes)),
        );
    }
    let original = (|| -> TestResult<()> {
        let mut cargo = Command::new("cargo");
        cargo
            .args(["build", "--offline", "--jobs", "1"])
            .current_dir(&rust)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Dwarnings");
        let rust_build = root.command("RUST-BUILD", &mut cargo);
        let mut dotnet = dotnet_command(&root.0.join("csharp"));
        dotnet
            .args([
                "msbuild",
                "Host/Host.csproj",
                "-t:Restore;Build",
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
            return Err("compiled hosts failed; originals retained".into());
        }
        let binary = target.join("debug").join(format!(
            "ferrule-string-int101-host{}",
            std::env::consts::EXE_SUFFIX
        ));
        let mut libraries = vec![(binary.clone(), std::fs::read(&binary)?)];
        for (name, bytes) in artifact_files(&csharp.join("bin/Debug/net10.0"))? {
            libraries.push((csharp.join("bin/Debug/net10.0").join(name), bytes));
        }
        let mut rust_run = Command::new(&binary);
        rust_run.arg(&root.0).current_dir(&rust);
        let rust_result = root.command("RUST-HOST", &mut rust_run);
        let mut csharp_run = dotnet_command(&root.0.join("csharp"));
        csharp_run
            .arg(csharp.join("bin/Debug/net10.0/Host.dll"))
            .arg(&root.0)
            .current_dir(root.0.join("csharp"));
        let csharp_result = root.command("CSHARP-HOST", &mut csharp_run);
        root.record(
            "BOTH-COMPLETE-HOST-RESULTS",
            &(&rust_result, &csharp_result),
        )?;
        let originals = libraries
            .iter()
            .map(|(path, _)| std::fs::read(path))
            .collect::<Vec<_>>();
        root.record("COMPLETE-LIBRARY-GUARD-AFTER", &originals)?;
        if !originals
            .iter()
            .zip(&libraries)
            .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted))
        {
            return Err("compiled library bytes changed".into());
        }
        if !rust_result?.status.success() || !csharp_result?.status.success() {
            return Err("compiled comparisons failed; originals retained".into());
        }
        for language in ["RUST", "CSHARP"] {
            let bytes = std::fs::read(root.0.join(format!("{language}-COMPLETE-RESULT.json")))?;
            root.record(&format!("{language}-COMPLETE-RESULT-BYTES"), &bytes)?;
            let actual: Json = serde_json::from_slice(&bytes)?;
            if actual["counts"] != corpus["expected_counts"]
                || actual["failures"] != serde_json::json!([])
            {
                return Err("complete counts or outcome vector differs".into());
            }
        }
        Ok(())
    })();
    root.record("COMPLETE-COMPILED-ORIGINAL-RESULT", &original)?;
    let after = inputs
        .iter()
        .map(|(path, _)| std::fs::read(path))
        .collect::<Vec<_>>();
    root.record("COMPLETE-GENERATED-HOST-INPUT-GUARD-AFTER", &after)?;
    let stable = after
        .iter()
        .zip(&inputs)
        .all(|(actual, (_, wanted))| matches!(actual, Ok(actual) if actual == wanted));
    original?;
    if !stable {
        return Err("complete generated or host input bytes changed".into());
    }
    Ok(())
}
