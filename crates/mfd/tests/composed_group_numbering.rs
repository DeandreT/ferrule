//! #1306: imported starting-boundary groups inside optional nested repetitions.
//! These independently authored complete oracles follow Ferrule's documented
//! starting-boundary and positional-number contracts, not vendor execution.
//! Flat grouping, ordinary nested numbering and authored Basic Projects do not
//! establish this imported composition. No global counter identity is involved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fgn_{name}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        host_policy::preflight_environment(&path)?;
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if host_policy::keep_artifacts() {
            eprintln!("retained generated host artifacts at {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/composed-group-numbering")
        .join(name)
}

fn project() -> mapping::Project {
    let imported = mfd::import_with_profile(
        &fixture("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(mfd::assess_import(&imported).executable);
    let issues = engine::validate(&imported.project);
    assert!(issues.is_empty(), "{issues:?}");
    imported.project
}

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("fixtures/composed-group-numbering/cases.json")).unwrap()
}

#[test]
fn imported_group_and_number_nodes_belong_to_the_nested_row_collection() {
    let project = project();
    let [batch] = project.root.children.as_slice() else {
        panic!("one Batch scope must drive the complete output");
    };
    assert_eq!(batch.target_field, "Batch");
    assert_eq!(batch.source().unwrap().unwrap(), ["Batch"]);
    assert!(batch.group_starting_with.is_none());
    let [group] = batch.children.as_slice() else {
        panic!("one grouped child must remain inside its Batch");
    };
    assert_eq!(group.target_field, "Group");
    // Scope selectors are relative to their enclosing Batch; graph frame pins
    // below retain the full endpoint-local Batch/Row identity.
    assert_eq!(group.source().unwrap().unwrap(), ["Row"]);
    assert!(group.group_by.is_none());
    assert!(group.group_adjacent_by.is_none());
    assert!(group.group_ending_with.is_none());
    assert!(group.group_into_blocks.is_none());
    assert!(matches!(
        &project.graph.nodes[&group.group_starting_with.unwrap()],
        Node::SourceField { path, frame }
            if path == &["Start"] && frame.as_deref() == Some(&["Batch".into(), "Row".into()])
    ));
    let positions: Vec<_> = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::Position { collection } => Some(collection.as_slice()),
            _ => None,
        })
        .collect();
    // An unqualified Position is the receiving scope's ordinal, not the outer
    // Batch position or the original row index. Its sole consumer is Number.
    assert_eq!(positions, [&[] as &[String]]);
    let binding = |name: &str| {
        group
            .bindings
            .iter()
            .find(|b| b.target_field == name)
            .unwrap()
            .node
    };
    assert!(matches!(
        &project.graph.nodes[&binding("First")],
        Node::SourceField { path, frame }
            if path == &["Value"] && frame.as_deref() == Some(&["Batch".into(), "Row".into()])
    ));
    assert!(matches!(
        &project.graph.nodes[&binding("Count")],
        Node::Aggregate { function: mapping::AggregateOp::Count, collection, value, expression: None, arg: None }
            if collection == &["Row"] && value.is_empty()
    ));
    let call = |node, name| match &project.graph.nodes[&node] {
        Node::Call { function, args } if function == name => args.as_slice(),
        other => panic!("expected {name}, got {other:?}"),
    };
    let [start, offset] = call(binding("Number"), "add") else {
        panic!("number addition")
    };
    let [zero_based, increment] = call(*offset, "multiply") else {
        panic!("number increment")
    };
    let [position, one] = call(*zero_based, "subtract") else {
        panic!("one-based position")
    };
    for (id, wanted) in [(*start, 3), (*increment, 4), (*one, 1)] {
        assert!(
            matches!(&project.graph.nodes[&id], Node::Const { value: Value::Int(actual) } if *actual == wanted)
        );
    }
    assert!(
        matches!(&project.graph.nodes[position], Node::Position { collection } if collection.is_empty())
    );
}

fn assert_complete_cases(project: &mapping::Project) {
    let cases = cases();
    // Reuse the same imported graph across different inputs, then revisit earlier
    // inputs after empty and populated calls. Numbering must be invocation-local.
    for index in [0, 1, 2, 3, 4, 2, 0, 3, 1, 4] {
        let case = &cases[index];
        let source = format_xml::from_str(case["xml"].as_str().unwrap(), &project.source).unwrap();
        let typed_source =
            format_json::from_str(&case["input"].to_string(), &project.source).unwrap();
        assert_eq!(source, typed_source, "{}: physical input", case["id"]);
        let before = source.clone();
        let expected =
            format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
        let actual = engine::run(project, &source).unwrap();
        assert_eq!(actual, expected, "{}: full document", case["id"]);
        assert_eq!(source, before, "{}: source was changed", case["id"]);
    }
}

#[test]
fn complete_documents_preserve_empty_batches_boundaries_and_fresh_numbering() {
    assert_complete_cases(&project());
}

#[test]
fn malformed_boolean_fails_at_input_scalar_parsing_not_group_evaluation() {
    let project = project();
    let input = "<Input><Batch><Label>invalid</Label><Row><Value>x</Value><Start>yes</Start></Row></Batch></Input>";
    let error = format_xml::from_str(input, &project.source).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot parse `yes` as Bool for element `Start`"
    );
    assert!(
        matches!(
            &error,
            format_xml::XmlFormatError::ScalarParse { name, ty, value }
                if name == "Start" && *ty == ScalarType::Bool && value == "yes"
        ),
        "{error:?}"
    );
}

#[test]
fn missing_boundary_predicate_is_an_executable_refusal_not_an_ignored_control() {
    let directory = TempDir::new("missing_predicate").unwrap();
    for name in ["source.xsd", "target.xsd"] {
        std::fs::copy(fixture(name), directory.0.join(name)).unwrap();
    }
    let design = std::fs::read_to_string(fixture("mapping.mfd")).unwrap();
    let edge = "  <edge from=\"14\" to=\"31\"/>\n";
    assert_eq!(design.matches(edge).count(), 1);
    let path = directory.0.join("mapping.mfd");
    std::fs::write(&path, design.replace(edge, "")).unwrap();
    let imported = mfd::import(&path).unwrap();
    let warning = "group-starting-with feeding `Batch/Group` has a missing or unsupported predicate; iteration skipped";
    assert_eq!(imported.warnings, [warning]);
    let report = mfd::assess_import(&imported);
    assert!(!report.executable);
    assert!(report.issues.iter().any(|issue| {
        issue.kind == mfd::ImportIssueKind::ImportWarning && issue.message == warning
    }));
    assert!(imported.project.root.children[0].children.is_empty());
    match mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) {
        Err(mfd::MfdError::IncompatibleImport(actual)) => assert_eq!(*actual, report),
        Err(other) => panic!("expected the precise incompatibility report: {other}"),
        Ok(_) => panic!("missing grouping predicate was admitted for execution"),
    }
}

// Reuse the maintained host target isolation and command/source-retention
// harness without changing its shared implementation. Its small policy tests
// also run here; neither module starts generated builds in ordinary tests.
#[path = "../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
#[allow(dead_code)]
#[path = "../../cli/tests/code_generation/host_policy.rs"]
mod host_policy;
use dotnet_temp::DotnetTemp;

trait CommandOutputExt {
    fn isolated_output(&mut self) -> std::io::Result<std::process::Output>;
}

impl CommandOutputExt for std::process::Command {
    fn isolated_output(&mut self) -> std::io::Result<std::process::Output> {
        self.output()
    }
}

fn artifacts() -> (codegen::ArtifactSet, codegen::ArtifactSet) {
    let program = codegen::lower(&project()).unwrap();
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-generated-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()
                    .unwrap()
                    .display()
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let csharp = codegen_csharp::emit(&program).unwrap();
    (rust, csharp)
}

#[test]
fn imported_composition_lowers_and_emits_both_existing_backends() {
    let (rust, csharp) = artifacts();
    assert!(!rust.files().is_empty());
    assert!(!csharp.files().is_empty());
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn checked(
    command: &mut std::process::Command,
    directory: &Path,
    label: &str,
) -> std::process::Output {
    let sources = [
        "rust",
        "csharp",
        "cases.json",
        "source-schema.json",
        "target-schema.json",
        "global.json",
        "NuGet.Config",
    ]
    .map(PathBuf::from);
    let output =
        host_policy::recorded_output_with_sources(command, directory, label, &sources).unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn csharp_command(root: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(
        std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into()),
    );
    command
        .current_dir(root)
        .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .dotnet_temp(root);
    command
}

fn csharp_sdk_version_command(root: &Path) -> std::process::Command {
    let mut command = csharp_command(root);
    command.arg("--version");
    command
}

fn csharp_release_command(root: &Path) -> std::process::Command {
    let mut command = csharp_command(root);
    command.args([
        "run",
        "--project",
        "csharp/Host/Host.csproj",
        "--configuration",
        "Release",
        "--disable-build-servers",
        "-p:BuildInParallel=false",
        "-p:UseSharedCompilation=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
        "--",
        "cases.json",
        "source-schema.json",
        "target-schema.json",
    ]);
    command
}

#[test]
fn csharp_release_command_passes_only_the_expected_host_arguments() {
    use std::ffi::OsStr;

    let directory = TempDir::new("command").unwrap();
    let command = csharp_release_command(&directory.0);
    let arguments: Vec<_> = command.get_args().collect();
    assert_eq!(arguments[0], "run");
    let separator = arguments.iter().position(|arg| *arg == "--").unwrap();
    for control in [
        "--disable-build-servers",
        "-p:BuildInParallel=false",
        "-p:UseSharedCompilation=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
    ] {
        assert_eq!(
            arguments[..separator]
                .iter()
                .filter(|arg| **arg == control)
                .count(),
            1,
            "missing or duplicated build control: {control}"
        );
    }
    assert_eq!(
        &arguments[separator + 1..],
        ["cases.json", "source-schema.json", "target-schema.json"].map(OsStr::new)
    );
    assert_eq!(
        arguments.iter().filter(|arg| **arg == "cases.json").count(),
        1
    );
    // dotnet run does not accept this MSBuild switch: it can become an app
    // argument before the cases path. DotnetTemp already disables node reuse.
    assert!(arguments.iter().all(|arg| {
        !arg.to_string_lossy()
            .to_ascii_lowercase()
            .contains("nodereuse")
    }));
    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| *key == "MSBUILDDISABLENODEREUSE"),
        Some((OsStr::new("MSBUILDDISABLENODEREUSE"), Some(OsStr::new("1"))))
    );
}

#[test]
fn csharp_sdk_probe_uses_the_release_executable_directory_and_isolation_policy() {
    use std::ffi::OsStr;

    let directory = TempDir::new("sdk_command").unwrap();
    let release = csharp_release_command(&directory.0);
    let version = csharp_sdk_version_command(&directory.0);
    assert_eq!(version.get_program(), release.get_program());
    assert_eq!(version.get_current_dir(), Some(directory.0.as_path()));
    assert_eq!(version.get_current_dir(), release.get_current_dir());
    assert_eq!(
        version.get_args().collect::<Vec<_>>(),
        [OsStr::new("--version")]
    );
    assert_eq!(
        version.get_envs().collect::<Vec<_>>(),
        release.get_envs().collect::<Vec<_>>()
    );
    let environment = release
        .get_envs()
        .collect::<std::collections::BTreeMap<_, _>>();
    for (key, value) in [
        ("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false"),
        ("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1"),
        ("DOTNET_NOLOGO", "1"),
        ("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
        ("DOTNET_CLI_USE_MSBUILD_SERVER", "0"),
        ("MSBUILDDISABLENODEREUSE", "1"),
        ("UseSharedCompilation", "false"),
    ] {
        assert_eq!(environment[OsStr::new(key)], Some(OsStr::new(value)));
    }
    let home = directory.0.join(".dotnet-home");
    assert_eq!(
        environment[OsStr::new("DOTNET_CLI_HOME")],
        Some(home.as_os_str())
    );
    let temporary = directory.0.join(".tmp");
    for key in ["TMPDIR", "TEMP", "TMP"] {
        assert_eq!(environment[OsStr::new(key)], Some(temporary.as_os_str()));
    }
}

#[cfg(windows)]
fn saved_user_path() -> Vec<u8> {
    let output = std::process::Command::new("reg.exe")
        .args(["query", r"HKCU\Environment", "/v", "Path"])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
#[ignore = "compiles generated Rust and C# Release hosts; requires Cargo and the pinned .NET SDK"]
fn generated_release_hosts_match_complete_documents_and_fresh_invocations() {
    let directory = TempDir::new("rel").unwrap();
    let root = directory.0.canonicalize().unwrap();
    let (rust, csharp) = artifacts();
    write_artifacts(&root.join("rust"), &rust);
    write_artifacts(&root.join("csharp"), &csharp);
    let project = project();
    for (name, schema) in [
        ("source-schema.json", project.source),
        ("target-schema.json", project.target),
    ] {
        std::fs::write(root.join(name), serde_json::to_vec(&schema).unwrap()).unwrap();
    }
    std::fs::copy(fixture("cases.json"), root.join("cases.json")).unwrap();
    host_policy::name_generated_host(&root.join("rust"), "mfd-composed-group-numbering").unwrap();
    let manifest_path = root.join("rust/Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(
        manifest_path,
        manifest.replace("[dependencies]", "[dependencies]\nserde_json = \"1\""),
    )
    .unwrap();
    std::fs::write(
        root.join("rust/src/main.rs"),
        include_str!("fixtures/composed-group-numbering/hosts/main.rs.txt"),
    )
    .unwrap();
    std::fs::write(
        root.join("global.json"),
        include_str!("../../../global.json"),
    )
    .unwrap();
    std::fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear/></packageSources></configuration>",
    )
    .unwrap();
    // Keep target/job/profile settings in arguments, preserving cross-lane
    // sccache reuse. The maintained harness selects and locks the host target;
    // its recorder does not reconfigure an explicitly resolved Cargo binary.
    // Preserve the Cargo basename: resolving a rustup proxy symlink can turn
    // its invocation into `rustup` instead of `cargo`, particularly on Windows.
    let cargo = Path::new(env!("CARGO"));
    assert!(
        cargo.is_absolute(),
        "Cargo must supply its absolute executable path"
    );
    host_policy::with_cargo_target(&root, |target| {
        checked(
            std::process::Command::new(cargo)
                .current_dir(root.join("rust"))
                .args(["run", "--release", "--offline", "--target-dir"])
                .arg(target)
                .args([
                    "-j",
                    "1",
                    "--config",
                    "profile.release.debug=0",
                    "--config",
                    "profile.release.incremental=false",
                    "--config",
                    "profile.release.strip=\"symbols\"",
                ])
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env("RUSTFLAGS", "-D warnings")
                .args([
                    "--",
                    "../cases.json",
                    "../source-schema.json",
                    "../target-schema.json",
                ]),
            &root,
            "rust-release",
        );
    })
    .unwrap();
    let host = root.join("csharp/Host");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(
        host.join("Host.csproj"),
        include_str!("fixtures/composed-group-numbering/hosts/Host.csproj"),
    )
    .unwrap();
    std::fs::write(
        host.join("Program.cs"),
        include_str!("fixtures/composed-group-numbering/hosts/Program.cs.txt"),
    )
    .unwrap();
    #[cfg(windows)]
    let before = saved_user_path();
    let sdk = checked(
        &mut csharp_sdk_version_command(&root),
        &root,
        "csharp-sdk-version",
    );
    eprintln!(
        "C# host SDK: {}",
        String::from_utf8_lossy(&sdk.stdout).trim()
    );
    checked(&mut csharp_release_command(&root), &root, "csharp-release");
    #[cfg(windows)]
    assert_eq!(
        before,
        saved_user_path(),
        "isolated .NET home changed User PATH"
    );
}
