struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os(ENVIRONMENT).ok_or("fresh absolute preparation directory required")?,
        );
        if !path.is_absolute()
            || fs::symlink_metadata(&path).is_ok()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err(
                "fresh absent absolute preparation root with existing parent required".into(),
            );
        }
        fs::create_dir(&path)?;
        host_policy::preflight_environment(&path)?;
        eprintln!("STARTING443_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write_io(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.0.join(name);
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| io::Error::other("evidence parent"))?,
        )?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        self.write_io(name, bytes)?;
        Ok(())
    }
    fn json(&self, name: &str, value: &impl serde::Serialize) -> TestResult<()> {
        self.write(name, &serde_json::to_vec_pretty(value)?)
    }
    fn debug(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn error(&self, name: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut text = format!("Debug={error:#?}\nDisplay={error}\n");
        let mut cause = error.source();
        while let Some(error) = cause {
            text.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
            cause = error.source();
        }
        text.push_str("CAUSE_CHAIN_TERMINATED=None\n");
        self.write(name, text.as_bytes())
    }
    fn artifacts(&self, name: &str, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
        for file in artifacts.files() {
            self.write(&format!("{name}/{}", file.path.as_str()), &file.contents)?;
        }
        self.json(
            &format!("{name}-COMPLETE-ARTIFACT-INDEX.json"),
            &artifact_manifest(artifacts),
        )
    }
}
fn root() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace")?
        .to_owned())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hashes(files: ArtifactFiles) -> Vec<legacy_artifact_policy::ManifestRow> {
    files
        .into_iter()
        .map(|(path, bytes)| (path, bytes.len(), digest(&bytes)))
        .collect()
}
fn artifact_manifest(artifacts: &codegen::ArtifactSet) -> Vec<legacy_artifact_policy::ManifestRow> {
    artifacts
        .files()
        .iter()
        .map(|f| legacy_artifact_policy::row(f.path.as_str(), &f.contents))
        .collect()
}
// Campaign-local replacement for the parent glob import: every campaign
// preparation/binding/historical/binary scan uses exact component keys here.
// The shared collector and the nine reviewed helper bodies remain untouched.
fn artifact_files(root: &Path) -> TestResult<ArtifactFiles> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<String, Vec<u8>>,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> TestResult<()> {
        let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let key = campaign_path_key(path.strip_prefix(root)?)?;
            campaign_insert_path_set(seen, key.clone())?;
            let kind = entry.file_type()?;
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                return Err("artifact scan refuses redirected/nonregular input".into());
            }
            if kind.is_dir() {
                visit(root, &path, files, seen)?;
            } else {
                campaign_insert_path(files, key, fs::read(path)?)?;
            }
        }
        Ok(())
    }
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("artifact root must be one unredirected directory".into());
    }
    let mut files = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    visit(root, root, &mut files, &mut seen)?;
    Ok(files.into_iter().collect())
}
fn source_bindings() -> TestResult<Vec<legacy_artifact_policy::ManifestRow>> {
    let root = root()?;
    let mut files = Vec::new();
    // Conservative whole workspace source/input closure, not a crate subset.
    // .git is repository administration; the root target directory is output.
    // Compiler lanes use external targets/homes through unchanged host_policy.
    for entry in fs::read_dir(&root)? {
        let path = entry?.path();
        let name = campaign_path_key(Path::new(path.file_name().ok_or("root filename")?))?;
        if name == ".git" || name == "target" {
            continue;
        }
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err("source closure cannot silently follow root symlinks".into());
        }
        if path.is_dir() {
            for (relative, bytes) in artifact_files(&path)? {
                files.push((format!("{name}/{relative}"), bytes));
            }
        } else if path.is_file() {
            files.push((name, fs::read(path)?));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if files.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err("duplicate or unordered source closure".into());
    }
    Ok(hashes(files))
}
fn command_runtime_binding() -> TestResult<Json> {
    let search = std::env::var_os("PATH").ok_or("PATH required")?;
    let mut tools = Vec::new();
    // Binding is read-only: preparation does not invoke even --version.
    for name in ["cargo", "rustc", "dotnet"] {
        let filename = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        };
        let path = std::env::split_paths(&search)
            .map(|p| p.join(&filename))
            .find(|p| p.is_file())
            .ok_or_else(|| format!("missing command {name}"))?
            .canonicalize()?;
        let bytes = fs::read(&path)?;
        tools.push(json!({"name":name,"path":path,"bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    // Arch's cargo/rustc entries are rustup proxies. Bind the installed real
    // tool binaries and selector settings too, without executing a resolver or
    // adding an unreviewed TOML parser. Likewise dotnet is an SDK host.
    let mut delegated = Vec::new();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|p| p.join(".rustup")));
    if let Some(rustup) = rustup.filter(|p| p.is_dir()) {
        let settings = rustup.join("settings.toml");
        if settings.is_file() {
            delegated.push((settings.display().to_string(), fs::read(&settings)?));
        }
        let toolchains = rustup.join("toolchains");
        if toolchains.is_dir() {
            for entry in fs::read_dir(toolchains)? {
                let entry = entry?.path();
                if entry.is_dir() {
                    for name in ["cargo", "rustc", "rustdoc"] {
                        let path = entry.join("bin").join(if cfg!(windows) {
                            format!("{name}.exe")
                        } else {
                            name.into()
                        });
                        if path.is_file() {
                            delegated.push((path.display().to_string(), fs::read(path)?));
                        }
                    }
                }
            }
        }
    }
    let dotnet = tools
        .iter()
        .find(|tool| tool["name"] == "dotnet")
        .and_then(|tool| tool["path"].as_str())
        .map(PathBuf::from)
        .ok_or("bound dotnet host")?;
    if let Some(parent) = dotnet.parent() {
        let sdks = parent.join("sdk");
        if sdks.is_dir() {
            for entry in fs::read_dir(sdks)? {
                let entry = entry?.path();
                if entry.is_dir() {
                    for relative in ["MSBuild.dll", "dotnet.dll", "Roslyn/bincore/csc.dll"] {
                        let path = entry.join(relative);
                        if path.is_file() {
                            delegated.push((path.display().to_string(), fs::read(path)?));
                        }
                    }
                }
            }
        }
    }
    delegated.sort_by(|a, b| a.0.cmp(&b.0));
    let selectors = [
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_HOME",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "DOTNET_ROOT",
        "DOTNET_ROLL_FORWARD",
        "DOTNET_MSBUILD_SDK_RESOLVER_SDKS_DIR",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()),
        )
    })
    .collect::<BTreeMap<_, _>>();
    reject_compiler_overrides(&selectors)?;
    let native = std::env::current_exe()?.canonicalize()?;
    let bytes = fs::read(&native)?;
    Ok(
        json!({"tools":tools,"delegated_tool_binaries_and_settings":hashes(delegated),"selectors":selectors,"native_test_binary":{"path":native,"bytes":bytes.len(),"sha256":digest(&bytes)},"path":search.to_string_lossy(),
        "rust_command":"bound cargo build --offline --jobs 1 --message-format=json --bin starting443-public-host --manifest-path rust-host/Cargo.toml --target-dir <fresh>; execute the exact reported absolute native binary directly",
        "csharp_build":"dotnet build Release --disable-build-servers -m:1 -p:UseSharedCompilation=false --artifacts-path <model-artifacts> <Host.csproj>",
        "csharp_run":"dotnet <fresh model Host.dll> <frozen controls> <model ordinal> <fresh evidence>",
        "execution_routes":execution_route_binding()?,
        "required_owned_limits":{"max_roots":16,"max_active_frames":32,"max_path_segments":64,"max_metadata_bytes":16777216,"max_walk_contexts":512}}),
    )
}
fn reject_compiler_overrides(selectors: &BTreeMap<&str, Option<String>>) -> TestResult<()> {
    for name in ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        if selectors.get(name).is_some_and(Option::is_some) {
            return Err(format!(
                "{name} must be unset: this campaign binds the default compiler route"
            )
            .into());
        }
    }
    Ok(())
}
