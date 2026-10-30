//! Retained emission only. No compiler, host, engine execution or cleanup here.
//! Native Windows directory handles pin ordinary parents against replacement.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde_json::json;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PROFILES: [(&str, &str); 5] = [
    ("direct", ""),
    ("conditional", "conditional"),
    ("root-scalars", "root-scalars"),
    ("edi", "edi"),
    ("conditional-edi", "conditional/edi"),
];
const HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup><ItemGroup><Compile Include="Program.cs"/><Compile Include="LookupOracle.cs"/><ProjectReference Include="@LIBRARY@"/></ItemGroup></Project>
"#;

fn fail(message: impl Into<String>) -> Box<dyn std::error::Error> {
    io::Error::other(message.into()).into()
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ordinary(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Reparse, offline and recall-backed entries are outside this profile.
        metadata.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) == 0
            && !metadata.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        !metadata.file_type().is_symlink()
    }
}

fn read_open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x02000000 | 0x00200000);
    }
    options.open(path)
}

fn hold_directory(path: &Path) -> Result<File> {
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(fail(
            "retained export requires native Windows locked-directory support",
        ))
    }
    #[cfg(windows)]
    {
        let metadata = fs::symlink_metadata(path)?;
        if !ordinary(&metadata) || !metadata.is_dir() {
            return Err(fail(format!("unsafe parent directory: {}", path.display())));
        }
        let held = read_open(path)?;
        let metadata = held.metadata()?;
        if !ordinary(&metadata) || !metadata.is_dir() {
            return Err(fail("opened parent is not an ordinary directory"));
        }
        Ok(held)
    }
}

fn safe_component(text: &str) -> bool {
    let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
    !text.is_empty()
        && text != "."
        && text != ".."
        && !text.ends_with(['.', ' '])
        && !text
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "CONIN$"
                | "CONOUT$"
                | "COM¹"
                | "COM²"
                | "COM³"
                | "LPT¹"
                | "LPT²"
                | "LPT³"
        )
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn relative(path: &str) -> Result<PathBuf> {
    if path.contains('\\') || path.split('/').any(|part| !safe_component(part)) {
        return Err(fail("ambiguous relative export path"));
    }
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.components().any(|part| {
            !matches!(part, Component::Normal(value) if value.to_str().is_some_and(safe_component))
        })
    {
        return Err(fail("unsafe relative export path"));
    }
    Ok(path.to_path_buf())
}

fn folded(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

fn overlaps(left: &Path, right: &Path) -> bool {
    let (left, right) = (folded(left), folded(right));
    left.starts_with(&right) || right.starts_with(&left)
}

fn resolved(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Ok(path.canonicalize()?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path.parent().ok_or_else(|| fail("path has no parent"))?;
    Ok(resolved(parent)?.join(path.file_name().ok_or_else(|| fail("path has no name"))?))
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(repo).args(args).output()?;
    if !output.status.success() {
        return Err(fail("Git metadata query failed"));
    }
    Ok(String::from_utf8(output.stdout)?)
}

struct Destination {
    root: PathBuf,
    directories: BTreeSet<PathBuf>,
    _held: Vec<File>,
}

impl Destination {
    fn create(requested: &Path, repo: &Path) -> Result<Self> {
        if !requested.is_absolute() {
            return Err(fail("FERRULE_LOOKUP_EXPORT_ROOT must be absolute"));
        }
        let text = requested
            .to_str()
            .ok_or_else(|| fail("export root must be Unicode"))?;
        if text
            .split(['/', '\\'])
            .skip(1)
            .any(|part| !safe_component(part))
        {
            return Err(fail("export root contains an ambiguous component"));
        }
        #[cfg(windows)]
        if !matches!(requested.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
        {
            return Err(fail("only ordinary absolute drive paths are admitted"));
        }
        if requested.components().any(|part| match part {
            Component::Normal(value) => !value.to_str().is_some_and(safe_component),
            Component::ParentDir | Component::CurDir => true,
            _ => false,
        }) {
            return Err(fail("ambiguous export root"));
        }
        match fs::symlink_metadata(requested) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(fail("export root must not exist, including dangling links")),
        }
        let parent = requested
            .parent()
            .ok_or_else(|| fail("export root has no parent"))?;
        let mut held = Vec::new();
        for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
            held.push(hold_directory(ancestor)?);
            match fs::symlink_metadata(ancestor.join(".git")) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(fail("export cannot be inside a repository or worktree")),
            }
        }
        let parent = parent.canonicalize()?;
        #[cfg(windows)]
        if !matches!(parent.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_) | std::path::Prefix::Disk(_)))
        {
            return Err(fail(
                "network or device-backed export parents are not admitted",
            ));
        }
        let root = parent.join(
            requested
                .file_name()
                .ok_or_else(|| fail("export root has no name"))?,
        );
        let mut protected = vec![repo.canonicalize()?];
        for item in git(repo, &["worktree", "list", "--porcelain", "-z"])?.split('\0') {
            if let Some(path) = item.strip_prefix("worktree ") {
                protected.push(Path::new(path).canonicalize()?);
            }
        }
        let common = PathBuf::from(git(repo, &["rev-parse", "--git-common-dir"])?.trim());
        protected.push(resolved(&repo.join(common))?);
        for name in [
            "CARGO_TARGET_DIR",
            "CARGO_BUILD_BUILD_DIR",
            "CARGO_SLOT_ROOT",
        ] {
            let value = std::env::var_os(name);
            if value.is_none() && name != "CARGO_SLOT_ROOT" {
                return Err(fail(
                    "retained export requires the fixed-slot Cargo wrapper environment",
                ));
            }
            if let Some(path) = value {
                if !Path::new(&path).is_absolute() {
                    return Err(fail("Cargo cache paths must be absolute"));
                }
                let path = resolved(&repo.join(path))?;
                if path
                    .file_name()
                    .and_then(|part| part.to_str())
                    .is_some_and(|name| {
                        name.strip_prefix("slot-").is_some_and(|slot| {
                            !slot.is_empty() && slot.bytes().all(|b| b.is_ascii_digit())
                        })
                    })
                {
                    protected.push(
                        path.parent()
                            .ok_or_else(|| fail("slot has no pool"))?
                            .to_path_buf(),
                    );
                }
                protected.push(path);
            }
        }
        if protected.iter().any(|path| overlaps(&root, path)) {
            return Err(fail(
                "export root overlaps source, worktrees, Git state or Cargo caches",
            ));
        }
        // All ancestors already exist and remain held. Never recursively create
        // parents, adopt an existing root, or remove a partially written export.
        fs::create_dir(&root)?;
        held.push(hold_directory(&root)?);
        Ok(Self {
            root,
            directories: BTreeSet::from([PathBuf::new()]),
            _held: held,
        })
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let name = relative(name)?;
        let mut directory = PathBuf::new();
        for part in name
            .parent()
            .ok_or_else(|| fail("file has no parent"))?
            .components()
        {
            directory.push(part);
            if self.directories.insert(directory.clone()) {
                let path = self.root.join(&directory);
                fs::create_dir(&path)?;
                self._held.push(hold_directory(&path)?);
            }
        }
        let path = self.root.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let mut held = read_open(&path)?;
        if !ordinary(&held.metadata()?) || !held.metadata()?.is_file() {
            return Err(fail("written output is not an ordinary file"));
        }
        let mut actual = Vec::new();
        Read::by_ref(&mut held)
            .take(bytes.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        if actual != bytes {
            return Err(fail("written output differs from planned bytes"));
        }
        self._held.push(held);
        Ok(())
    }

    fn verify_membership(&self, expected: &BTreeSet<PathBuf>) -> Result<()> {
        let mut files = BTreeSet::new();
        let mut directories = BTreeSet::from([PathBuf::new()]);
        for directory in &self.directories {
            for entry in fs::read_dir(self.root.join(directory))? {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                if !ordinary(&metadata) {
                    return Err(fail("unsafe entry in retained output"));
                }
                let path = directory.join(entry.file_name());
                if metadata.is_dir() {
                    directories.insert(path);
                } else if metadata.is_file() {
                    files.insert(path);
                } else {
                    return Err(fail("unsupported retained output entry"));
                }
            }
        }
        if files != *expected || directories != self.directories {
            return Err(fail("retained output membership differs"));
        }
        Ok(())
    }
}

fn fixture_names() -> Vec<String> {
    let mut names = Vec::new();
    for (folder, files) in [
        (
            "",
            &[
                "mapping.mfd",
                "primary.schema.json",
                "lookup.schema.json",
                "target.schema.json",
                "cases.json",
                "rust_host.rs",
                "csharp_host.cs",
                "typed_assert.cs",
            ][..],
        ),
        (
            "conditional",
            &["mapping.mfd", "primary.schema.json", "cases.json"][..],
        ),
        (
            "root-scalars",
            &[
                "mapping.mfd",
                "primary.schema.json",
                "lookup.schema.json",
                "target.schema.json",
                "cases.json",
                "rust_host.rs",
                "csharp_host.cs",
                "README.md",
            ][..],
        ),
        (
            "edi",
            &[
                "mapping.mfd",
                "888.Config",
                "Envelope.Config",
                "cases.json",
                "rust_host.rs",
                "csharp_host.cs",
            ][..],
        ),
        (
            "conditional/edi",
            &["mapping.mfd", "888.Config", "Envelope.Config", "cases.json"][..],
        ),
    ] {
        names.extend(files.iter().map(|file| {
            if folder.is_empty() {
                (*file).into()
            } else {
                format!("{folder}/{file}")
            }
        }));
    }
    names
}

fn add(files: &mut BTreeMap<String, Vec<u8>>, name: String, bytes: Vec<u8>) -> Result<()> {
    relative(&name)?;
    if files.keys().any(|other| other.eq_ignore_ascii_case(&name)) {
        return Err(fail("duplicate or case-aliased output"));
    }
    files.insert(name, bytes);
    Ok(())
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ordinary_path(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| fail("generated project path must be Unicode"))?;
    Ok(text.strip_prefix("\\\\?\\").unwrap_or(text).to_string())
}

pub(super) fn export() -> Result<()> {
    let requested = std::env::var_os("FERRULE_LOOKUP_EXPORT_ROOT")
        .ok_or_else(|| fail("FERRULE_LOOKUP_EXPORT_ROOT is required"))?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut destination = Destination::create(Path::new(&requested), &repo)?;
    println!(
        "RETAINED LOOKUP EXPORT ROOT: {}",
        destination.root.display()
    );
    let runtime = repo.join("crates/codegen-runtime").canonicalize()?;
    let mut held_sources = Vec::new();
    let mut fixtures = BTreeMap::new();
    let mut source_rows = Vec::new();
    let mut source_directories = BTreeSet::new();
    for name in fixture_names() {
        let path = super::fixtures().join(&name);
        for parent in path
            .parent()
            .ok_or_else(|| fail("fixture has no parent"))?
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if source_directories.insert(parent.to_path_buf()) {
                held_sources.push(hold_directory(parent)?);
            }
        }
        let mut file = read_open(&path)?;
        let metadata = file.metadata()?;
        if !ordinary(&metadata) || !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(fail("unsupported or oversized source fixture"));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != metadata.len() {
            return Err(fail("fixture size changed"));
        }
        source_rows.push(json!({"path":name,"bytes":bytes.len(),"sha256":hash(&bytes)}));
        fixtures.insert(name, bytes);
        held_sources.push(file);
    }
    let mut files = BTreeMap::new();
    for (name, bytes) in &fixtures {
        add(&mut files, format!("fixtures/{name}"), bytes.clone())?;
    }
    add(
        &mut files,
        "NuGet.Config".into(),
        b"<configuration><packageSources><clear/></packageSources></configuration>\n".to_vec(),
    )?;
    let mut profiles = Vec::new();
    for (name, profile) in PROFILES {
        let prefix = format!("profiles/{name}");
        let (project, rust, csharp) = super::generated_artifacts(profile);
        for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
            for file in artifacts.into_files() {
                add(
                    &mut files,
                    format!("{prefix}/generated/{language}/{}", file.path.as_str()),
                    file.contents,
                )?;
            }
        }
        for (kind, schema) in [("source", &project.source), ("target", &project.target)] {
            add(
                &mut files,
                format!("{prefix}/{kind}-schema.json"),
                serde_json::to_vec(schema)?,
            )?;
        }
        let fixture_prefix = if profile.is_empty() {
            String::new()
        } else {
            format!("{profile}/")
        };
        let host_prefix = if profile.ends_with("edi") {
            "edi/"
        } else if profile == "root-scalars" {
            "root-scalars/"
        } else {
            ""
        };
        add(
            &mut files,
            format!("{prefix}/cases.json"),
            fixtures[&format!("{fixture_prefix}cases.json")].clone(),
        )?;
        add(
            &mut files,
            format!("{prefix}/rust-host/host.rs"),
            fixtures[&format!("{host_prefix}rust_host.rs")].clone(),
        )?;
        add(
            &mut files,
            format!("{prefix}/csharp-host/Program.cs"),
            fixtures[&format!("{host_prefix}csharp_host.cs")].clone(),
        )?;
        add(
            &mut files,
            format!("{prefix}/csharp-host/LookupOracle.cs"),
            fixtures["typed_assert.cs"].clone(),
        )?;
        let library = destination.root.join(&prefix).join("generated/rust");
        let manifest = format!(
            "[package]\nname = \"json-lookup-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[workspace]\n[dependencies]\njson-lookup-mapping = {{ path = {} }}\ncodegen-runtime = {{ path = {} }}\nserde_json = \"1\"\n[[bin]]\nname = \"lookup-host\"\npath = \"host.rs\"\n",
            serde_json::to_string(&ordinary_path(&library)?)?,
            serde_json::to_string(&ordinary_path(&runtime)?)?
        );
        add(
            &mut files,
            format!("{prefix}/rust-host/Cargo.toml"),
            manifest.into_bytes(),
        )?;
        let library = destination
            .root
            .join(&prefix)
            .join("generated/csharp/Ferrule.Generated.csproj");
        add(
            &mut files,
            format!("{prefix}/csharp-host/Host.csproj"),
            HOST_PROJECT
                .replace("@LIBRARY@", &xml(&ordinary_path(&library)?))
                .into_bytes(),
        )?;
        profiles.push(json!({
            "name": name,
            "fixture_profile": profile,
            "directory": prefix,
            "rust_library": "generated/rust/Cargo.toml",
            "rust_package": "json-lookup-mapping",
            "rust_host": "rust-host/Cargo.toml",
            "rust_binary": "lookup-host",
            "csharp_library": "generated/csharp/Ferrule.Generated.csproj",
            "csharp_host": "csharp-host/Host.csproj",
            "csharp_assembly": "Host.dll",
            "target_framework": "net10.0"
        }));
    }
    let mut membership = files.keys().cloned().collect::<Vec<_>>();
    membership.push("export-manifest.json".into());
    membership.sort();
    let outputs = files
        .iter()
        .map(|(name, bytes)| json!({"path":name,"bytes":bytes.len(),"sha256":hash(bytes)}))
        .collect::<Vec<_>>();
    let manifest = json!({
        "format": 1,
        "operation": "emission-only",
        "output_root": destination.root,
        "source_head": git(&repo, &["rev-parse", "HEAD"])?.trim(),
        "source_fixture_root": super::fixtures(),
        "source_fixtures": source_rows,
        "compiled_exporter_sha256": hash(include_bytes!("retained_export.rs")),
        "compiled_test_sha256": hash(include_bytes!("../json_array_lookup.rs")),
        "profiles": profiles,
        "outputs": outputs,
        "output_membership": membership,
        "manifest_hash_self_excluded": true,
        "engine_mapping_executed": false,
        "compilers_executed": false,
        "hosts_executed": false,
        "build_policy": "Copy each host directory to a separate new build root before restore/lockfile generation. Absolute library references keep retained generated source unchanged. Use fixed Cargo slots, admitted Release stages, Cargo JSON artifact identity, and pinned .NET SDK with global-tools PATH opt-out."
    });
    add(
        &mut files,
        "export-manifest.json".into(),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    for (name, bytes) in &files {
        destination.write(name, bytes)?;
    }
    destination.verify_membership(
        &files
            .keys()
            .map(|name| relative(name))
            .collect::<Result<BTreeSet<_>>>()?,
    )?;
    println!(
        "RETAINED LOOKUP EXPORT MANIFEST: {}",
        destination.root.join("export-manifest.json").display()
    );
    Ok(())
}

#[test]
fn retained_export_relative_paths_refuse_aliases_traversal_and_devices() {
    assert!(relative("profiles/direct/rust-host/Cargo.toml").is_ok());
    for path in [
        "",
        "/root",
        "C:/root",
        "../file",
        "a/../file",
        "a/./file",
        "a//file",
        "a\\file",
        "CON.txt",
        "COM¹",
        "LPT².log",
        "CONIN$",
        "file:stream",
        "trailing.",
        "trailing ",
    ] {
        assert!(relative(path).is_err(), "{path}");
    }
}

#[test]
fn retained_export_never_adopts_existing_roots_or_writes_in_its_source_tree() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let ordinary_repo = PathBuf::from(ordinary_path(&repo).unwrap());
    assert!(Destination::create(Path::new("relative-export"), &repo).is_err());
    assert!(Destination::create(&ordinary_repo, &repo).is_err());
    let candidate = ordinary_repo.join("retained-export-must-not-be-created");
    assert!(
        matches!(fs::symlink_metadata(&candidate), Err(error) if error.kind() == io::ErrorKind::NotFound)
    );
    assert!(Destination::create(&candidate, &repo).is_err());
    assert!(
        matches!(fs::symlink_metadata(&candidate), Err(error) if error.kind() == io::ErrorKind::NotFound)
    );
}

#[test]
fn retained_export_overlap_uses_case_insensitive_components_not_text_prefixes() {
    assert!(overlaps(Path::new("Alpha/Child"), Path::new("alpha")));
    assert!(overlaps(Path::new("Alpha"), Path::new("alpha/Child")));
    assert!(!overlaps(Path::new("AlphaSibling"), Path::new("alpha")));
    assert!(!overlaps(Path::new("Alpha/One"), Path::new("alpha/Two")));
}
