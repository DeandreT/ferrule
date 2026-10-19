fn tool_route(name: &str) -> TestResult<Json> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let invocation = std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH required")?)
        .map(|path| path.join(&filename))
        .find(|path| path.is_file())
        .ok_or("bound tool missing")?;
    let invocation = if invocation.is_absolute() {
        invocation
    } else {
        std::env::current_dir()?.join(invocation)
    };
    let resolved = invocation.canonicalize()?;
    let bytes = fs::read(&resolved)?;
    // Preserve argv[0] for rustup's cargo/rustc proxies while binding the actual
    // resolved executable. Executing canonical rustup as `cargo build` is wrong.
    Ok(
        json!({"invocation_path":invocation,"resolved_path":resolved,"bytes":bytes.len(),"sha256":digest(&bytes)}),
    )
}
fn reject_execution_routes(values: &BTreeMap<String, String>) -> TestResult<()> {
    for name in values.keys() {
        if (name.starts_with("CARGO_TARGET_") && name.ends_with("_RUNNER"))
            || ["CARGO_BUILD_TARGET", "FERRULE_CODEGEN_HOST_TARGET_DIR"].contains(&name.as_str())
        {
            return Err(format!(
                "{name} must be unset: dedicated native build and direct host execution only"
            )
            .into());
        }
    }
    Ok(())
}
fn child_environment(directory: &Path, dotnet: bool) -> TestResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "WINDIR",
        "TEMP",
        "TMP",
        "TMPDIR",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_HOME",
        "DOTNET_ROOT",
    ] {
        if let Some(value) = std::env::var_os(name) {
            values.insert(name.to_owned(), value.to_string_lossy().into_owned());
        }
    }
    values.insert("RUST_BACKTRACE".into(), "1".into());
    for name in ["TEMP", "TMP", "TMPDIR"] {
        values.insert(
            name.into(),
            directory.join(".runtime-temp").display().to_string(),
        );
    }
    if dotnet {
        for (name, value) in [
            ("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false".to_owned()),
            (
                "DOTNET_CLI_HOME",
                directory.join(".dotnet-home").display().to_string(),
            ),
            ("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1".into()),
            ("DOTNET_CLI_TELEMETRY_OPTOUT", "1".into()),
            ("DOTNET_NOLOGO", "1".into()),
            ("DOTNET_PROCESSOR_COUNT", "2".into()),
            (
                "NUGET_PACKAGES",
                std::env::var_os("NUGET_PACKAGES")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".nuget/packages"))
                    })
                    .unwrap_or_else(|| directory.join(".nuget-packages"))
                    .display()
                    .to_string(),
            ),
        ] {
            values.insert(name.into(), value);
        }
    }
    Ok(values)
}
fn configuration_rows(cwd: &Path) -> TestResult<Json> {
    let mut paths = std::collections::BTreeSet::new();
    for ancestor in cwd.ancestors() {
        for name in [
            ".cargo/config",
            ".cargo/config.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "global.json",
            "NuGet.Config",
            "nuget.config",
            "Directory.Build.props",
            "Directory.Build.targets",
            "Directory.Packages.props",
        ] {
            paths.insert(ancestor.join(name));
        }
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(home) = cargo_home {
        for name in ["config", "config.toml"] {
            paths.insert(home.join(name));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        paths.insert(home.join(".nuget/NuGet/NuGet.Config"));
        paths.insert(home.join(".config/NuGet/NuGet.Config"));
    }
    let mut rows = Vec::new();
    for path in paths {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                rows.push(json!({"path":path,"present":false}))
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("configuration must be regular, not redirected".into());
                }
                let bytes = fs::read(&path)?;
                rows.push(
                    json!({"path":path,"present":true,"bytes":bytes.len(),"sha256":digest(&bytes)}),
                );
            }
        }
    }
    Ok(json!(rows))
}
fn execution_route_binding() -> TestResult<Json> {
    let inherited: BTreeMap<String, String> = std::env::vars().collect();
    reject_execution_routes(&inherited)?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    let execution = bound_execution_root()?;
    let environment = [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "WINDIR",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_HOME",
        "DOTNET_ROOT",
        "NUGET_PACKAGES",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            std::env::var_os(name).map(|v| v.to_string_lossy().into_owned()),
        )
    })
    .collect::<BTreeMap<_, _>>();
    Ok(
        json!({"version":2,"rust_policy":"cargo build JSON artifact + direct absolute executable; never cargo run",
        "csharp_policy":"bound absolute dotnet + exact fresh Host.dll; no dotnet run",
        "native_cwd":cwd,"native_configuration":configuration_rows(&cwd)?,
        "prospective_execution_root":execution,"execution_configuration":configuration_rows(&execution)?,"environment_selectors":environment,
        "cargo":tool_route("cargo")?,"rustc":tool_route("rustc")?,"dotnet":tool_route("dotnet")?,
        "child_environment_allowlist":["PATH","HOME","USERPROFILE","SYSTEMROOT","WINDIR","TEMP","TMP","TMPDIR","RUSTUP_HOME","RUSTUP_TOOLCHAIN","CARGO_HOME","DOTNET_ROOT"],
        "runner_environment":"all CARGO_TARGET_*_RUNNER variables rejected, including empty values",
        "config_runner":"bound for custody, unambiguously bypassed by direct OS execution",
        "ambient_runtime_injection":"env_clear; no LD_PRELOAD, DOTNET_STARTUP_HOOKS, profiler, wrapper or inherited flags"}),
    )
}
fn bound_execution_root() -> TestResult<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os("FERRULE_BASIC_STARTING443_BOUND_EXECUTION_ROOT")
            .ok_or("independently reviewed future execution root required")?,
    );
    if !path.is_absolute()
        || path.starts_with(root()?)
        || !path.parent().is_some_and(Path::is_dir)
        || path.file_name().is_none()
    {
        return Err("external absolute execution root with existing parent required".into());
    }
    if path
        .parent()
        .unwrap()
        .canonicalize()?
        .join(path.file_name().unwrap())
        != path
        || fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("execution root identity cannot be redirected".into());
    }
    Ok(path)
}
fn bound_file(path: &Path) -> TestResult<Json> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("regular unredirected host file required".into());
    }
    let path = path.canonicalize()?;
    let bytes = fs::read(&path)?;
    Ok(json!({"path":path,"bytes":bytes.len(),"sha256":digest(&bytes)}))
}
fn command_with_bound_environment(
    program: &Path,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(environment).current_dir(cwd);
    command
}
fn rust_artifact_path(stdout: &[u8], manifest: &Path, target: &Path) -> TestResult<PathBuf> {
    let mut artifacts = Vec::new();
    for line in std::str::from_utf8(stdout)?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let message: Json = serde_json::from_str(line)?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "starting443-public-host"
            && message["target"]["kind"] == json!(["bin"])
            && message["profile"]["test"] == false
        {
            if message["manifest_path"] != json!(manifest) {
                return Err("Rust artifact from wrong source manifest".into());
            }
            artifacts.push(PathBuf::from(
                message["executable"]
                    .as_str()
                    .ok_or("actual host executable missing")?,
            ));
        }
    }
    if artifacts.len() != 1 {
        return Err(
            "exactly one actual Rust executable artifact required; no zero/duplicate success"
                .into(),
        );
    }
    let expected = target.join("debug").join(if cfg!(windows) {
        "starting443-public-host.exe"
    } else {
        "starting443-public-host"
    });
    if artifacts[0] != expected {
        return Err("Rust artifact outside the fresh native target/default profile".into());
    }
    Ok(expected)
}
fn record_bound_process(
    evidence: &Evidence,
    label: &str,
    command: &mut Command,
    sources: &[PathBuf],
    inputs: &mut PreparedInputFence,
) -> TestResult<Output> {
    inputs.check(evidence, &format!("{label}-before"), false)?;
    let prepared: Json = serde_json::from_slice(&fs::read(evidence.0.join(SUCCESS))?)?;
    if prepared["commands"] != command_runtime_binding()? {
        return Err(
            "unreviewed tool, environment selector or configuration changed before process launch"
                .into(),
        );
    }
    if prepared["sources"] != serde_json::to_value(source_bindings()?)? {
        return Err(
            "complete repository dependency source pathset/body changed before process".into(),
        );
    }
    let program = PathBuf::from(command.get_program());
    if !program.is_absolute() {
        return Err("every process must use its bound absolute invocation path".into());
    }
    let resolved = program.canonicalize()?;
    let bytes = fs::read(&resolved)?;
    let configuration = configuration_rows(&evidence.0)?;
    evidence.json(&format!("{label}-COMPLETE-EXECUTION-ROUTE.json"),&json!({"invocation_path":program,"resolved_path":resolved,
        "bytes":bytes.len(),"sha256":digest(&bytes),"cwd":command.get_current_dir(),"argv":command.get_args().collect::<Vec<_>>(),
        "explicit_environment":command.get_envs().collect::<BTreeMap<_,_>>(),"environment_policy":"env_clear + explicit allowlist",
        "configuration":configuration}))?;
    let output = host_policy::recorded_output_with_sources(command, &evidence.0, label, sources);
    // Check even on nonzero exit/spawn error. The original process evidence is
    // retained first; a copied current source list is never the expected pathset.
    inputs.check(
        evidence,
        &format!("{label}-after"),
        label == "starting443-rust-build",
    )?;
    if prepared["sources"] != serde_json::to_value(source_bindings()?)? {
        return Err(
            "complete repository dependency source pathset/body changed across process".into(),
        );
    }
    if program.canonicalize()? != resolved
        || fs::read(&resolved)? != bytes
        || configuration_rows(&evidence.0)? != configuration
    {
        return Err(
            "resolved process executable or reviewed configuration changed across process".into(),
        );
    }
    Ok(output?)
}
fn make_host_contract(
    evidence: &Evidence,
    label: &str,
    engine: &str,
    program: &Json,
    assembly: Option<&Json>,
    argv: &[PathBuf],
    environment: &BTreeMap<String, String>,
    output: &Path,
) -> TestResult<Json> {
    match fs::symlink_metadata(output) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(_) => {
            return Err(
                "host output must be absent before invocation; never reuse stale products".into(),
            );
        }
    }
    let run_id = digest(
        format!(
            "{}|{}|{}|{}|{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            output.display(),
            program["sha256"],
            digest(&fs::read(evidence.0.join(SUCCESS))?)
        )
        .as_bytes(),
    );
    let mut identity = json!({"program":program["path"],"cwd":evidence.0,"argv":argv,"environment":environment,"run_id":run_id});
    if let Some(assembly) = assembly {
        identity["assembly"] = assembly["path"].clone();
    }
    Ok(
        json!({"version":2,"label":label,"engine":engine,"run_id":run_id,"program":program,"assembly":assembly,
        "host_identity":identity,"output_directory":output,"configuration":configuration_rows(&evidence.0)?,
        "runtime_source_path":runtime_path()?,"complete_source_binding":source_bindings()?,
        "ordinary_input":include_str!("../../../../../conformance/basic-owned-group-starting-with/legacy-input.json"),
        "ordinary_expected":include_str!("../../../../../conformance/basic-owned-group-starting-with/legacy-expected.json")}),
    )
}
