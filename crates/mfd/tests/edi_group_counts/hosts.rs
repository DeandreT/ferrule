use std::path::Path;
use std::process::Command;

use super::{Package, fixture, wire};

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn run(command: &mut Command, directory: &Path, label: &str) {
    let output = command.current_dir(directory).output().unwrap();
    let status = output.status.to_string();
    for (suffix, contents) in [
        ("stdout", output.stdout.as_slice()),
        ("stderr", output.stderr.as_slice()),
        ("status", status.as_bytes()),
    ] {
        std::fs::write(directory.join(format!("{label}.{suffix}")), contents).unwrap();
    }
    assert!(
        output.status.success(),
        "{label} failed in {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[cfg(windows)]
fn saved_user_path() -> Vec<u8> {
    let output = Command::new("reg.exe")
        .args(["query", r"HKCU\Environment", "/v", "Path"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
#[ignore = "compiles generated Rust and C# Release hosts; requires Cargo and the pinned .NET SDK"]
fn generated_release_hosts_preserve_edi_group_counts() {
    let imported = mfd::import(&fixture("counts.mfd")).unwrap();
    assert!(mfd::assess_import(&imported).executable);
    let program = codegen::lower(&imported.project).unwrap();
    let mut source =
        codegen::X12BoundaryOptions::from_format_options(&imported.project.source_options).unwrap();
    source.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
    source.lenient_segments = false;
    let policy = codegen::X12BoundaryPolicy {
        source: Some(source),
        target: None,
    };
    let package = Package::new();
    let rust = package.0.join("rust");
    let csharp = package.0.join("csharp");
    write_artifacts(
        &rust,
        &codegen_rust::emit_with_x12(
            &program,
            &codegen_rust::Options {
                package_name: "ferrule-edi-group-counts".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../codegen-runtime")
                        .canonicalize()
                        .unwrap()
                        .display()
                        .to_string(),
                ),
            },
            &policy,
        )
        .unwrap(),
    );
    write_artifacts(
        &csharp,
        &codegen_csharp::emit_with_x12(&program, &policy).unwrap(),
    );
    let mut cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../fixtures/edi-group-counts/cases.json")).unwrap();
    for case in &mut cases {
        case["input"] = wire(case).into();
    }
    let valid = cases[0]["input"].as_str().unwrap().to_owned();
    for malformed in [
        valid.replace("*00501*", "*0050A*"),
        valid.replace("W05*N*ALPHA~", ""),
        valid.replace("SE*3*0001~", ""),
    ] {
        cases.push(serde_json::json!({"input": malformed, "reject": true}));
    }
    for directory in [&rust, &csharp] {
        std::fs::write(
            directory.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    let manifest_path = rust.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(
        manifest_path,
        manifest.replace("[dependencies]", "[dependencies]\nserde_json = \"1\""),
    )
    .unwrap();
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("../fixtures/edi-group-counts/Host.rs.txt"),
    )
    .unwrap();
    // Cargo retains the outer target lock while running this test. A configured
    // launcher can lease a second reusable cache; the portable default uses a
    // disposable host-only cache rather than deadlocking on the outer target.
    let mut rust_command = match std::env::var_os("FERRULE_CODEGEN_CARGO_LAUNCHER") {
        Some(launcher) => {
            let mut command = Command::new("pwsh");
            command
                .args(["-NoProfile", "-File"])
                .arg(launcher)
                .arg("cargo");
            command
        }
        None => {
            let mut command = Command::new("cargo");
            let cache = package.0.join("cargo-target");
            command
                .env("CARGO_TARGET_DIR", &cache)
                .env("CARGO_BUILD_BUILD_DIR", &cache);
            command
        }
    };
    run(
        rust_command
            .args(["run", "--release", "--quiet"])
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_INCREMENTAL", "0")
            .env_remove("CARGO_ENCODED_RUSTFLAGS"),
        &rust,
        "rust-release",
    );
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness).unwrap();
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#,
    )
    .unwrap();
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("../fixtures/edi-group-counts/Host.cs.txt"),
    )
    .unwrap();
    std::fs::write(
        csharp.join("global.json"),
        include_str!("../../../../global.json"),
    )
    .unwrap();
    let home = csharp.join(".cli-home");
    let temp = csharp.join(".tmp");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&temp).unwrap();
    #[cfg(windows)]
    let before = saved_user_path();
    let dotnet = || {
        let mut command =
            Command::new(std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into()));
        command
            .env("DOTNET_CLI_HOME", &home)
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("UseSharedCompilation", "false")
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .env("TMPDIR", &temp);
        command
    };
    run(dotnet().arg("--version"), &csharp, "dotnet-sdk");
    run(
        dotnet().args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--disable-build-servers",
            "-p:UseSharedCompilation=false",
            "-nodeReuse:false",
        ]),
        &csharp,
        "csharp-release",
    );
    #[cfg(windows)]
    assert_eq!(
        before,
        saved_user_path(),
        "isolated .NET home changed User PATH"
    );
}
