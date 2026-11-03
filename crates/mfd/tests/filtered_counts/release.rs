//! TEST-ONLY Release runner draft. No command in this file has been executed.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const RESERVE: u64 = 45 * 1024 * 1024 * 1024;

fn admission(free: Option<u64>) -> Result<(), u64> {
    match free {
        Some(bytes) if bytes < RESERVE => Err(bytes),
        _ => Ok(()),
    }
}

fn admitted<T>(free: Option<u64>, invoke: impl FnOnce() -> T) -> Result<T, u64> {
    admission(free).map(|()| invoke())
}

fn available_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let space = rustix::fs::statvfs(path).unwrap();
        Some(space.f_bavail.saturating_mul(space.f_frsize))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn command_receipt(invocation: &Path, command: &Command, free: Option<u64>) {
    json(
        &invocation.join("command.json"),
        &serde_json::json!({
            "program":command.get_program(), "arguments":command.get_args().collect::<Vec<_>>(),
            "cwd":command.get_current_dir(), "free_bytes_before":free,
            // Only overrides/removals deliberately constructed here; never inherited secrets.
            "environment":command.get_envs().map(|(key,value)|(key.to_string_lossy().into_owned(),value.map(|value|value.to_string_lossy().into_owned()))).collect::<std::collections::BTreeMap<_,_>>(),
        }),
    );
}

/// General recording is NOT generated-build admission (P2).
fn capture(root: &Path, label: &str, command: &mut Command, free: Option<u64>) -> Output {
    let invocation = root.join("receipts").join(label);
    std::fs::create_dir_all(invocation.parent().unwrap()).unwrap();
    std::fs::create_dir(&invocation).unwrap();
    command_receipt(&invocation, command, free);
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            json(
                &invocation.join("spawn-error.json"),
                &serde_json::json!({"kind":format!("{:?}",error.kind()),"error":format!("{error:?}")}),
            );
            json(
                &invocation.join("status.json"),
                &serde_json::json!({"started":false,"success":false,"exit_code":null,"signal":null,"spawn_error":true}),
            );
            panic!("child failed to start; original retained: {error}");
        }
    };
    std::fs::write(invocation.join("stdout.original"), &output.stdout).unwrap();
    std::fs::write(invocation.join("stderr.original"), &output.stderr).unwrap();
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        output.status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    json(
        &invocation.join("status.json"),
        &serde_json::json!({
            "started":true,"success":output.status.success(),"exit_code":output.status.code(),"signal":signal,
        }),
    );
    output
}

fn checked(root: &Path, label: &str, command: &mut Command, free: Option<u64>) -> Output {
    let output = capture(root, label, command, free);
    assert!(
        output.status.success(),
        "{label}: actual stdout/stderr/status retained in receipts"
    );
    output
}

/// Only actual cargo build / dotnet build callers use this wrapper.
fn generated_build(root: &Path, label: &str, command: &mut Command) -> Output {
    let free = available_space(command.get_current_dir().unwrap());
    match admitted(free, || checked(root, label, command, free)) {
        Ok(output) => output,
        Err(bytes) => {
            let invocation = root.join("receipts").join(label);
            std::fs::create_dir_all(invocation.parent().unwrap()).unwrap();
            std::fs::create_dir(&invocation).unwrap();
            command_receipt(&invocation, command, free);
            json(
                &invocation.join("status.json"),
                &serde_json::json!({
                    "started":false,"admission_refused":true,"required_bytes":RESERVE,"available_bytes":bytes,"exit_code":null,
                }),
            );
            panic!("generated host requires 45 GiB free; available: {bytes}");
        }
    }
}

fn cargo(cwd: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(cwd)
        .env("RUSTFLAGS", "-D warnings")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env_remove("RUSTUP_TOOLCHAIN");
    for name in [
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "CARGO_BUILD_BUILD_DIR",
        "CARGO_BUILD_JOBS",
        "CARGO_INCREMENTAL",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("CARGO_PROFILE_") {
            command.env_remove(name);
        }
    }
    command
}

fn rust_build(cwd: &Path, target: &Path) -> Command {
    let mut command = cargo(cwd);
    command
        .args([
            "build",
            "--release",
            "--offline",
            "--locked",
            "-j",
            "1",
            "--config",
            "profile.release.incremental=false",
            "--config",
            "profile.release.debug=0",
            "--config",
            "profile.release.strip=\"symbols\"",
            "--manifest-path",
        ])
        .arg(cwd.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target);
    command
}

fn csharp_build(cwd: &Path, executable: &Path, artifacts: &Path) -> Command {
    let mut command = super::sdk_policy::dotnet_command(cwd, executable.as_os_str());
    command
        .args([
            "build",
            "--configuration",
            "Release",
            "--disable-build-servers",
            "-nodeReuse:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:DisableTransitiveFrameworkReferenceDownloads=true",
            "-p:AutomaticallyUseReferenceAssemblyPackages=false",
        ])
        .arg(format!(
            "-p:RestoreConfigFile={}",
            cwd.join("NuGet.Config").display()
        ))
        .arg("--artifacts-path")
        .arg(artifacts)
        .arg(cwd.join("Harness/Harness.csproj"));
    command
}

fn hash_file(path: &Path) -> serde_json::Value {
    let mut input = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let bytes = input.read(&mut buffer).unwrap();
        if bytes == 0 {
            break;
        }
        hash.update(&buffer[..bytes]);
        count += bytes as u64;
    }
    serde_json::json!({"path":path,"bytes":count,"sha256":format!("{:x}",hash.finalize())})
}

pub(super) fn source_fence(epoch_path: &Path) {
    let epoch: serde_json::Value =
        serde_json::from_slice(&std::fs::read(epoch_path).unwrap()).unwrap();
    assert_eq!(epoch["version"], 1);
    let repository = PathBuf::from(epoch["repository"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    assert_eq!(
        repository,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    );
    let head = epoch["head"].as_str().unwrap();
    assert_eq!(head.len(), 40);
    assert!(head.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let sources = epoch["files"].as_array().unwrap();
    assert!(
        !sources.is_empty(),
        "parent must supply the complete frozen source/runtime readset"
    );
    let mut seen = std::collections::BTreeSet::new();
    for source in sources {
        let relative = Path::new(source["path"].as_str().unwrap());
        assert!(seen.insert(relative));
        assert!(
            !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        let path = repository.join(relative).canonicalize().unwrap();
        assert!(path.starts_with(&repository));
        let actual = hash_file(&path);
        assert_eq!(actual["bytes"], source["bytes"]);
        assert_eq!(
            actual["sha256"], source["sha256"],
            "frozen source/runtime bytes changed"
        );
    }
}

fn fence(root: &Path) {
    source_fence(&root.join("source-epoch.json"));
    let readset: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("readset-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(readset["version"], 1);
    for item in readset["files"].as_array().unwrap() {
        let relative = Path::new(item["path"].as_str().unwrap());
        assert!(
            !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        let path = root.join(relative).canonicalize().unwrap();
        assert!(path.starts_with(root));
        let actual = hash_file(&path);
        assert_eq!(actual["bytes"], item["bytes"]);
        assert_eq!(
            actual["sha256"],
            item["sha256"],
            "prepared source/fixture/policy changed: {}",
            path.display()
        );
    }
}

fn distinct_target(target: &Path, outer: &Path) {
    assert!(target.is_absolute() && outer.is_absolute());
    assert!(!target.starts_with(outer) && !outer.starts_with(target));
    assert!(
        !std::env::current_exe()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(target)
    );
}

fn host_protocol(output: &Output, cases: &serde_json::Value) {
    let text = std::str::from_utf8(&output.stdout).unwrap();
    let records = text
        .lines()
        .filter(|line| line.starts_with('{'))
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let mut expected_total = 0;
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let selected = records
            .iter()
            .filter(|record| {
                record
                    .get("case")
                    .or_else(|| record.get("name"))
                    .and_then(serde_json::Value::as_str)
                    == Some(name)
            })
            .collect::<Vec<_>>();
        if case["reject"] == true {
            assert_eq!(
                selected.len(),
                2,
                "both actual input boundaries must refuse: {name}"
            );
            expected_total += 2;
            for boundary in ["text", "utf8"] {
                let matching = selected
                    .iter()
                    .filter(|record| record["boundary"] == boundary)
                    .collect::<Vec<_>>();
                assert_eq!(matching.len(), 1);
                assert_eq!(matching[0]["stage"], "input");
                assert_eq!(matching[0]["category"], case["runtimeCategory"]);
            }
        } else {
            assert_eq!(
                selected.len(),
                4,
                "two real calls for both boundaries required: {name}"
            );
            expected_total += 4;
            for boundary in ["text", "utf8"] {
                for attempt in 1..=2 {
                    let matching = selected
                        .iter()
                        .filter(|record| {
                            record["boundary"] == boundary && record["attempt"] == attempt
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(matching.len(), 1);
                    assert_eq!(matching[0]["stage"], "mapping");
                    assert_eq!(matching[0]["output"], case["expected"]);
                    if case.get("expectedXml").is_some() {
                        assert!(matching[0]["xml"].is_string());
                    }
                }
            }
        }
    }
    assert_eq!(
        records.len(),
        expected_total,
        "unexpected or missing host observations"
    );
}

#[test]
#[ignore = "actual generated Rust and C# Release builds; parent must grant the lane after native freeze/review"]
fn compiled_filtered_count_release_hosts_from_saved_projects() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_FILTERED_COUNT_ARTIFACT_ROOT")
            .expect("explicit prepared root required"),
    )
    .canonicalize()
    .unwrap();
    let outer = PathBuf::from(
        std::env::var_os("FERRULE_FILTERED_COUNT_OUTER_TARGET")
            .expect("exact outer --target-dir identity required"),
    )
    .canonicalize()
    .unwrap();
    let selected = PathBuf::from(
        std::env::var_os("FERRULE_DOTNET_EXE")
            .expect("parent supplies the verified actual dotnet executable"),
    );
    assert!(selected.is_absolute());
    let dotnet = selected.canonicalize().unwrap();
    assert!(dotnet.is_file());
    let dotnet_identity = hash_file(&dotnet);
    json(&root.join("dotnet-executable.json"), &dotnet_identity);
    fence(&root);
    for cohort in ["xml", "bool", "edi", "xml-output"] {
        let case = root.join(cohort);
        let admission: serde_json::Value =
            serde_json::from_slice(&std::fs::read(case.join("admission.json")).unwrap()).unwrap();
        for key in ["project_execution_contract", "program_execution_contract"] {
            assert!(admission.get(key).is_some_and(serde_json::Value::is_null));
        }
        assert_eq!(admission["arbitrary_host_guard_claim"], false);
        let rust = case.join("rust");
        let csharp = case.join("csharp");
        let rust_target = case.join("rust-target");
        let cs_artifacts = case.join("csharp-artifacts");
        let cases: serde_json::Value =
            serde_json::from_slice(&std::fs::read(rust.join("cases.json")).unwrap()).unwrap();
        let cs_cases: serde_json::Value =
            serde_json::from_slice(&std::fs::read(csharp.join("cases.json")).unwrap()).unwrap();
        assert_eq!(cases, cs_cases);
        distinct_target(&rust_target, &outer);
        distinct_target(&cs_artifacts, &outer);
        // The same resolved executable, cwd and environment factory for all C# children.
        let policy_bytes = std::fs::read(csharp.join("global.json")).unwrap();
        assert_eq!(policy_bytes, include_bytes!("../../../../global.json"));
        let policy: serde_json::Value = serde_json::from_slice(&policy_bytes).unwrap();
        let declared = policy["sdk"]["version"].as_str().unwrap();
        assert_eq!(declared, "10.0.112");
        assert_eq!(policy["sdk"]["rollForward"], "latestPatch");
        assert_eq!(policy["sdk"]["allowPrerelease"], false);
        let mut sdk = super::sdk_policy::dotnet_command(&csharp, dotnet.as_os_str());
        sdk.arg("--version");
        let observed = checked(&case, "sdk", &mut sdk, available_space(&csharp)); // NO reserve gate.
        super::sdk_policy::check_sdk_version(&observed.stdout, declared).unwrap();
        assert_eq!(hash_file(&dotnet), dotnet_identity);
        let mut lock = cargo(&rust);
        lock.args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(rust.join("Cargo.toml"));
        checked(&case, "rust-lock", &mut lock, available_space(&rust)); // Metadata preparation, NOT a build.
        let lock_identity = hash_file(&rust.join("Cargo.lock"));
        json(&case.join("rust-lock.json"), &lock_identity);
        fence(&root);
        generated_build(&case, "rust-build", &mut rust_build(&rust, &rust_target));
        assert_eq!(hash_file(&rust.join("Cargo.lock")), lock_identity);
        let rust_binary = rust_target.join("release").join(format!(
            "ferrule-filtered-counts{}",
            std::env::consts::EXE_SUFFIX
        ));
        let rust_identity = hash_file(&rust_binary);
        json(&case.join("rust-binary.json"), &rust_identity);
        for repeat in 1..=2 {
            assert_eq!(hash_file(&rust_binary), rust_identity);
            let mut command = Command::new(&rust_binary);
            command.current_dir(&rust);
            let actual = checked(
                &case,
                &format!("rust-run-{repeat}"),
                &mut command,
                available_space(&rust),
            ); // NO reserve gate.
            host_protocol(&actual, &cases);
            assert_eq!(hash_file(&rust_binary), rust_identity);
        }
        fence(&root);
        generated_build(
            &case,
            "csharp-build",
            &mut csharp_build(&csharp, &dotnet, &cs_artifacts),
        );
        assert_eq!(hash_file(&dotnet), dotnet_identity);
        let binary_dir = cs_artifacts.join("bin/Harness/release");
        let mut binaries = Vec::new();
        let mut entries = std::fs::read_dir(&binary_dir)
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if entry.file_type().unwrap().is_file() {
                binaries.push(hash_file(&entry.path()));
            }
        }
        json(
            &case.join("csharp-binaries.json"),
            &serde_json::json!(binaries),
        );
        for repeat in 1..=2 {
            for identity in &binaries {
                assert_eq!(
                    hash_file(Path::new(identity["path"].as_str().unwrap())),
                    *identity
                );
            }
            let mut command = super::sdk_policy::dotnet_command(&csharp, dotnet.as_os_str());
            command.arg(binary_dir.join("Harness.dll"));
            let actual = checked(
                &case,
                &format!("csharp-run-{repeat}"),
                &mut command,
                available_space(&csharp),
            ); // NO reserve gate.
            host_protocol(&actual, &cases);
            assert_eq!(hash_file(&dotnet), dotnet_identity);
            for identity in &binaries {
                assert_eq!(
                    hash_file(Path::new(identity["path"].as_str().unwrap())),
                    *identity
                );
            }
        }
        fence(&root);
    }
    assert_eq!(hash_file(&dotnet), dotnet_identity);
}

#[test]
fn generated_build_reserve_has_exact_injected_boundaries() {
    for free in [0, 1, RESERVE - 1] {
        assert_eq!(admission(Some(free)), Err(free));
    }
    for free in [RESERVE, RESERVE + 1] {
        assert_eq!(admission(Some(free)), Ok(()));
    }
    assert_eq!(admission(None), Ok(()));
}

#[test]
fn generated_build_admission_does_not_call_the_build_callback_below_the_floor() {
    let attempts = std::cell::Cell::new(0);
    let invoke = |free| admitted(Some(free), || attempts.set(attempts.get() + 1));
    assert_eq!(invoke(RESERVE - 1), Err(RESERVE - 1));
    assert_eq!(attempts.get(), 0);
    assert_eq!(invoke(RESERVE), Ok(()));
    assert_eq!(attempts.get(), 1);
}

#[test]
fn sdk_build_and_run_share_the_same_command_identity_and_environment() {
    let cwd = Path::new("/construction-only-count-csharp");
    let executable = Path::new("/construction-only-resolved-dotnet");
    let sdk = super::sdk_policy::dotnet_command(cwd, executable.as_os_str());
    let build = csharp_build(cwd, executable, &cwd.join("artifacts"));
    let run = super::sdk_policy::dotnet_command(cwd, executable.as_os_str());
    for command in [&build, &run] {
        assert_eq!(sdk.get_program(), command.get_program());
        assert_eq!(sdk.get_current_dir(), command.get_current_dir());
        assert_eq!(
            sdk.get_envs().collect::<Vec<_>>(),
            command.get_envs().collect::<Vec<_>>()
        );
    }
    for observed in [
        b"10.0.113\n".as_slice(),
        b"10.0.200\n",
        b"10.0.112-preview\n",
        b"\xff",
    ] {
        assert!(super::sdk_policy::check_sdk_version(observed, "10.0.112").is_err());
    }
    assert!(super::sdk_policy::check_sdk_version(b"10.0.112\n", "10.0.112").is_ok());
}

#[test]
fn rust_build_uses_only_command_line_target_job_and_profile_settings() {
    let cwd = Path::new("/construction-only-count-rust");
    let target = Path::new("/construction-only-generated-target");
    let command = rust_build(cwd, target);
    let arguments = command
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for required in [
        "build",
        "--release",
        "--offline",
        "--locked",
        "-j",
        "1",
        "--target-dir",
    ] {
        assert!(arguments.iter().any(|argument| argument == required));
    }
    let target_argument = arguments
        .iter()
        .position(|argument| argument == "--target-dir")
        .unwrap();
    assert_eq!(
        std::path::Path::new(&arguments[target_argument + 1]),
        target
    );
    for (name, value) in command.get_envs() {
        if name.to_string_lossy().starts_with("CARGO_") {
            assert!(value.is_none(), "do not export Cargo build configuration");
        }
    }
}

#[test]
fn receipt_capture_at_low_space_is_not_generated_build_admission() {
    let base = std::env::var_os("FERRULE_FILTERED_COUNT_POLICY_RECEIPT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join(format!(
        "ferrule_count_receipt_control_{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.current_dir(&root).arg("--list");
    let actual = checked(&root, "list", &mut command, Some(1));
    assert_eq!(
        std::fs::read(root.join("receipts/list/stdout.original")).unwrap(),
        actual.stdout
    );
    assert_eq!(
        std::fs::read(root.join("receipts/list/stderr.original")).unwrap(),
        actual.stderr
    );
    eprintln!("retained count policy control receipts: {}", root.display());
}
