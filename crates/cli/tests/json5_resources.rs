//! Issue145: explicit ignored physical JSON5 public boundaries; no default large test.
#![cfg(all(feature = "codegen-tests", target_os = "linux"))]

use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
#[path = "json5_resources/corpus.rs"]
mod corpus;
#[path = "json5_resources/support.rs"]
mod support;

fn write_artifacts(root: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    std::fs::create_dir(root)?;
    for file in artifacts.files() {
        let path = root.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().ok_or("artifact parent required")?)?;
        std::fs::write(path, &file.contents)?;
    }
    Ok(())
}

fn prepare_libraries(root: &Path) -> TestResult<Vec<(String, String, PathBuf)>> {
    let runtime =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"))?;
    let mut hosts = Vec::new();
    for profile in corpus::PROFILES {
        let program = corpus::program(profile)?;
        let valid = codegen::validate_program(&program);
        support::record(
            root,
            &format!("{profile}-PROGRAM-AND-VALIDATION"),
            &(&program, &valid),
        )?;
        valid?;
        for language in ["rust", "csharp"] {
            let directory = root.join(format!("{language}-{profile}"));
            std::fs::create_dir(&directory)?;
            let library = directory.join("library");
            let host = directory.join("host");
            std::fs::create_dir(&host)?;
            let artifacts = if language == "rust" {
                let options = codegen_rust::Options {
                    package_name: format!("json5-resource-{profile}"),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        runtime.display().to_string(),
                    ),
                };
                let result = codegen_rust::emit_with_json5(&program, &options);
                support::record(
                    root,
                    &format!("{language}-{profile}-EMISSION-ORIGINAL"),
                    &result,
                )?;
                result?
            } else {
                let result = codegen_csharp::emit_with_json5(&program);
                support::record(
                    root,
                    &format!("{language}-{profile}-EMISSION-ORIGINAL"),
                    &result,
                )?;
                result?
            };
            write_artifacts(&library, &artifacts)?;
            if language == "rust" {
                std::fs::create_dir(host.join("src"))?;
                std::fs::write(
                    host.join("src/main.rs"),
                    include_bytes!("fixtures/json5_resources_rust.rs.txt"),
                )?;
                std::fs::write(
                    host.join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"json5-resource-host-{profile}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\njson5_resource_map = {{ package = \"json5-resource-{profile}\", path = \"../library\" }}\ncodegen-runtime = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\",\"float_roundtrip\"] }}\nsha2 = \"0.10.9\"\n[workspace]\n",
                        runtime.to_str().ok_or("UTF8 runtime path required")?
                    ),
                )?;
            } else {
                std::fs::write(
                    host.join("Program.cs"),
                    include_bytes!("fixtures/json5_resources_csharp.cs.txt"),
                )?;
                std::fs::write(
                    host.join("Host.csproj"),
                    r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup>
  <ItemGroup><ProjectReference Include="../library/Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
                )?;
                std::fs::write(
                    directory.join("NuGet.Config"),
                    "<configuration><packageSources><clear /></packageSources></configuration>\n",
                )?;
            }
            hosts.push((language.into(), profile.into(), host));
        }
    }
    Ok(hosts)
}

fn rust_executable(output: &[u8], name: &str, target: &Path) -> TestResult<PathBuf> {
    let mut paths = Vec::new();
    for line in std::str::from_utf8(output)?.lines() {
        let record: Json = serde_json::from_str(line)?;
        if record["reason"] == "compiler-artifact"
            && record["target"]["name"] == name
            && record["target"]["kind"] == json!(["bin"])
            && record["profile"]["test"] == false
            && let Some(path) = record["executable"].as_str()
        {
            paths.push(std::fs::canonicalize(path)?);
        }
    }
    paths.sort();
    paths.dedup();
    if paths.len() != 1 || !paths[0].starts_with(target) {
        return Err("exact actual normal Cargo binary artifact required".into());
    }
    Ok(paths.remove(0))
}

#[test]
#[ignore = "48 physical JSON5 calls require authentic global184 receipt, explicit serial root opt-in, tools, headroom and deadline"]
fn json5_original_normalized_and_output_physical_boundaries_all48() -> TestResult<()> {
    if std::env::var("FERRULE_JSON5_RESOURCE_OPT_IN").as_deref() != Ok("1") {
        return Err("explicit root resource opt-in required".into());
    }
    let parent = support::required_path("FERRULE_JSON5_RESOURCE_EVIDENCE_DIR")?;
    if !parent.is_dir() {
        return Err("existing confined evidence parent required".into());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = parent.join(format!("json5-resource145-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root)?;
    println!(
        "retained complete original resource cohort: {}",
        root.display()
    );
    // Authenticate both languages and all276 full original call records before
    // generating any physical input or building any resource library/host.
    let mut fixed = support::small_barrier(&root)?;
    fixed.extend(support::source_binding(&root)?);
    let target = support::required_path("FERRULE_CODEGEN_HOST_TARGET_DIR")?;
    if !target.is_dir() {
        return Err("existing coordinated shared Cargo target required".into());
    }
    let cargo = support::required_path("FERRULE_CODEGEN_CARGO")?;
    let rustc = support::required_path("FERRULE_CODEGEN_RUSTC")?;
    let dotnet = support::required_path("FERRULE_CODEGEN_DOTNET")?;
    let time = support::required_path("FERRULE_CODEGEN_GNU_TIME")?;
    let timeout = support::required_path("FERRULE_CODEGEN_TIMEOUT")?;
    let df = support::required_path("FERRULE_CODEGEN_DF")?;
    for tool in [&cargo, &rustc, &dotnet, &time, &timeout, &df] {
        let actual = support::identity(tool)?;
        if !fixed.iter().any(|value| value == &actual) {
            return Err("selected actual tool missing from root source/toolchain binding".into());
        }
    }
    for directory in [
        ".dotnet-home",
        ".nuget-packages",
        ".tmp",
        "corpus",
        "observations",
    ] {
        std::fs::create_dir(root.join(directory))?;
    }
    let runner = support::Runner::new(root.clone(), time.clone(), timeout.clone())?;
    let mut dynamic = Vec::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> TestResult<()> {
        for (label, tool, args) in [
            ("CARGO-VERSION", &cargo, vec!["--version".into()]),
            ("RUSTC-VERSION", &rustc, vec!["--version".into()]),
            ("DOTNET-VERSION", &dotnet, vec!["--info".into()]),
            ("GNU-TIME-VERSION", &time, vec!["--version".into()]),
            ("TIMEOUT-VERSION", &timeout, vec!["--version".into()]),
        ] {
            let result = runner.command(label, tool, &args, &root, false, &target)?;
            if !result.status.success() {
                return Err("actual bound tool version failed".into());
            }
            if label == "GNU-TIME-VERSION"
                && !String::from_utf8_lossy(&result.stdout).contains("GNU Time")
            {
                return Err("selected actual GNU-time identity/version required".into());
            }
        }
        support::headroom(&runner, "BEFORE-PHYSICAL-SETUP", &target)?;
        let target_schema = ir::SchemaNode::group(
            "Output",
            vec![ir::SchemaNode::scalar("n", ir::ScalarType::String)],
        );
        let instance = ir::Instance::Group(
            vec![(
                "n".into(),
                ir::Instance::Scalar(ir::Value::String("abc".into())),
            )]
            .into(),
        );
        let normalized = format_json::to_value(&target_schema, &instance);
        support::record(&root, "SMALL-WRITER-NORMALIZED-ORIGINAL", &normalized)?;
        let text = serde_json::to_string_pretty(&normalized?);
        support::record(&root, "SMALL-WRITER-PRETTY-ORIGINAL", &text)?;
        let text = text? + "\n";
        std::fs::write(root.join("SMALL-WRITER-COMPLETE.json"), &text)?;
        assert_eq!(text, "{\n  \"n\": \"abc\"\n}\n");
        assert_eq!(text.len(), 17);
        let cases = corpus::prepare(&root.join("corpus"))?;
        let hosts = prepare_libraries(&root)?;
        // Snapshot complete existing generated source trees before builds.
        for (_, _, host) in &hosts {
            dynamic.extend(support::tree(
                host.parent().ok_or("profile parent required")?,
            )?);
        }
        dynamic.extend(support::tree(&root.join("corpus"))?);
        support::record(&root, "GENERATED-SOURCE-CORPUS-BEFORE", &dynamic)?;
        let mut executables = Vec::new();
        let mut libraries = Vec::new();
        // All four profiles in both languages are built before the first call.
        for (language, profile, host) in &hosts {
            support::headroom(&runner, &format!("PRE-BUILD-{language}-{profile}"), &target)?;
            let label = format!("BUILD-{language}-{profile}");
            let args = if language == "rust" {
                vec![
                    "build".into(),
                    "--offline".into(),
                    "--jobs".into(),
                    "1".into(),
                    "--message-format=json".into(),
                    "--bin".into(),
                    format!("json5-resource-host-{profile}"),
                ]
            } else {
                vec![
                    "build".into(),
                    "Host.csproj".into(),
                    "--configuration".into(),
                    "Release".into(),
                    "--nologo".into(),
                    "-m:1".into(),
                    "-nr:false".into(),
                    "-p:UseSharedCompilation=false".into(),
                    "-p:BuildInParallel=false".into(),
                    "-p:NuGetAudit=false".into(),
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true".into(),
                    "-p:EnableTargetingPackDownload=false".into(),
                    "-p:EnableRuntimePackDownload=false".into(),
                    "-p:AutomaticallyUseReferenceAssemblyPackages=false".into(),
                ]
            };
            let result = runner.command(
                &label,
                if language == "rust" { &cargo } else { &dotnet },
                &args,
                host,
                false,
                &target,
            )?;
            if !result.status.success() {
                return Err(
                    "resource host build failed; no physical calls, originals retained".into(),
                );
            }
            let executable = if language == "rust" {
                rust_executable(
                    &result.stdout,
                    &format!("json5-resource-host-{profile}"),
                    &target,
                )?
            } else {
                std::fs::canonicalize(host.join("bin/Release/net10.0/Host.dll"))?
            };
            libraries.push(if language == "rust" {
                support::compiler_artifact_identity(
                    &executable,
                    &result.stdout,
                    &format!("json5-resource-host-{profile}"),
                    &target,
                )?
            } else {
                support::identity(&executable)?
            });
            if language == "csharp" {
                for file in [
                    "Host.deps.json",
                    "Host.runtimeconfig.json",
                    "Ferrule.Generated.dll",
                ] {
                    libraries.push(support::identity(
                        &host.join("bin/Release/net10.0").join(file),
                    )?);
                }
            }
            executables.push((language.clone(), profile.clone(), host.clone(), executable));
        }
        support::check_guard(&root, "AFTER-BUILD-ORIGINAL-SOURCE-CORPUS", &dynamic)?;
        // Include the new genuine Cargo.lock bodies after successful builds.
        for (language, _, host) in &hosts {
            if language == "rust" {
                dynamic.push(support::identity(&host.join("Cargo.lock"))?);
            }
        }
        dynamic.extend(libraries);
        support::record(&root, "COMPLETE-EXECUTION-LIBRARY-CORPUS-BEFORE", &dynamic)?;
        let mut count = 0;
        for case in &cases {
            let id = case["id"].as_str().ok_or("literal case id required")?;
            let profile = case["profile"].as_str().ok_or("literal profile required")?;
            for language in ["rust", "csharp"] {
                let (_, _, host, executable) = executables
                    .iter()
                    .find(|(lang, name, _, _)| lang == language && name == profile)
                    .ok_or("actual selected resource host required")?;
                for route in 0..4 {
                    support::check_guard(
                        &root,
                        &format!("PRE-{id}-{language}-{route}-FIXED"),
                        &fixed,
                    )?;
                    support::check_guard(
                        &root,
                        &format!("PRE-{id}-{language}-{route}-LIBRARY"),
                        &dynamic,
                    )?;
                    support::headroom(&runner, &format!("PRE-{id}-{language}-{route}"), &target)?;
                    let observation = root
                        .join("observations")
                        .join(format!("{id}-{language}-{route}"));
                    std::fs::create_dir(&observation)?;
                    let mut args = Vec::new();
                    if language == "csharp" {
                        args.push(executable.display().to_string());
                    }
                    args.extend([
                        root.join("corpus")
                            .join(format!("{id}.json"))
                            .display()
                            .to_string(),
                        route.to_string(),
                        observation.display().to_string(),
                    ]);
                    let observed = runner.command(
                        &format!("CALL-{id}-{language}-{route}"),
                        if language == "rust" {
                            executable
                        } else {
                            &dotnet
                        },
                        &args,
                        host,
                        true,
                        &target,
                    );
                    let fixed_after = support::check_guard(
                        &root,
                        &format!("POST-{id}-{language}-{route}-FIXED"),
                        &fixed,
                    );
                    let dynamic_after = support::check_guard(
                        &root,
                        &format!("POST-{id}-{language}-{route}-LIBRARY"),
                        &dynamic,
                    );
                    support::record(
                        &root,
                        &format!("CALL-{id}-{language}-{route}-STATUS-AND-AFTER"),
                        &(&observed, &fixed_after, &dynamic_after),
                    )?;
                    let observed = observed?;
                    fixed_after?;
                    dynamic_after?;
                    if !observed.status.success() {
                        return Err("physical public comparison failed; no further call, full originals retained".into());
                    }
                    let complete = support::json_file(&observation.join("CALL-COMPLETE.json"))?;
                    if complete["calls"] != 1
                        || complete["failures"] != 0
                        || complete["route"] != route
                        || complete["id"] != id
                        || complete["input_after_exact"] != true
                        || complete["case_after_exact"] != true
                    {
                        return Err("complete original one-call/after guard record required".into());
                    }
                    count += 1;
                    std::fs::write(
                        root.join("REACHED-PUBLIC-CALLS.json"),
                        serde_json::to_vec_pretty(
                            &json!({"completed":count,"planned":48,"physical":true}),
                        )?,
                    )?;
                }
            }
        }
        assert_eq!(count, 48);
        std::fs::write(
            root.join("COMPLETE-PHYSICAL-COUNT.json"),
            serde_json::to_vec_pretty(
                &json!({"total":48,"rust":24,"csharp":24,"failures":0,"small_receipt_bound":true}),
            )?,
        )?;
        Ok(())
    }));
    if let Err(original) = &outcome {
        let text = original
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| original.downcast_ref::<&str>().copied())
            .unwrap_or("opaque original panic payload; not fabricated");
        std::fs::write(root.join("ORIGINAL-PANIC.txt"), text)?;
    }
    let fixed_after = support::check_guard(&root, "FINAL-FIXED-GUARD-ORIGINAL", &fixed);
    let dynamic_after = support::check_guard(&root, "FINAL-DYNAMIC-GUARD-ORIGINAL", &dynamic);
    support::record(
        &root,
        "FINAL-GUARD-RESULTS",
        &(&fixed_after, &dynamic_after),
    )?;
    match outcome {
        Ok(result) => {
            support::record(&root, "ORIGINAL-CAMPAIGN-RESULT", &result)?;
            result?;
            fixed_after?;
            dynamic_after?;
            Ok(())
        }
        Err(original) => {
            let text = original
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| original.downcast_ref::<&str>().copied())
                .unwrap_or("opaque original panic payload; not fabricated");
            std::fs::write(root.join("ORIGINAL-PANIC.txt"), text)?;
            std::panic::resume_unwind(original)
        }
    }
}
