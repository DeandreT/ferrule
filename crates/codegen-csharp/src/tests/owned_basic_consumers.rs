//! Builds and runs the C# hosts that `owned_basic` retains, without an
//! external campaign. The crate's own test binary regenerates them into a
//! fresh temporary directory through their retention variables; each host then
//! builds and runs, checking its frozen expectations and failing on any
//! difference. The directory is removed unless
//! `FERRULE_CODEGEN_KEEP_ARTIFACTS=1`.
//!
//! Public consumers build against their emitted `Ferrule.Generated` library.
//! The private and checked hosts read `internal` runtime members such as
//! `FerruleBasicSourceCandidate.SourceId`, so each compiles into one assembly
//! with the runtime sources, as the private renderer's mapping does.
use crate::dotnet_temp::DotnetTemp;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bytes of a failing command's output quoted in the test failure.
const EXCERPT: usize = 16 * 1024;
const PUBLIC: &str = "FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR";
const PRIVATE: &str = "FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR";
const CHECKED: &str = "FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR";

/// One family of retained hosts: the variables that retain them, the project
/// each one builds and the assembly it produces under `--artifacts-path`.
struct HostKind {
    name: &'static str,
    variables: &'static [&'static str],
    project: fn(&Path) -> PathBuf,
    assembly: &'static str,
}

const PUBLIC_CONSUMERS: HostKind = HostKind {
    name: "public Basic consumers",
    variables: &[PUBLIC],
    project: |host| host.join("consumer/PublicConsumer.csproj"),
    assembly: "bin/PublicConsumer/release/Ferrule.PublicBasicConsumer.dll",
};

const PRIVATE_HOSTS: HostKind = HostKind {
    name: "private and checked Basic hosts",
    variables: &[PRIVATE, CHECKED],
    project: private_project,
    assembly: "bin/Host/release/Host.dll",
};

struct Run(PathBuf);

impl Drop for Run {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Some("1".as_ref()) {
            eprintln!("retained Basic C# hosts at {}", self.0.display());
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

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A host project compiling the retained sources together with the runtime
/// sources, so the host may read the runtime's internal members.
fn private_project(host: &Path) -> PathBuf {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/csharp/Ferrule.Runtime")
        .canonicalize()
        .expect("runtime sources");
    let runtime = xml_escape(&runtime.display().to_string());
    let project = host.join("Host.csproj");
    fs::write(
        &project,
        format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization><NuGetAudit>false</NuGetAudit><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup><ItemGroup><Compile Include=\"*.cs\" /><Compile Include=\"{runtime}/**/*.cs\" Exclude=\"{runtime}/bin/**;{runtime}/obj/**\" /></ItemGroup></Project>\n"
        ),
    )
    .expect("private host project");
    project
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

fn build_and_run(kind: &HostKind, root: &Path, sandbox: &Path, label: &str) -> Result<(), String> {
    let artifacts = root.join("build").join(label);
    let project = (kind.project)(&root.join("hosts").join(label));
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
        .arg(project)
        .output()
        .map_err(|error| format!("{label} build: {error}"))?;
    if !build.status.success() {
        return Err(format!("{label} build: {}", excerpt(&build)));
    }
    let run = dotnet(sandbox)
        .arg(artifacts.join(kind.assembly))
        .output()
        .map_err(|error| format!("{label} run: {error}"))?;
    if !run.status.success() {
        return Err(format!("{label} run: {}", excerpt(&run)));
    }
    Ok(())
}

/// Every retained host label below `hosts`, as `<variable directory>/<host>`.
fn labels(hosts: &Path) -> Vec<String> {
    let mut labels = Vec::new();
    for family in fs::read_dir(hosts).expect("retained host families") {
        let family = family.expect("host family").file_name();
        for host in fs::read_dir(hosts.join(&family)).expect("retained hosts") {
            labels.push(format!(
                "{}/{}",
                family.to_string_lossy(),
                host.expect("host entry").file_name().to_string_lossy()
            ));
        }
    }
    labels.sort();
    labels
}

fn check_hosts(kind: &HostKind, tag: &str) {
    let run =
        Run(std::env::temp_dir().join(format!("ferrule_csharp_{tag}_{}", std::process::id())));
    let _ = fs::remove_dir_all(&run.0);
    let hosts = run.0.join("hosts");
    // The ignored runners are not selected again: the child runs plain tests only.
    let mut regenerate = Command::new(std::env::current_exe().expect("test binary"));
    regenerate.args(["tests::owned_basic::", "--test-threads=2"]);
    for variable in [PUBLIC, PRIVATE, CHECKED] {
        regenerate.env_remove(variable);
    }
    for variable in kind.variables {
        let directory = hosts.join(variable);
        fs::create_dir_all(&directory).expect("host directory");
        regenerate.env(variable, directory);
    }
    let generated = regenerate.output().expect("regenerate hosts");
    assert!(generated.status.success(), "{}", excerpt(&generated));
    let labels = labels(&hosts);
    assert!(!labels.is_empty(), "no {} were retained", kind.name);

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
                    if let Err(error) = build_and_run(kind, root, &sandbox, label)
                        && let Ok(mut failures) = failures.lock()
                    {
                        failures.push(error);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("host workers");
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {} {} failed:\n{}",
        failures.len(),
        labels.len(),
        kind.name,
        failures.join("\n\n")
    );
}

#[test]
#[ignore = "builds and runs every retained public Basic C# consumer; requires the .NET 10 SDK"]
fn public_basic_consumers_build_and_check_their_expectations() {
    check_hosts(&PUBLIC_CONSUMERS, "public_basic_consumers");
}

#[test]
#[ignore = "builds and runs every retained private and checked Basic C# host; requires the .NET 10 SDK"]
fn private_basic_hosts_build_and_check_their_expectations() {
    check_hosts(&PRIVATE_HOSTS, "private_basic_hosts");
}
