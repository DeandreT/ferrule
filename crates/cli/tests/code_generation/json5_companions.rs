use super::*;
use cli::generate_project_with_json5_adapters;

#[path = "json5_companions/facade.rs"]
mod facade;
#[path = "json5_companions/projects.rs"]
mod projects;
#[path = "json5_companions/support.rs"]
mod support;

use support::{RetainedDirectory, check_guard, generate, write_project};

#[test]
#[ignore = "root-qualified JSON5 cohort requires bound shared target and offline toolchains"]
fn json5_companions_all184_small_public_calls_in_rust_and_csharp() -> TestResult<()> {
    let root = RetainedDirectory::new("small_public_hosts")?;
    let target = support::rust_target()?;
    std::fs::write(
        root.0.join("CASES.json"),
        include_bytes!("json5_companions/CASES.json"),
    )?;
    let cases: serde_json::Value =
        serde_json::from_slice(include_bytes!("json5_companions/CASES.json"))?;
    assert_eq!(
        cases["cases"]
            .as_array()
            .ok_or("cases must be an array")?
            .len(),
        23
    );
    for directory in ["original-generated", "rust", "csharp"] {
        std::fs::create_dir(root.0.join(directory))?;
    }
    for language in ["rust", "csharp"] {
        std::fs::create_dir(root.0.join("original-generated").join(language))?;
    }
    let runtime =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"))?;
    let mut rust_dependencies = String::new();
    let mut csharp_projects = String::new();
    for fixture in projects::FIXTURES {
        let candidate = projects::project(fixture);
        let project = write_project(&root, fixture, &candidate)?;
        for language in ["rust", "csharp"] {
            let original = root
                .0
                .join("original-generated")
                .join(language)
                .join(fixture);
            let selected = if language == "rust" {
                GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                }
            } else {
                GenerateTarget::CSharp
            };
            generate(
                &root,
                &project,
                &original,
                selected,
                &format!("{language}-{fixture}"),
            )?;
            // Keep the complete public-writer artifact tree. Only a separate
            // compile-host copy receives the unique Cargo package-name fixture.
            let output = root.0.join(language).join(fixture);
            support::copy_tree(&original, &output)?;
            if language == "rust" {
                let manifest = output.join("Cargo.toml");
                let text = std::fs::read_to_string(&manifest)?;
                let package = format!("ferrule-json5-small-{}", fixture.to_ascii_lowercase());
                let before = "name = \"ferrule-generated-mapping\"";
                if text.matches(before).count() != 1 {
                    return Err("unexpected generated package declaration".into());
                }
                std::fs::write(
                    manifest,
                    text.replacen(before, &format!("name = {package:?}"), 1),
                )?;
                rust_dependencies.push_str(&format!(
                    "fixture_{} = {{ package = {package:?}, path = {:?} }}\n",
                    fixture.to_ascii_lowercase(),
                    format!("../{fixture}")
                ));
            } else {
                csharp_projects.push_str(&format!(
                    "    <Projects Include=\"{fixture}/Ferrule.Generated.csproj\" />\n"
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
            "[package]\nname = \"ferrule-json5-small-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncodegen-runtime = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n{rust_dependencies}\n[workspace]\n",
            runtime.to_str().ok_or("runtime path must be UTF-8")?
        ),
    )?;
    std::fs::write(
        rust_host.join("src/main.rs"),
        include_str!("json5_companions/Host.rs.txt"),
    )?;
    let csharp_host = root.0.join("csharp/Host");
    std::fs::create_dir(&csharp_host)?;
    std::fs::write(csharp_host.join("Host.csproj"), support::HOST_PROJECT)?;
    std::fs::write(
        csharp_host.join("Program.cs"),
        include_str!("json5_companions/Host.cs.txt"),
    )?;
    csharp_projects.push_str("    <Projects Include=\"Host/Host.csproj\" />\n");
    std::fs::write(
        root.0.join("csharp/Cohort.proj"),
        format!(
            "<Project>\n  <ItemGroup>\n{csharp_projects}  </ItemGroup>\n  <Target Name=\"Build\">\n    <MSBuild Projects=\"@(Projects)\" Targets=\"Restore;Build\" BuildInParallel=\"false\" />\n  </Target>\n</Project>\n"
        ),
    )?;
    let before = support::source_guard(&root)?;
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> TestResult<Vec<support::Identity>> {
            let mut cargo = Command::new("cargo");
            cargo
                .args(["build", "--offline", "--jobs", "1"])
                .current_dir(&rust_host)
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_INCREMENTAL", "0")
                .env("RUSTFLAGS", "-Dwarnings");
            let built = root.command("RUST-BUILD", &mut cargo)?;
            if !built.status.success() {
                return Err("Rust small public host build failed; originals retained".into());
            }
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
            let built = root.command("CSHARP-BUILD", &mut dotnet)?;
            if !built.status.success() {
                return Err("C# small public host build failed; originals retained".into());
            }
            let libraries = support::library_guard(&root, &target)?;
            let mut rust = Command::new(target.join("debug").join(format!(
                "ferrule-json5-small-host{}",
                std::env::consts::EXE_SUFFIX
            )));
            rust.arg(&root.0).current_dir(&rust_host);
            let rust_result = root.command("RUST-HOST", &mut rust);
            // Both languages are observed even if one host returns a failure.
            let mut csharp = dotnet_command(&root.0.join("csharp"));
            csharp
                .arg(csharp_host.join("bin/Debug/net10.0/Host.dll"))
                .arg(&root.0)
                .current_dir(root.0.join("csharp"));
            let csharp_result = root.command("CSHARP-HOST", &mut csharp);
            let library_guard = check_guard(&root, &libraries, "COMPLETE-LIBRARY-GUARD-AFTER");
            root.record(
                "BOTH-HOST-ORIGINAL-RESULTS",
                &(&rust_result, &csharp_result, &library_guard),
            )?;
            library_guard?;
            if !rust_result?.status.success() || !csharp_result?.status.success() {
                return Err(
                    "small public host comparison failed; complete originals retained".into(),
                );
            }
            let rust: serde_json::Value = serde_json::from_slice(&std::fs::read(
                root.0.join("COMPLETE_CALL_COUNT_RUST.json"),
            )?)?;
            let csharp: serde_json::Value = serde_json::from_slice(&std::fs::read(
                root.0.join("COMPLETE_CALL_COUNT_CSHARP.json"),
            )?)?;
            root.record("ALL184-SMALL-GATE-ORIGINAL", &(&rust, &csharp))?;
            if rust["total"] != 92
                || csharp["total"] != 92
                || rust["failures"] != 0
                || csharp["failures"] != 0
            {
                return Err(
                    "all 184 small public calls must pass before physical qualification".into(),
                );
            }
            Ok(libraries)
        },
    ));
    let guard = check_guard(&root, &before, "COMPLETE-INPUT-SOURCE-GUARD-AFTER");
    root.record("COMPLETE-INPUT-SOURCE-GUARD-RESULT", &guard)?;
    match original {
        Ok(result) => {
            root.record("ORIGINAL-COHORT-RESULT", &result)?;
            let libraries = result?;
            guard?;
            support::write_small_gate(&root, &before, &libraries)?;
            Ok(())
        }
        Err(original) => {
            let message = original
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| original.downcast_ref::<&str>().copied())
                .unwrap_or("non-string original panic payload");
            std::fs::write(root.0.join("ORIGINAL-PANIC.txt"), format!("{message}\n"))?;
            std::panic::resume_unwind(original)
        }
    }
}
