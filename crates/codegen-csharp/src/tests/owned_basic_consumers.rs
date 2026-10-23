//! Builds and runs the public Basic C# consumers that `owned_basic` retains,
//! without an external campaign. The crate's own test binary regenerates them
//! into a fresh temporary directory through
//! `FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR`; each consumer then builds against
//! its emitted library and checks its frozen expectations, failing on any
//! difference. The directory is removed unless
//! `FERRULE_CODEGEN_KEEP_ARTIFACTS=1`.
//!
//! The private and checked hosts (`FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR`,
//! `FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR`) are not built here: they call
//! `FerruleBasicSourceCandidate` identity members of a historical runtime
//! that the current runtime no longer exposes.
use crate::dotnet_temp::DotnetTemp;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bytes of a failing command's output quoted in the test failure.
const EXCERPT: usize = 16 * 1024;
/// The consumer project's assembly, under its `--artifacts-path`.
const ASSEMBLY: &str = "bin/PublicConsumer/release/Ferrule.PublicBasicConsumer.dll";

struct Run(PathBuf);

impl Drop for Run {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Some("1".as_ref()) {
            eprintln!("retained public Basic consumers at {}", self.0.display());
        } else {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn excerpt(output: &Output) -> String {
    let tail = |bytes: &[u8]| {
        String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(EXCERPT)..]).into_owned()
    };
    format!(
        "{:?}\n{}{}",
        output.status,
        tail(&output.stdout),
        tail(&output.stderr)
    )
}

/// A `dotnet` command whose CLI home, package cache and temporary files stay
/// inside `sandbox`.
fn dotnet(sandbox: &Path) -> Command {
    let mut command = Command::new("dotnet");
    command
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_CLI_HOME", sandbox.join(".dotnet-home"))
        .env("NUGET_PACKAGES", sandbox.join(".nuget"))
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .dotnet_temp(sandbox);
    command
}

fn build_and_run(root: &Path, sandbox: &Path, label: &str) -> Result<(), String> {
    let artifacts = root.join("build").join(label);
    let build = dotnet(sandbox)
        .args([
            "build",
            "--configuration",
            "Release",
            "--nologo",
            "--verbosity",
            "quiet",
            "-nodeReuse:false",
            "-p:UseSharedCompilation=false",
            "--artifacts-path",
        ])
        .arg(&artifacts)
        .arg(
            root.join("consumers")
                .join(label)
                .join("consumer/PublicConsumer.csproj"),
        )
        .output()
        .map_err(|error| format!("{label} build: {error}"))?;
    if !build.status.success() {
        return Err(format!("{label} build: {}", excerpt(&build)));
    }
    let run = dotnet(sandbox)
        .arg(artifacts.join(ASSEMBLY))
        .output()
        .map_err(|error| format!("{label} run: {error}"))?;
    if !run.status.success() {
        return Err(format!("{label} run: {}", excerpt(&run)));
    }
    Ok(())
}

#[test]
#[ignore = "builds and runs every retained public Basic C# consumer; requires the .NET 10 SDK"]
fn public_basic_consumers_build_and_check_their_expectations() {
    let run = Run(std::env::temp_dir().join(format!(
        "ferrule_csharp_public_basic_consumers_{}",
        std::process::id()
    )));
    let _ = fs::remove_dir_all(&run.0);
    let consumers = run.0.join("consumers");
    fs::create_dir_all(&consumers).expect("consumer directory");
    // The ignored runner is not selected again: the child runs plain tests only.
    let generated = Command::new(std::env::current_exe().expect("test binary"))
        .args(["tests::owned_basic::", "--test-threads=2"])
        .env("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR", &consumers)
        .env_remove("FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR")
        .env_remove("FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR")
        .output()
        .expect("regenerate consumers");
    assert!(generated.status.success(), "{}", excerpt(&generated));
    let mut labels = fs::read_dir(&consumers)
        .expect("retained consumers")
        .map(|entry| {
            entry
                .expect("consumer entry")
                .file_name()
                .into_string()
                .expect("UTF-8 consumer label")
        })
        .collect::<Vec<_>>();
    labels.sort();
    assert!(!labels.is_empty(), "no public consumer was retained");

    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 4).clamp(1, 4));
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let (next, failures, labels, root) = (&next, &failures, &labels, &run.0);
            scope.spawn(move || {
                // One package cache per worker keeps concurrent restores apart.
                let sandbox = root.join(format!("sandbox{worker}"));
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(label) = labels.get(index) else {
                        break;
                    };
                    if let Err(error) = build_and_run(root, &sandbox, label)
                        && let Ok(mut failures) = failures.lock()
                    {
                        failures.push(error);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("consumer workers");
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {} public Basic consumers failed:\n{}",
        failures.len(),
        labels.len(),
        failures.join("\n\n")
    );
}
