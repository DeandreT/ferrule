// The expected input pathset is the ORIGINAL successful preparation, never a
// source copy or a fresh directory listing taken immediately before a build.
// Output allowances are an explicit coordinator plan, not suffix/name filters.
// Serialize native separators BETWEEN components only. On Unix a backslash is
// part of one filename and must never become a portable directory separator.
fn campaign_path_key(path: &Path) -> TestResult<String> {
    let raw = path
        .to_str()
        .ok_or("path identity requires exact UTF-8, not lossy conversion")?;
    let mut parts = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err("relative normal path components required".into());
        };
        let name = name.to_str().ok_or("path component must be exact UTF-8")?;
        if name.contains('\\') || name.contains('/') || name.contains('\0') {
            return Err("noncanonical filesystem component".into());
        }
        parts.push(name);
    }
    // components() elides repeated/trailing separators and inner '.'. Refuse
    // those spellings rather than silently canonicalizing their keys as well.
    let spelled = raw
        .split(|c| c == '/' || (cfg!(windows) && c == '\\'))
        .collect::<Vec<_>>();
    if parts.is_empty() || spelled != parts {
        return Err("noncanonical relative path spelling".into());
    }
    let key = parts.join("/");
    codegen::ArtifactPath::new(key.as_str())?;
    Ok(key)
}
fn campaign_validate_key(key: &str) -> TestResult<()> {
    codegen::ArtifactPath::new(key)?;
    if campaign_path_key(Path::new(key))? != key {
        return Err("noncanonical manifest key".into());
    }
    Ok(())
}
fn campaign_insert_path<T>(map: &mut BTreeMap<String, T>, key: String, value: T) -> TestResult<()> {
    campaign_validate_key(&key)?;
    match map.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(_) => {
            Err("duplicate canonical path key".into())
        }
    }
}
fn campaign_insert_path_set(
    set: &mut std::collections::BTreeSet<String>,
    key: String,
) -> TestResult<()> {
    campaign_validate_key(&key)?;
    if !set.insert(key) {
        return Err("duplicate canonical path key".into());
    }
    Ok(())
}
fn campaign_path_set(
    paths: impl IntoIterator<Item = String>,
) -> TestResult<std::collections::BTreeSet<String>> {
    let mut set = std::collections::BTreeSet::new();
    for path in paths {
        campaign_insert_path_set(&mut set, path)?;
    }
    Ok(set)
}
struct PreparedInputFence {
    originals: BTreeMap<String, (usize, String)>,
    directories: std::collections::BTreeSet<String>,
    output_files: std::collections::BTreeSet<String>,
    output_directories: std::collections::BTreeSet<String>,
    generated_lock: Option<(usize, String)>,
}
impl PreparedInputFence {
    fn new(prepared: &Json, success_bytes: &[u8]) -> TestResult<Self> {
        let rows: Vec<legacy_artifact_policy::ManifestRow> =
            serde_json::from_value(prepared["evidence"].clone())?;
        let mut originals = BTreeMap::new();
        for (path, bytes, sha) in rows {
            if path == SUCCESS {
                return Err("prepared rows cannot replace their success receipt".into());
            }
            campaign_insert_path(&mut originals, path, (bytes, sha))?;
        }
        campaign_insert_path(
            &mut originals,
            SUCCESS.into(),
            (success_bytes.len(), digest(success_bytes)),
        )?;
        let mut directories = std::collections::BTreeSet::new();
        for path in originals.keys() {
            for parent in Path::new(path)
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                let key = campaign_path_key(parent)?;
                // The same logical ancestor is shared by multiple original
                // rows. This is derivation, not a second filesystem entry.
                if !directories.contains(&key) {
                    campaign_insert_path_set(&mut directories, key)?;
                }
            }
        }
        let mut output_files = campaign_path_set([
            "ALL-COMPLETE-NATIVE-COMPARISONS.json".into(),
            "ALL-COMPLETE-LEGACY-NATIVE-COMPARISONS.json".into(),
            "ALL-GENERATED-EXECUTION-STATUSES.txt".into(),
            "RUST-COMPLETE-COMPILER-ARTIFACT-SELECTION.txt".into(),
            "RUST-COMPLETE-EXECUTABLE-BINDING-FAILURE.txt".into(),
            "RUST-COMPLETE-PRE-RUN-EXECUTABLE.json".into(),
            "RUST-COMPLETE-POST-RUN-EXECUTABLE.json".into(),
            "rust-COMPLETE-HOST-EVIDENCE-INDEX.json".into(),
            "rust-COMPLETE-HOST-EVIDENCE-COMPARISON.json".into(),
        ])?;
        // No compiler source tree is an output directory. In particular no
        // recursive bin/obj/target/.cargo exemption is allowed inside a project.
        let mut output_directories = campaign_path_set([
            "native".into(),
            "legacy-control".into(),
            "host-originals".into(),
            "run-contracts".into(),
            "prepared-input-fences".into(),
            "rust-actual".into(),
            "rust-build-target".into(),
            ".runtime-temp".into(),
            ".dotnet-home".into(),
            ".nuget-packages".into(),
        ])?;
        for label in ["starting443-rust-build", "starting443-rust-host"] {
            campaign_insert_path_set(
                &mut output_files,
                format!("{label}-COMPLETE-EXECUTION-ROUTE.json"),
            )?;
        }
        for index in 0..=MODELS {
            let label = if index == MODELS {
                "legacy".into()
            } else {
                format!("model{index}")
            };
            campaign_insert_path_set(&mut output_directories, format!("dotnet-artifacts-{label}"))?;
            campaign_insert_path_set(&mut output_directories, format!("csharp-{label}-actual"))?;
            for suffix in [
                "PRE-RUN-BINARIES.json",
                "POST-RUN-BINARIES.json",
                "COMPLETE-ASSEMBLY-BINDING-FAILURE.txt",
                "COMPLETE-HOST-EVIDENCE-INDEX.json",
                "COMPLETE-HOST-EVIDENCE-COMPARISON.json",
            ] {
                campaign_insert_path_set(&mut output_files, format!("csharp-{label}-{suffix}"))?;
            }
            for phase in ["build", "host"] {
                campaign_insert_path_set(
                    &mut output_files,
                    format!("starting443-csharp-{label}-{phase}-COMPLETE-EXECUTION-ROUTE.json"),
                )?;
            }
        }
        for path in originals.keys().chain(directories.iter()) {
            if output_files.contains(path)
                || output_directories
                    .iter()
                    .any(|output| Path::new(path).starts_with(output))
            {
                return Err("execution output allowance overlaps frozen preparation".into());
            }
        }
        Ok(Self {
            originals,
            directories,
            output_files,
            output_directories,
            generated_lock: None,
        })
    }

    fn inspect(&mut self, root: &Path, may_create_rust_lock: bool) -> TestResult<Json> {
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || root.canonicalize()? != root {
            return Err("prepared input root must remain a resolved directory".into());
        }
        let mut files = BTreeMap::new();
        let mut directories = std::collections::BTreeSet::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut outputs = Vec::new();
        let mut unexpected = Vec::new();
        self.visit(
            root,
            root,
            may_create_rust_lock,
            &mut files,
            &mut directories,
            &mut outputs,
            &mut unexpected,
            &mut seen,
        )?;
        let mut expected = self.originals.clone();
        if let Some(lock) = &self.generated_lock {
            campaign_insert_path(&mut expected, "rust-host/Cargo.lock".into(), lock.clone())?;
        }
        // Cargo alone may produce this one new file, only at the first Rust
        // build's postfence. Its actual bytes become a separately retained output
        // binding; no other input row is refreshed from actual source products.
        let new_lock = if may_create_rust_lock && !expected.contains_key("rust-host/Cargo.lock") {
            files.get("rust-host/Cargo.lock").cloned()
        } else {
            None
        };
        if let Some(lock) = &new_lock {
            campaign_insert_path(&mut expected, "rust-host/Cargo.lock".into(), lock.clone())?;
        }
        let matched = unexpected.is_empty() && files == expected && directories == self.directories;
        let record = json!({"matched":matched,"expected_files":expected,"actual_files":files,
            "expected_directories":self.directories,"actual_directories":directories,"unexpected_entries":unexpected,
            "permitted_output_files":self.output_files,"permitted_output_directories":self.output_directories,
            "observed_output_entries":outputs,"new_generated_cargo_lock":new_lock});
        if matched && new_lock.is_some() {
            self.generated_lock = new_lock;
        }
        Ok(record)
    }

    fn visit(
        &self,
        root: &Path,
        directory: &Path,
        may_create_rust_lock: bool,
        files: &mut BTreeMap<String, (usize, String)>,
        directories: &mut std::collections::BTreeSet<String>,
        outputs: &mut Vec<String>,
        unexpected: &mut Vec<String>,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> TestResult<()> {
        let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = campaign_path_key(path.strip_prefix(root)?)?;
            // Cover files, directories AND output allowance roots before any
            // skip/insert: even an identical duplicate is a pathset failure.
            campaign_insert_path_set(seen, relative.clone())?;
            let kind = entry.file_type()?;
            if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
                unexpected.push(relative);
                continue;
            }
            if kind.is_dir() && self.output_directories.contains(&relative) {
                outputs.push(relative);
                continue;
            }
            if kind.is_file() && self.output_files.contains(&relative) {
                outputs.push(relative);
                continue;
            }
            if kind.is_dir() {
                campaign_insert_path_set(directories, relative.clone())?;
                // Recurse even into unreviewed directories: their presence and
                // complete source/config pathset are reported, never skipped.
                self.visit(
                    root,
                    &path,
                    may_create_rust_lock,
                    files,
                    directories,
                    outputs,
                    unexpected,
                    seen,
                )?;
            } else {
                let bytes = fs::read(path)?;
                let is_lock = relative == "rust-host/Cargo.lock"
                    && (may_create_rust_lock || self.generated_lock.is_some());
                if !self.originals.contains_key(&relative) && !is_lock {
                    unexpected.push(relative.clone());
                }
                campaign_insert_path(files, relative, (bytes.len(), digest(&bytes)))?;
            }
        }
        Ok(())
    }

    fn check(
        &mut self,
        evidence: &Evidence,
        label: &str,
        may_create_rust_lock: bool,
    ) -> TestResult<()> {
        let record = self.inspect(&evidence.0, may_create_rust_lock)?;
        evidence.json(&format!("prepared-input-fences/{label}.json"), &record)?;
        if record["matched"] != true {
            return Err("complete prepared input pathset/body/topology differs; source/config addition is not an output".into());
        }
        Ok(())
    }
}
