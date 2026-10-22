//! An explicit offline project bundle; acquisition never discovers local files.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use codegen::{ArtifactPath, ArtifactSet, GeneratedFile};
use serde::Serialize;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

const MAX_FILES: usize = 4096;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENTS: usize = 64;
const MAX_PATH_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_CATALOGS: usize = 64;
const PACKAGE: &str = "ferrule-package.json";
const INVENTORY: &str = "ferrule-bundle.json";
const PROJECT: &str = "project.json";

/// The existing package importer's catalog kinds, in declaration order.
#[derive(Clone, Copy, Debug, Serialize)]
pub enum BundleCatalogKind {
    #[serde(rename = "edi-config")]
    EdiConfig,
    #[serde(rename = "json-schema")]
    JsonSchema,
}

#[derive(Clone, Debug, Serialize)]
pub struct BundleCatalog {
    pub kind: BundleCatalogKind,
    pub root: String,
}

/// All selected file names are portable paths relative to the explicit root.
/// A catalog declaration selects no files implicitly. Instance data is omitted
/// unless include_inputs is true; configured static inputs are then included.
#[derive(Clone, Debug)]
pub struct BundleOptions {
    pub project: String,
    pub resources: Vec<String>,
    pub catalogs: Vec<BundleCatalog>,
    pub include_inputs: bool,
}

#[derive(Clone, Debug)]
pub struct BundleOutcome {
    pub output_directory: PathBuf,
    pub project_path: PathBuf,
    pub files_written: usize,
    pub bytes_written: u64,
    pub input_files: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    path: String,
    source: Option<String>,
    roles: BTreeSet<String>,
    byte_len: usize,
    sha256: String,
    #[serde(skip)]
    bytes: Vec<u8>,
}

struct Plan {
    entries: BTreeMap<String, Entry>,
    bytes: u64,
}
impl Plan {
    fn add(
        &mut self,
        path: String,
        source: Option<String>,
        role: &str,
        bytes: Vec<u8>,
    ) -> anyhow::Result<()> {
        portable(&path)?;
        if self.entries.contains_key(&path) {
            bail!("duplicate bundle destination `{path}`");
        }
        if self.entries.len() >= MAX_FILES
            || bytes.len() as u64 > MAX_FILE_BYTES
            || bytes.len() as u64 > MAX_BUNDLE_BYTES.saturating_sub(self.bytes)
        {
            bail!("bundle file count or byte budget exceeded");
        }
        self.bytes += bytes.len() as u64;
        self.entries.insert(
            path.clone(),
            Entry {
                path,
                source,
                roles: BTreeSet::from([role.into()]),
                byte_len: bytes.len(),
                sha256: digest(&bytes),
                bytes,
            },
        );
        Ok(())
    }
    fn copy(&mut self, root: &Root, path: &str, role: &str) -> anyhow::Result<()> {
        if let Some(entry) = self.entries.get_mut(path) {
            if entry.source.as_deref() != Some(path) {
                bail!("duplicate bundle destination `{path}`");
            }
            entry.roles.insert(role.into());
            return Ok(());
        }
        let bytes = root.read(path, MAX_BUNDLE_BYTES.saturating_sub(self.bytes))?;
        self.add(path.into(), Some(path.into()), role, bytes)
    }
}

/// Exports one validated Project and explicitly declared resources. Source
/// opens are confined below root without following descendant symlinks. The
/// completed tree is validated by the existing package-manifest loader and
/// published by the same atomic, no-replacement writer as generated projects.
pub fn bundle_project(
    root: &Path,
    destination: &Path,
    options: &BundleOptions,
) -> anyhow::Result<BundleOutcome> {
    Root::supported()?;
    portable(&options.project)?;
    if options.resources.len() > MAX_FILES - 3 || options.catalogs.len() > MAX_CATALOGS {
        bail!("bundle resource or catalog count limit exceeded");
    }
    // Also catch dangling destination symlinks before reading any selected file.
    match std::fs::symlink_metadata(destination) {
        Ok(_) => bail!(
            "bundle destination {} already exists",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspecting bundle destination"),
    }
    let root = Root::open(root)?;
    let project_bytes = root.read(&options.project, MAX_FILE_BYTES)?;
    let original_project = serde_json::json!({"path":options.project,"byteLen":project_bytes.len(),"sha256":digest(&project_bytes)});
    let text = std::str::from_utf8(&project_bytes).context("bundle project must be UTF-8")?;
    let mut project = mapping::project_file::decode_str(text).context("decoding bundle project")?;
    super::require_valid(&project)?;
    if !project.sqlite_read_inputs().is_empty() {
        bail!("offline bundles do not support declared SQLite query inputs");
    }
    if project.source_options.http_get.is_some()
        || project
            .extra_sources
            .iter()
            .any(|s| s.options.http_get.is_some() || s.dynamic_path.is_some())
    {
        bail!(
            "offline bundles require static local sources; HTTP and dynamic sources are unsupported"
        );
    }
    let project_parent = options
        .project
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let mut rebases = Vec::new();
    let mut inputs = BTreeSet::new();
    let mut database_inputs = BTreeSet::new();
    let mut outputs = Vec::new();
    if let Some(path) = &mut project.source_path {
        rebase(path, project_parent, "source_path", &mut rebases)?;
        inputs.insert(path.clone());
        if is_database(path, &project.source_options) {
            database_inputs.insert(path.clone());
        }
    }
    if let Some(path) = &mut project.target_path {
        rebase(path, project_parent, "target_path", &mut rebases)?;
        outputs.push(path.clone());
    }
    for (i, source) in project.extra_sources.iter_mut().enumerate() {
        rebase(
            &mut source.path,
            project_parent,
            &format!("extra_sources[{i}].path"),
            &mut rebases,
        )?;
        inputs.insert(source.path.clone());
        if is_database(&source.path, &source.options) {
            database_inputs.insert(source.path.clone());
        }
    }
    for (i, target) in project.extra_targets.iter_mut().enumerate() {
        if let Some(path) = &mut target.path {
            rebase(
                path,
                project_parent,
                &format!("extra_targets[{i}].path"),
                &mut rebases,
            )?;
            outputs.push(path.clone());
        }
    }
    for input in &inputs {
        if [PROJECT, PACKAGE, INVENTORY].contains(&input.as_str()) {
            bail!("configured input `{input}` collides with bundle metadata");
        }
    }
    let mut selected = BTreeSet::new();
    for resource in &options.resources {
        portable(resource)?;
        if !selected.insert(resource) {
            bail!("duplicate declared resource `{resource}`");
        }
        if resource == &options.project {
            bail!("the project cannot also be declared as a resource");
        }
        if inputs.contains(resource) && !options.include_inputs {
            bail!(
                "resource `{resource}` is configured instance data; use --include-inputs explicitly"
            );
        }
    }
    for catalog in &options.catalogs {
        portable(&catalog.root)?;
        root.directory(&catalog.root)?;
        if !options
            .resources
            .iter()
            .any(|p| p.starts_with(&format!("{}/", catalog.root)))
        {
            bail!(
                "catalog `{}` needs at least one explicitly declared resource file",
                catalog.root
            );
        }
    }
    let mut plan = Plan {
        entries: BTreeMap::new(),
        bytes: 0,
    };
    plan.add(
        PROJECT.into(),
        None,
        "project",
        mapping::project_file::encode_pretty(&project)?.into_bytes(),
    )?;
    for resource in &options.resources {
        plan.copy(&root, resource, "resource")?;
    }
    if options.include_inputs {
        for input in &inputs {
            // A live SQLite database cannot be copied correctly as one main
            // file without a snapshot/backup contract. Never guess at sidecars.
            if database_inputs.contains(input) {
                bail!(
                    "SQLite instance copying needs a database snapshot; bundle resources and supply the database at runtime"
                );
            }
            plan.copy(&root, input, "input")?;
        }
    }
    let package = serde_json::json!({"schemaVersion":1,"kind":"ferrule.mapping-package","catalogs":options.catalogs});
    let package_bytes = json_bytes(&package)?;
    if package_bytes.len() > 64 * 1024 {
        bail!("package manifest exceeds the existing 64 KiB import limit");
    }
    plan.add(PACKAGE.into(), None, "package-manifest", package_bytes)?;
    let mut paths = Paths::default();
    for path in plan
        .entries
        .keys()
        .map(String::as_str)
        .chain([INVENTORY])
        .chain(inputs.iter().map(String::as_str))
    {
        paths.file(path, true)?;
    }
    for output in &outputs {
        if [PROJECT, PACKAGE, INVENTORY].contains(&output.as_str()) {
            bail!("configured output `{output}` collides with bundle metadata");
        }
        // Selected targets can intentionally share paths; an existing workbook
        // can also be an explicitly declared update-existing template.
        paths.file(output, true)?;
    }
    for catalog in &options.catalogs {
        paths.directory(&catalog.root)?;
    }
    let input_files = plan
        .entries
        .values()
        .filter(|e| e.roles.contains("input"))
        .count();
    let inventory = serde_json::json!({"schemaVersion":1,"kind":"ferrule.project-bundle",
        "project":PROJECT,"originalProject":original_project,"includeInputs":options.include_inputs,
        "files":plan.entries.values().collect::<Vec<_>>(),"pathRebases":rebases,
        "limits":{"files":MAX_FILES,"fileBytes":MAX_FILE_BYTES,"bundleBytes":MAX_BUNDLE_BYTES,
        "pathBytes":MAX_PATH_BYTES,"pathComponents":MAX_PATH_COMPONENTS,"pathMetadataBytes":MAX_PATH_METADATA_BYTES}});
    plan.add(
        INVENTORY.into(),
        None,
        "bundle-inventory",
        json_bytes(&inventory)?,
    )?;
    let artifacts = ArtifactSet::new(
        plan.entries
            .into_values()
            .map(|e| Ok(GeneratedFile::new(ArtifactPath::new(e.path)?, e.bytes)))
            .collect::<anyhow::Result<Vec<_>>>()?,
    )?;
    let files_written = artifacts.len();
    super::code_generation::write_artifacts_validated(destination, &artifacts, |staging| {
        mfd::PackageManifest::load(&staging.join(PACKAGE))
            .context("validating staged bundle package manifest")?;
        Ok(())
    })
    .context("publishing offline project bundle")?;
    Ok(BundleOutcome {
        output_directory: destination.into(),
        project_path: destination.join(PROJECT),
        files_written,
        bytes_written: plan.bytes,
        input_files,
    })
}

fn is_database(path: &str, options: &mapping::FormatOptions) -> bool {
    // Match read_instance's explicit-adapter precedence before its suffix
    // fallback. A JSON document named input.sqlite is not a SQLite instance.
    if options.xbrl.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.external_source.is_some()
        || options.pdf.is_some()
        || options.flextext.is_some()
        || options.protobuf.is_some()
        || options.edi_kind.is_some()
        || options.xml_document
        || options.json_document
        || options.json5
        || options.json_lines
        || options.fixed_width.is_some()
    {
        return false;
    }
    Path::new(path)
        .extension()
        .and_then(|p| p.to_str())
        .is_some_and(|s| {
            ["db", "sqlite", "sqlite3"]
                .iter()
                .any(|ext| s.eq_ignore_ascii_case(ext))
        })
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn json_bytes(value: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn rebase(
    path: &mut String,
    parent: &str,
    owner: &str,
    records: &mut Vec<serde_json::Value>,
) -> anyhow::Result<()> {
    let before = path.clone();
    let forward = path.replace('\\', "/");
    if forward.starts_with('/') || forward.is_empty() {
        bail!("configured bundle path must be relative and nonempty");
    }
    let mut components = if parent.is_empty() {
        Vec::new()
    } else {
        parent.split('/').collect::<Vec<_>>()
    };
    let mut normal_seen = false;
    for component in forward.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                // Only ancestors of the already no-follow-opened project can
                // be cancelled safely. A configured normal component may be a
                // symlink, so normal/.. must not be normalized away unseen.
                if normal_seen {
                    bail!("configured path `{before}` cancels a normal component before `..`");
                }
                if components.pop().is_none() {
                    bail!("configured path `{before}` traverses outside the bundle source root");
                }
            }
            value => {
                portable(value)?;
                components.push(value);
                normal_seen = true;
            }
        }
    }
    let expected = components.join("/");
    let from = Path::new("/").join(parent);
    *path = super::project_paths::rebase_path(&before, &from, Path::new("/"))?
        .context("configured bundle path is not a portable relative file")?;
    if path != &expected {
        bail!("configured bundle path has ambiguous normalized identity");
    }
    portable(path)?;
    records.push(serde_json::json!({"owner":owner,"from":before,"to":path}));
    Ok(())
}
fn portable(path: &str) -> anyhow::Result<()> {
    ArtifactPath::new(path.to_owned()).context("invalid portable bundle path")?;
    if path.len() > MAX_PATH_BYTES {
        bail!("bundle path exceeds {MAX_PATH_BYTES} bytes");
    }
    if path.split('/').count() > MAX_PATH_COMPONENTS {
        bail!("bundle path exceeds {MAX_PATH_COMPONENTS} components");
    }
    for component in path.split('/') {
        if component.ends_with(['.', ' '])
            || component
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || super::project_paths::dos_device_name(std::ffi::OsStr::new(component))
        {
            bail!("bundle path `{path}` is not a portable file name");
        }
    }
    Ok(())
}
fn case_key(path: &str) -> String {
    path.nfc()
        .flat_map(char::to_uppercase)
        .collect::<String>()
        .nfc()
        .collect()
}

#[derive(Default)]
struct Paths {
    prefixes: BTreeMap<String, String>,
    files: BTreeSet<String>,
    retained_bytes: usize,
}
impl Paths {
    fn charge(&mut self, bytes: usize) -> anyhow::Result<()> {
        if bytes > MAX_PATH_METADATA_BYTES.saturating_sub(self.retained_bytes) {
            bail!("bundle path metadata budget exceeded");
        }
        self.retained_bytes += bytes;
        Ok(())
    }
    fn prefixes(&mut self, path: &str) -> anyhow::Result<String> {
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let key = case_key(&prefix);
            if let Some(prior) = self.prefixes.get(&key) {
                if prior != &prefix {
                    bail!("case/normalization-ambiguous bundle paths `{prior}` and `{prefix}`");
                }
            } else {
                self.charge(key.len() + prefix.len())?;
                self.prefixes.insert(key, prefix.clone());
            }
        }
        Ok(case_key(path))
    }
    fn file(&mut self, path: &str, allow_same: bool) -> anyhow::Result<()> {
        let key = self.prefixes(path)?;
        if self.files.contains(&key) {
            if allow_same {
                return Ok(());
            }
            bail!("duplicate bundle destination `{path}`");
        }
        for (index, _) in key.match_indices('/') {
            if self.files.contains(&key[..index]) {
                bail!("bundle file/directory collision at `{path}`");
            }
        }
        if self.files.iter().any(|p| p.starts_with(&format!("{key}/"))) {
            bail!("bundle file/directory collision at `{path}`");
        }
        self.charge(key.len())?;
        self.files.insert(key);
        Ok(())
    }
    fn directory(&mut self, path: &str) -> anyhow::Result<()> {
        let key = self.prefixes(path)?;
        if self.files.contains(&key) {
            bail!("catalog directory collides with bundle file `{path}`");
        }
        for (index, _) in key.match_indices('/') {
            if self.files.contains(&key[..index]) {
                bail!("catalog directory crosses a bundle file `{path}`");
            }
        }
        Ok(())
    }
}

struct Root {
    directory: File,
}
impl Root {
    fn supported() -> anyhow::Result<()> {
        #[cfg(not(any(
            target_os = "android",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos",
            target_os = "watchos",
            target_os = "redox"
        )))]
        bail!(
            "offline bundle creation currently requires Unix descriptor-relative no-follow file opens"
        );
        #[cfg(any(
            target_os = "android",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos",
            target_os = "watchos",
            target_os = "redox"
        ))]
        Ok(())
    }
    fn open(path: &Path) -> anyhow::Result<Self> {
        #[cfg(any(
            target_os = "android",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos",
            target_os = "watchos",
            target_os = "redox"
        ))]
        let directory = File::from(
            rustix::fs::open(
                path,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NONBLOCK,
                rustix::fs::Mode::empty(),
            )
            .context("opening explicit bundle source root")?,
        );
        #[cfg(not(any(
            target_os = "android",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos",
            target_os = "watchos",
            target_os = "redox"
        )))]
        let directory = File::open(path).context("opening explicit bundle source root")?;
        if !directory.metadata()?.is_dir() {
            bail!("bundle source root is not a directory");
        }
        Ok(Self { directory })
    }
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
        target_os = "redox"
    ))]
    fn open_relative(&self, path: &str, directory: bool) -> anyhow::Result<File> {
        use rustix::fs::{Mode, OFlags, openat};
        portable(path)?;
        let mut held = self.directory.try_clone()?;
        let components = path.split('/').collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            let is_directory = directory || index + 1 != components.len();
            let flags = OFlags::RDONLY
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | if is_directory {
                    OFlags::DIRECTORY
                } else {
                    OFlags::empty()
                };
            held = File::from(
                openat(&held, *component, flags, Mode::empty()).with_context(|| {
                    format!("opening declared bundle path `{path}` without symlinks")
                })?,
            );
        }
        Ok(held)
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
        target_os = "redox"
    )))]
    fn open_relative(&self, _: &str, _: bool) -> anyhow::Result<File> {
        let _ = &self.directory;
        bail!("unsupported bundle producer host")
    }
    fn directory(&self, path: &str) -> anyhow::Result<()> {
        self.open_relative(path, true)?;
        Ok(())
    }
    fn read(&self, path: &str, remaining: u64) -> anyhow::Result<Vec<u8>> {
        let mut file = self.open_relative(path, false)?;
        let before = file.metadata()?;
        if !before.is_file() {
            bail!("declared bundle resource `{path}` is not a regular file");
        }
        if before.len() > MAX_FILE_BYTES || before.len() > remaining {
            bail!("bundle byte budget exceeded by `{path}`");
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        file.by_ref()
            .take(before.len() + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("reading declared bundle file `{path}`"))?;
        let after = file.metadata()?;
        let named = self.open_relative(path, false)?.metadata()?;
        if bytes.len() as u64 != before.len()
            || !same_metadata(&before, &after)?
            || !same_metadata(&before, &named)?
        {
            bail!("declared bundle file `{path}` changed during acquisition");
        }
        Ok(bytes)
    }
}
fn same_metadata(a: &Metadata, b: &Metadata) -> anyhow::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.gid() == b.gid()
            && a.nlink() == b.nlink()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec())
    }
    #[cfg(not(unix))]
    Ok(a.len() == b.len() && a.modified()? == b.modified()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn configured_paths_refuse_cancelled_normal_components() {
        for stored in [
            "link/../input.json",
            "../link/../data/input.json",
            r"..\link\..\data\input.json",
            "../link/./../data/input.json",
        ] {
            let mut path = stored.to_owned();
            let error = rebase(&mut path, "maps", "source_path", &mut Vec::new())
                .expect_err("normal components must not disappear before no-follow validation");
            assert!(error.to_string().contains("cancels a normal component"));
            assert_eq!(path, stored);
        }
    }

    #[test]
    fn portable_names_refuse_all_win32_device_aliases() {
        for name in [
            "CONIN$",
            "conout$.txt",
            "NUL .txt",
            "con  .json",
            "COM1 .dat",
            "lpt9 .json",
            "COM¹ .txt",
            "LPT³.log",
        ] {
            assert!(portable(&format!("resources/{name}")).is_err(), "{name}");
        }
        for name in ["CONIN$-safe.txt", "NUL-file.txt", "COM0.txt", "LPT10.json"] {
            assert!(portable(&format!("resources/{name}")).is_ok(), "{name}");
        }
    }

    #[test]
    fn database_snapshot_policy_respects_explicit_document_adapters() {
        let default = mapping::FormatOptions::default();
        for path in ["input.db", "input.sqlite", "input.SQLITE3"] {
            assert!(is_database(path, &default));
            // Tabular identity is only a fallback for unrecognized suffixes;
            // it does not override a recognized SQLite filename.
            assert!(is_database(
                path,
                &mapping::FormatOptions {
                    tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
                    ..Default::default()
                }
            ));
            for options in [
                mapping::FormatOptions {
                    json_document: true,
                    ..Default::default()
                },
                mapping::FormatOptions {
                    json5: true,
                    ..Default::default()
                },
                mapping::FormatOptions {
                    json_lines: true,
                    ..Default::default()
                },
                mapping::FormatOptions {
                    xml_document: true,
                    ..Default::default()
                },
            ] {
                assert!(!is_database(path, &options));
            }
        }
        assert!(!is_database("input.json", &default));
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> anyhow::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "ferrule-bundle-publish-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root)?;
            Ok(Self(root))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn artifacts() -> anyhow::Result<ArtifactSet> {
        Ok(ArtifactSet::new([GeneratedFile::new(
            ArtifactPath::new("nested/resource.txt")?,
            b"COMPLETE RESOURCE".to_vec(),
        )])?)
    }

    #[test]
    fn failed_staged_validation_removes_stage_and_never_publishes() -> anyhow::Result<()> {
        let root = Temp::new()?;
        let out = root.0.join("bundle");
        let result = super::super::code_generation::write_artifacts_validated(
            &out,
            &artifacts()?,
            |stage| {
                let bytes = std::fs::read(stage.join("nested/resource.txt"))?;
                eprintln!("complete staged original: {bytes:?}");
                assert_eq!(bytes, b"COMPLETE RESOURCE");
                bail!("controlled package validation refusal")
            },
        );
        eprintln!("complete validation refusal: {result:#?}");
        assert!(result.is_err());
        assert!(!out.exists());
        assert_eq!(std::fs::read_dir(&root.0)?.count(), 0);
        Ok(())
    }

    #[test]
    fn late_destination_is_not_replaced_and_staging_is_removed() -> anyhow::Result<()> {
        let root = Temp::new()?;
        let out = root.0.join("bundle");
        let result = super::super::code_generation::write_artifacts_validated(
            &out,
            &artifacts()?,
            |stage| {
                assert_eq!(
                    std::fs::read(stage.join("nested/resource.txt"))?,
                    b"COMPLETE RESOURCE"
                );
                std::fs::create_dir(&out)?;
                std::fs::write(out.join("sentinel"), b"LATE DESTINATION\0\xff")?;
                Ok(())
            },
        );
        eprintln!("complete late destination refusal: {result:#?}");
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(out.join("sentinel"))?,
            b"LATE DESTINATION\0\xff"
        );
        assert_eq!(
            std::fs::read_dir(&root.0)?
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>(),
            vec!["bundle"]
        );
        Ok(())
    }
}
