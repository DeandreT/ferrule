//! Ordinary emitted-library consumers, opt-in only. No direct-runtime substitute.
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

// Keep the maintained target-overlap checks and process-local cache lease.
#[path = "../../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
#[path = "../../../cli/tests/code_generation/host_policy.rs"]
#[allow(dead_code)]
mod safe_host_policy;
use dotnet_temp::DotnetTemp;

trait CommandOutputExt {
    fn isolated_output(&mut self) -> io::Result<Output>;
}
impl CommandOutputExt for Command {
    fn isolated_output(&mut self) -> io::Result<Output> {
        self.output()
    }
}

pub struct TempDir(pub PathBuf);
impl TempDir {
    pub fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_composition_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        safe_host_policy::preflight_environment(&path)?;
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() || safe_host_policy::keep_artifacts() {
            eprintln!("retained composition originals: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn recorded(command: &mut Command, root: &Path, label: &str) -> Output {
    let path = root.join(format!("{label}.command.original.txt"));
    std::fs::write(path, format!("{command:?}\n")).unwrap();
    let result = command.output();
    match result {
        Ok(output) => {
            std::fs::write(
                root.join(format!("{label}.stdout.original")),
                &output.stdout,
            )
            .unwrap();
            std::fs::write(
                root.join(format!("{label}.stderr.original")),
                &output.stderr,
            )
            .unwrap();
            std::fs::write(
                root.join(format!("{label}.status.original")),
                format!("{:?}\n", output.status),
            )
            .unwrap();
            assert!(
                output.status.success(),
                "{command:?}\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            output
        }
        Err(error) => {
            std::fs::write(
                root.join(format!("{label}.spawn-error.original")),
                format!("{error:?}\n"),
            )
            .unwrap();
            panic!("{command:?}: {error}");
        }
    }
}

fn dotnet(root: &Path) -> Command {
    let mut command =
        Command::new(std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into()));
    command
        .current_dir(root)
        .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("NUGET_PACKAGES", root.join(".nuget-packages"))
        .dotnet_temp(root);
    command
}

pub fn ordinary_artifacts(
    project: &mapping::Project,
) -> (codegen::ArtifactSet, codegen::ArtifactSet) {
    let program = codegen::lower(project).expect(
        "ordinary profile must admit the authored composition; never replace it with a passthrough",
    );
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-generated-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        },
    )
    .unwrap();
    let csharp = codegen_csharp::emit(&program).unwrap();
    (rust, csharp)
}

pub fn run(
    project: &mapping::Project,
    catalog: &Path,
    label: &str,
    rust_host: &str,
    csharp_host: &str,
) {
    let root = TempDir::new(label).unwrap();
    let (rust, csharp) = ordinary_artifacts(project);
    for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
        for file in artifacts.files() {
            let path = root.0.join(language).join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file.contents).unwrap();
        }
    }
    std::fs::write(
        root.0.join("project.original.json"),
        mapping::project_file::encode_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::copy(catalog, root.0.join("cases.json")).unwrap();
    for (name, schema) in [("source", &project.source), ("target", &project.target)] {
        std::fs::write(
            root.0.join(format!("{name}-schema.json")),
            serde_json::to_string(schema).unwrap(),
        )
        .unwrap();
    }
    for target in &project.extra_targets {
        std::fs::write(
            root.0.join(format!("{}-schema.json", target.name)),
            serde_json::to_string(&target.schema).unwrap(),
        )
        .unwrap();
    }
    let rust = root.0.join("rust");
    safe_host_policy::name_generated_host(&rust, root.0.file_name().unwrap().to_str().unwrap())
        .unwrap();
    let manifest = rust.join("Cargo.toml");
    let body = std::fs::read_to_string(&manifest).unwrap();
    assert_eq!(body.matches("[dependencies]").count(), 1);
    std::fs::write(
        &manifest,
        body.replace("[dependencies]", "[dependencies]\nserde_json = \"1\""),
    )
    .unwrap();
    std::fs::write(rust.join("src/main.rs"), rust_host).unwrap();
    std::fs::write(rust.join("src/oracle.rs"), include_str!("Oracle.rs.txt")).unwrap();
    safe_host_policy::with_cargo_target(&root.0, |target| {
        let mut command = Command::new("cargo");
        command
            .current_dir(&rust)
            .args(["run", "--release", "--offline", "-j1", "--target-dir"])
            .arg(target)
            .args([
                "--config",
                "profile.release.debug=0",
                "--config",
                "profile.release.incremental=false",
                "--config",
                "profile.release.strip=\"symbols\"",
            ])
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-D warnings")
            .arg("--")
            .arg(root.0.join("cases.json"))
            .arg(root.0.join("source-schema.json"))
            .arg(root.0.join("target-schema.json"));
        recorded(&mut command, &root.0, "rust-release");
    })
    .unwrap();
    let csharp = root.0.join("csharp");
    let host = csharp.join("host");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(host.join("Program.cs"), csharp_host).unwrap();
    std::fs::write(host.join("Oracle.cs"), include_str!("Oracle.cs.txt")).unwrap();
    std::fs::write(host.join("Host.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../Ferrule.Generated.csproj\"/></ItemGroup></Project>\n").unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
        root.0.join("global.json"),
    )
    .unwrap();
    std::fs::write(
        root.0.join("NuGet.Config"),
        "<configuration><packageSources><clear/></packageSources></configuration>\n",
    )
    .unwrap();
    std::fs::create_dir(root.0.join(".dotnet-home")).unwrap();
    std::fs::create_dir(root.0.join(".nuget-packages")).unwrap();
    // Same executable, cwd and environment for SDK proof, build and execution.
    let version = recorded(dotnet(&root.0).arg("--version"), &root.0, "dotnet-version");
    let pin: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.0.join("global.json")).unwrap()).unwrap();
    let expected = pin["sdk"]["version"].as_str().unwrap();
    let actual = String::from_utf8(version.stdout).unwrap();
    let actual = actual.trim();
    let prefix = &expected[..expected.rfind('.').unwrap() + 1];
    assert!(
        actual.starts_with(prefix) && !actual.contains('-'),
        "SDK pin {expected}: {actual}"
    );
    let patch = |version: &str| version.rsplit('.').next().unwrap().parse::<u32>().unwrap();
    assert_eq!(
        patch(actual) / 100,
        patch(expected) / 100,
        "SDK feature band"
    );
    assert!(patch(actual) >= patch(expected), "SDK patch precedes pin");
    recorded(
        dotnet(&root.0).args([
            "build",
            "csharp/host/Host.csproj",
            "--configuration",
            "Release",
            "--disable-build-servers",
            "-m:1",
            "-nodeReuse:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
        ]),
        &root.0,
        "csharp-release-build",
    );
    recorded(
        dotnet(&root.0)
            .arg(host.join("bin/Release/net10.0/Host.dll"))
            .arg(root.0.join("cases.json"))
            .arg(root.0.join("source-schema.json"))
            .arg(root.0.join("target-schema.json")),
        &root.0,
        "csharp-release-run",
    );
}
