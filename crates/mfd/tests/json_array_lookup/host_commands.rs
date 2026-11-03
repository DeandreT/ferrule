use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

const CSHARP_HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit>
    <DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>
    <AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup>
</Project>
"#;

fn copy_sdk_policy(root: &Path, source: &Path) -> Result<String, String> {
    let bytes = std::fs::read(source).map_err(|error| format!("read SDK global.json: {error}"))?;
    let policy: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "SDK global.json must be valid JSON".to_owned())?;
    let version = policy["sdk"]["version"]
        .as_str()
        .ok_or("SDK global.json must declare sdk.version")?;
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("SDK global.json must pin a stable three-part sdk.version".into());
    }
    if policy["sdk"]["rollForward"] != "latestPatch" || policy["sdk"]["allowPrerelease"] != false {
        return Err(
            "SDK global.json must set sdk.rollForward=latestPatch and sdk.allowPrerelease=false"
                .into(),
        );
    }
    // SDK resolution follows cwd ancestry, not the --project argument.
    std::fs::write(root.join("global.json"), &bytes)
        .map_err(|error| format!("copy SDK global.json: {error}"))?;
    Ok(version.to_owned())
}

pub(super) fn prepare_csharp_host(root: &Path, hosts: &Path, shared: &Path) -> String {
    let sdk_version = copy_sdk_policy(
        root,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
    )
    .unwrap_or_else(|message| panic!("{message}"));
    let host = root.join("csharp/host");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(
        root.join("NuGet.Config"),
        r#"<configuration><packageSources><clear/></packageSources></configuration>"#,
    )
    .unwrap();
    std::fs::write(host.join("Host.csproj"), CSHARP_HOST_PROJECT).unwrap();
    std::fs::copy(hosts.join("csharp_host.cs"), host.join("Program.cs")).unwrap();
    std::fs::copy(shared.join("typed_assert.cs"), host.join("LookupOracle.cs")).unwrap();
    sdk_version
}

pub(super) fn check_sdk_version(stdout: &[u8], expected: &str) -> Result<(), String> {
    let actual = std::str::from_utf8(stdout)
        .map_err(|_| "dotnet --version returned non-UTF-8 output".to_owned())?;
    if actual.trim() != expected {
        return Err(format!(
            "dotnet --version must select the exact global.json sdk.version ({expected})"
        ));
    }
    Ok(())
}

pub(super) fn rust_command(root: &Path, fixtures: &Path) -> Command {
    let rust = root.join("rust");
    let mut command = Command::new("cargo");
    command
        .current_dir(&rust)
        .args([
            "run",
            "--release",
            "--offline",
            "-j",
            "1",
            "--config",
            "profile.release.incremental=false",
            "--config",
            "profile.release.strip=\"symbols\"",
            "--manifest-path",
        ])
        // Keep the generated crate's default target, never the outer build's.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .arg(rust.join("Cargo.toml"))
        .args(["--bin", "lookup-host", "--"])
        .arg(fixtures.join("cases.json"))
        .arg(root.join("source-schema.json"))
        .arg(root.join("target-schema.json"));
    command
}

pub(super) fn csharp_commands(
    root: &Path,
    fixtures: &Path,
    executable: &OsStr,
) -> (Command, Command) {
    let command = || {
        let mut command = Command::new(executable);
        command
            .current_dir(root)
            .env("DOTNET_CLI_HOME", root.join("dotnet-home"))
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1");
        command
    };
    let mut sdk = command();
    sdk.arg("--version");
    let mut host = command();
    host.args([
        "run",
        "--configuration",
        "Release",
        "--disable-build-servers",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
        "-p:NuGetAudit=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
        "--project",
    ])
    .arg(root.join("csharp/host/Host.csproj"))
    .arg("--")
    .arg(fixtures.join("cases.json"))
    .arg(root.join("source-schema.json"))
    .arg(root.join("target-schema.json"));
    (sdk, host)
}

#[test]
fn csharp_host_copies_exact_sdk_policy_and_constructs_matching_offline_commands() {
    use std::collections::BTreeMap;

    let dir = super::TempDir::new();
    let shared = super::fixtures();
    let fixtures = shared.join("root-scalars");
    let policy =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json")).unwrap();
    let policy_path = dir.0.join("global.json");
    assert!(!policy_path.exists());
    let version = prepare_csharp_host(&dir.0, &fixtures, &shared);
    assert_eq!(std::fs::read(&policy_path).unwrap(), policy);
    let expected: serde_json::Value = serde_json::from_slice(&policy).unwrap();
    assert_eq!(version, expected["sdk"]["version"].as_str().unwrap());
    // Preparation also replaces stale sandbox policy, rather than preserving it.
    std::fs::write(&policy_path, b"{}\n").unwrap();
    assert_eq!(prepare_csharp_host(&dir.0, &fixtures, &shared), version);
    assert_eq!(std::fs::read(&policy_path).unwrap(), policy);
    let project = std::fs::read_to_string(dir.0.join("csharp/host/Host.csproj")).unwrap();
    let project = roxmltree::Document::parse(&project).unwrap();
    for (property, value) in [
        ("DisableTransitiveFrameworkReferenceDownloads", "true"),
        ("AutomaticallyUseReferenceAssemblyPackages", "false"),
    ] {
        assert_eq!(
            project
                .descendants()
                .find(|node| node.has_tag_name(property))
                .unwrap()
                .text(),
            Some(value)
        );
    }
    assert_eq!(
        std::fs::read(dir.0.join("csharp/host/Program.cs")).unwrap(),
        std::fs::read(fixtures.join("csharp_host.cs")).unwrap()
    );
    assert_eq!(
        std::fs::read(dir.0.join("csharp/host/LookupOracle.cs")).unwrap(),
        std::fs::read(shared.join("typed_assert.cs")).unwrap()
    );
    // Construction only: never execute this path or claim an SDK observation.
    let executable = dir.0.join("construction-only-dotnet");
    let (sdk, host) = csharp_commands(&dir.0, &fixtures, executable.as_os_str());
    assert_eq!(sdk.get_program(), executable.as_os_str());
    assert_eq!(host.get_program(), sdk.get_program());
    assert_eq!(sdk.get_current_dir(), Some(dir.0.as_path()));
    assert_eq!(host.get_current_dir(), sdk.get_current_dir());
    assert_eq!(
        sdk.get_envs().collect::<Vec<_>>(),
        host.get_envs().collect::<Vec<_>>()
    );
    let environment = host.get_envs().collect::<BTreeMap<_, _>>();
    for (key, value) in [
        ("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false"),
        ("DOTNET_NOLOGO", "1"),
        ("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1"),
        ("DOTNET_CLI_USE_MSBUILD_SERVER", "0"),
        ("MSBUILDDISABLENODEREUSE", "1"),
    ] {
        assert_eq!(environment[OsStr::new(key)], Some(OsStr::new(value)));
    }
    assert_eq!(
        environment[OsStr::new("DOTNET_CLI_HOME")],
        Some(dir.0.join("dotnet-home").as_os_str())
    );
    assert_eq!(
        sdk.get_args().collect::<Vec<_>>(),
        [OsStr::new("--version")]
    );
    let args = host.get_args().collect::<Vec<_>>();
    for required in [
        "run",
        "Release",
        "--disable-build-servers",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
    ] {
        assert!(args.contains(&OsStr::new(required)));
    }
    let project_arg = args.iter().position(|arg| *arg == "--project").unwrap();
    assert_eq!(args[project_arg + 1], dir.0.join("csharp/host/Host.csproj"));
    assert_application_arguments(&host, &dir.0, &fixtures);
}

#[test]
fn rust_host_keeps_its_default_target_one_build_job_and_three_application_arguments() {
    use std::collections::BTreeMap;

    let dir = super::TempDir::new();
    let fixtures = super::fixtures().join("root-scalars");
    let command = rust_command(&dir.0, &fixtures);
    assert_eq!(command.get_program(), "cargo");
    assert_eq!(
        command.get_current_dir(),
        Some(dir.0.join("rust").as_path())
    );
    let environment = command.get_envs().collect::<BTreeMap<_, _>>();
    for removed in [
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "CARGO_BUILD_BUILD_DIR",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        assert_eq!(environment[OsStr::new(removed)], None);
    }
    assert_eq!(
        environment[OsStr::new("RUSTFLAGS")],
        Some(OsStr::new("-D warnings"))
    );
    let args = command.get_args().collect::<Vec<_>>();
    assert_eq!(
        &args[..11],
        [
            "run",
            "--release",
            "--offline",
            "-j",
            "1",
            "--config",
            "profile.release.incremental=false",
            "--config",
            "profile.release.strip=\"symbols\"",
            "--manifest-path",
            dir.0.join("rust/Cargo.toml").to_str().unwrap(),
        ]
    );
    assert_eq!(&args[11..14], ["--bin", "lookup-host", "--"]);
    assert!(!args.contains(&OsStr::new("--target-dir")));
    assert_application_arguments(&command, &dir.0, &fixtures);
}

fn assert_application_arguments(command: &Command, root: &Path, fixtures: &Path) {
    let args = command.get_args().collect::<Vec<_>>();
    let separator = args.iter().position(|arg| *arg == "--").unwrap();
    assert_eq!(
        &args[separator + 1..],
        [
            fixtures.join("cases.json").as_os_str(),
            root.join("source-schema.json").as_os_str(),
            root.join("target-schema.json").as_os_str(),
        ]
    );
}

#[test]
fn malformed_sdk_policies_refuse_before_copying_or_constructing_a_build() {
    let dir = super::TempDir::new();
    let source = dir.0.join("source-global.json");
    for (bytes, message) in [
        ("{", "SDK global.json must be valid JSON"),
        ("{}", "SDK global.json must declare sdk.version"),
        (
            r#"{"sdk":{"version":10}}"#,
            "SDK global.json must declare sdk.version",
        ),
        (
            r#"{"sdk":{"version":""}}"#,
            "SDK global.json must pin a stable three-part sdk.version",
        ),
        (
            r#"{"sdk":{"version":"10.0"}}"#,
            "SDK global.json must pin a stable three-part sdk.version",
        ),
        (
            r#"{"sdk":{"version":"10.0.112-preview"}}"#,
            "SDK global.json must pin a stable three-part sdk.version",
        ),
        (
            r#"{"sdk":{"version":"10.0.112","rollForward":"latestFeature","allowPrerelease":false}}"#,
            "SDK global.json must set sdk.rollForward=latestPatch and sdk.allowPrerelease=false",
        ),
        (
            r#"{"sdk":{"version":"10.0.112","rollForward":"latestPatch","allowPrerelease":true}}"#,
            "SDK global.json must set sdk.rollForward=latestPatch and sdk.allowPrerelease=false",
        ),
    ] {
        std::fs::write(&source, bytes).unwrap();
        assert_eq!(copy_sdk_policy(&dir.0, &source).unwrap_err(), message);
        assert!(!dir.0.join("global.json").exists());
    }
}

#[test]
fn sdk_probe_requires_the_exact_locked_version_not_another_patch_or_band() {
    assert!(check_sdk_version(b"10.0.112\n", "10.0.112").is_ok());
    assert!(check_sdk_version(b"10.0.112\r\n", "10.0.112").is_ok());
    for observed in ["", "10.0.113\n", "10.0.200\n", "10.0.112-preview\n"] {
        assert_eq!(
            check_sdk_version(observed.as_bytes(), "10.0.112").unwrap_err(),
            "dotnet --version must select the exact global.json sdk.version (10.0.112)"
        );
    }
    assert_eq!(
        check_sdk_version(&[0xff], "10.0.112").unwrap_err(),
        "dotnet --version returned non-UTF-8 output"
    );
}

#[test]
fn process_receipts_keep_actual_command_stdout_stderr_and_status() {
    let dir = super::TempDir::new();
    let executable = std::env::current_exe().unwrap();
    let mut command = Command::new(&executable);
    command.current_dir(&dir.0).arg("--list");
    let output = super::checked(&dir.0, "receipt-control", &mut command);
    let recorded: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("receipt-control-command.json")).unwrap())
            .unwrap();
    assert_eq!(recorded["command"], executable.to_string_lossy().as_ref());
    assert_eq!(recorded["arguments"], serde_json::json!(["--list"]));
    assert_eq!(recorded["cwd"], dir.0.to_string_lossy().as_ref());
    assert_eq!(
        std::fs::read(dir.0.join("receipt-control.stdout")).unwrap(),
        output.stdout
    );
    assert_eq!(
        std::fs::read(dir.0.join("receipt-control.stderr")).unwrap(),
        output.stderr
    );
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("receipt-control-status.json")).unwrap())
            .unwrap();
    assert_eq!(status, serde_json::json!({"success": true, "exit_code": 0}));
}
