use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use codegen::ArtifactSet;

use super::{extension_for_dispatch, load_project, validate_tabular_fallback};

mod x12;

/// Source language and runtime linkage for one generated mapping project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateTarget {
    Rust { runtime_path: PathBuf },
    CSharp,
}

/// Files written by a successful atomic generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOutcome {
    pub output_directory: PathBuf,
    pub files_written: usize,
}

/// Lowers a project and atomically writes a complete generated source tree.
///
/// The destination must not already exist. This keeps an unsupported mapping
/// or interrupted write from publishing a partial project.
pub fn generate_project(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
) -> anyhow::Result<GenerateOutcome> {
    generate_project_impl(project_path, output_directory, target, Adapter::Ordinary)
}

/// Generate the ordinary mapping APIs plus an explicitly selected bounded flat
/// CSV output adapter. Stored target format identity and policy remain exact.
pub fn generate_project_with_csv_output(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
) -> anyhow::Result<GenerateOutcome> {
    generate_project_impl(project_path, output_directory, target, Adapter::Csv)
}

/// Generate the ordinary mapping APIs plus explicitly selected singular JSON5
/// text and byte companions. File suffixes and stored identities do not select it.
pub fn generate_project_with_json5_adapters(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
) -> anyhow::Result<GenerateOutcome> {
    generate_project_impl(project_path, output_directory, target, Adapter::Json5)
}

/// Generate explicit raw X12 004010 companions for a singular C# mapping.
/// The stored primary format identities and all retained options must agree
/// with the selected bounded boundary before any source tree is published.
pub fn generate_project_with_x12_adapters(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
) -> anyhow::Result<GenerateOutcome> {
    if target != GenerateTarget::CSharp {
        bail!("generated raw X12 adapters currently require the C# backend");
    }
    generate_project_impl(project_path, output_directory, target, Adapter::X12)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Adapter {
    Ordinary,
    Csv,
    Json5,
    X12,
}

fn generate_project_impl(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
    adapter: Adapter,
) -> anyhow::Result<GenerateOutcome> {
    let project = load_project(project_path)?;
    let x12_policy = if adapter == Adapter::X12 {
        Some(x12::policy(&project)?)
    } else {
        None
    };
    if adapter == Adapter::Json5 {
        // Admission borrows the loaded schemas before ordinary lowering clones them.
        // The shared policy owns every option/schema decision and original cause.
        for (options, side) in [
            (&project.source_options, codegen::Json5BoundarySide::Source),
            (&project.target_options, codegen::Json5BoundarySide::Target),
        ] {
            codegen::validate_json5_format_options(options, side)?;
        }
        for (schema, side) in [
            (&project.source, codegen::Json5BoundarySide::Source),
            (&project.target, codegen::Json5BoundarySide::Target),
        ] {
            codegen_schema::json5_profile::validate_schema(schema)
                .map_err(|error| codegen::Json5BoundaryPolicyError::Schema { side, error })?;
        }
    }
    let csv_policy = if adapter == Adapter::Csv {
        let policy = codegen::CsvOutputPolicy::from_format_options(&project.target_options)?;
        if let Some(path) = &project.target_path {
            let path = Path::new(path);
            validate_tabular_fallback(path, &project.target_options, "target")?;
            let extension = extension_for_dispatch(path, &project.target_options)?;
            if !matches!(extension.as_str(), "csv" | "txt") {
                bail!(
                    "generated CSV output requires a CSV target; stored target selects {extension:?}"
                );
            }
        }
        Some(policy)
    } else {
        None
    };
    let program = codegen::lower(&project).map_err(|error| {
        let details = error
            .diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        anyhow::anyhow!("{error}:\n  - {details}")
    })?;
    let artifacts = match target {
        GenerateTarget::Rust { runtime_path } => {
            let runtime_path = fs::canonicalize(&runtime_path).with_context(|| {
                format!(
                    "resolving Rust codegen runtime path {}",
                    runtime_path.display()
                )
            })?;
            let runtime_path = runtime_path
                .to_str()
                .context("Rust codegen runtime path must be valid UTF-8")?;
            let options = codegen_rust::Options {
                package_name: "ferrule-generated-mapping".to_string(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime_path.to_owned()),
            };
            if adapter == Adapter::Json5 {
                codegen_rust::emit_with_json5(&program, &options)?
            } else {
                match &csv_policy {
                    Some(policy) => codegen_rust::emit_with_csv_output(&program, &options, policy)?,
                    None => codegen_rust::emit(&program, &options)?,
                }
            }
        }
        GenerateTarget::CSharp => {
            if let Some(policy) = &x12_policy {
                codegen_csharp::emit_with_x12(&program, policy)?
            } else if adapter == Adapter::Json5 {
                codegen_csharp::emit_with_json5(&program)?
            } else {
                match &csv_policy {
                    Some(policy) => codegen_csharp::emit_with_csv_output(&program, policy)?,
                    None => codegen_csharp::emit(&program)?,
                }
            }
        }
    };
    write_artifacts(output_directory, &artifacts)?;
    Ok(GenerateOutcome {
        output_directory: output_directory.to_path_buf(),
        files_written: artifacts.len(),
    })
}

fn write_artifacts(output_directory: &Path, artifacts: &ArtifactSet) -> anyhow::Result<()> {
    if output_directory.exists() {
        bail!(
            "generated output directory {} already exists",
            output_directory.display()
        );
    }
    let parent = output_directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating generated output parent {}", parent.display()))?;
    let name = output_directory
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .context("generated output directory must have a UTF-8 file name")?;
    let mut staging = None;
    for attempt in 0..100_u32 {
        let candidate = parent.join(format!(
            ".{name}.ferrule-stage-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => {
                staging = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "creating generated staging directory {}",
                        candidate.display()
                    )
                });
            }
        }
    }
    let staging = staging.context("could not allocate a generated staging directory")?;
    let mut pending = PendingDirectory(Some(staging));
    let staging = pending.path();
    for file in artifacts.files() {
        let path = staging.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("creating generated artifact directory {}", parent.display())
            })?;
        }
        fs::write(&path, &file.contents)
            .with_context(|| format!("writing generated artifact {}", path.display()))?;
    }
    publish_directory(staging, output_directory)
        .map_err(|error| publication_error(staging, output_directory, error))?;
    pending.commit();
    Ok(())
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
))]
fn publish_directory(staging: &Path, output_directory: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging,
        rustix::fs::CWD,
        output_directory,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
)))]
fn publish_directory(staging: &Path, output_directory: &Path) -> std::io::Result<()> {
    if output_directory.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "generated output destination already exists",
        ));
    }
    fs::rename(staging, output_directory)
}

fn publication_error(
    staging: &Path,
    output_directory: &Path,
    error: std::io::Error,
) -> anyhow::Error {
    let mut context = format!(
        "publishing generated output directory {}",
        output_directory.display()
    );
    if let Some(hint) = atomic_publication_hint(staging, output_directory, &error) {
        context.push_str(": ");
        context.push_str(hint);
        context.push_str(
            "; choose an output filesystem supporting atomic no-replace directory rename, or use a CLI built for the destination's native operating system",
        );
    }
    anyhow::Error::new(error).context(context)
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
))]
fn atomic_publication_hint(
    staging: &Path,
    output_directory: &Path,
    error: &std::io::Error,
) -> Option<&'static str> {
    let errno = rustix::io::Errno::from_io_error(error);
    if error.kind() == std::io::ErrorKind::Unsupported
        || errno == Some(rustix::io::Errno::NOSYS)
        || errno == Some(rustix::io::Errno::OPNOTSUPP)
    {
        return Some(
            "atomic no-replace directory publication is unavailable in this runtime or filesystem",
        );
    }
    // EINVAL also covers invalid paths and self-descendant renames. Narrow
    // the hint to a real directory and an absent sibling destination. Keep
    // it conditional: filesystem name rules can also reject a valid-looking
    // destination. Failed metadata reads preserve the ordinary OS diagnostic.
    if errno == Some(rustix::io::Errno::INVAL)
        && valid_publication_siblings(staging, output_directory)
    {
        return Some(
            "atomic no-replace directory publication was refused; this runtime or filesystem may not support the required operation",
        );
    }
    None
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
)))]
fn atomic_publication_hint(
    _staging: &Path,
    _output_directory: &Path,
    error: &std::io::Error,
) -> Option<&'static str> {
    (error.kind() == std::io::ErrorKind::Unsupported).then_some(
        "atomic no-replace directory publication is unavailable in this runtime or filesystem",
    )
}

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "redox",
))]
fn valid_publication_siblings(staging: &Path, output_directory: &Path) -> bool {
    let (Some(staging_name), Some(output_name)) =
        (staging.file_name(), output_directory.file_name())
    else {
        return false;
    };
    if staging_name == output_name {
        return false;
    }
    let parent = |path: &Path| {
        fs::canonicalize(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        )
    };
    let (Ok(staging_parent), Ok(output_parent)) = (parent(staging), parent(output_directory))
    else {
        return false;
    };
    staging_parent == output_parent
        && fs::symlink_metadata(staging).is_ok_and(|metadata| metadata.is_dir())
        && matches!(fs::symlink_metadata(output_directory), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

struct PendingDirectory(Option<PathBuf>);

impl PendingDirectory {
    fn path(&self) -> &Path {
        self.0.as_deref().unwrap_or_else(|| Path::new("."))
    }

    fn commit(&mut self) {
        self.0 = None;
    }
}

impl Drop for PendingDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn publication_fixture() -> std::io::Result<PathBuf> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_codegen_diagnostic_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        fs::create_dir(root.join("staging"))?;
        fs::write(root.join("staging/generated.txt"), "complete source")?;
        Ok(root)
    }

    #[test]
    fn unavailable_publication_retains_original_io_cause_and_staging_cleanup() -> anyhow::Result<()>
    {
        let root = publication_fixture()?;
        let staging = root.join("staging");
        let destination = root.join("destination");
        let pending = PendingDirectory(Some(staging.clone()));
        let error = publication_error(
            &staging,
            &destination,
            std::io::Error::new(std::io::ErrorKind::Unsupported, "original OS cause"),
        );
        assert!(error.to_string().contains("atomic no-replace"));
        assert!(
            error
                .to_string()
                .contains("destination's native operating system")
        );
        let cause = error.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::Unsupported);
        assert_eq!(cause.to_string(), "original OS cause");
        assert_eq!(error.chain().count(), 2);
        assert!(staging.join("generated.txt").is_file());
        drop(pending);
        assert!(!staging.exists());
        assert!(!destination.exists());
        fs::remove_dir(root)?;
        Ok(())
    }

    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
        target_os = "redox",
    ))]
    #[test]
    fn unavailable_atomic_syscall_errors_preserve_errno_and_conditional_inval_hint()
    -> anyhow::Result<()> {
        let root = publication_fixture()?;
        for errno in [
            rustix::io::Errno::NOSYS,
            rustix::io::Errno::OPNOTSUPP,
            rustix::io::Errno::INVAL,
        ] {
            let error = publication_error(
                &root.join("staging"),
                &root.join("destination"),
                std::io::Error::from_raw_os_error(errno.raw_os_error()),
            );
            assert!(error.to_string().contains("atomic no-replace"));
            assert!(error.to_string().contains("choose an output filesystem"));
            assert_eq!(
                error
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .raw_os_error(),
                Some(errno.raw_os_error())
            );
            assert_eq!(error.chain().count(), 2);
            if errno == rustix::io::Errno::INVAL {
                assert!(error.to_string().contains("may not support"));
            }
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
        target_os = "redox",
    ))]
    #[test]
    fn invalid_publication_paths_and_existing_entries_do_not_get_filesystem_hint()
    -> anyhow::Result<()> {
        let root = publication_fixture()?;
        let staging = root.join("staging");
        let existing = root.join("existing");
        let regular_file = root.join("regular-file");
        fs::create_dir(&existing)?;
        fs::write(&regular_file, "existing")?;
        let mut cases = vec![
            (staging.clone(), staging.clone()),
            (staging.clone(), staging.join("descendant")),
            (staging.clone(), existing.clone()),
            (staging.clone(), regular_file.clone()),
            (regular_file.clone(), root.join("destination")),
            (root.join("missing-staging"), root.join("destination")),
            (staging.clone(), root.join("missing-parent/destination")),
            (staging.clone(), root.join("regular-file/destination")),
            (staging.clone(), root.join("invalid\0name")),
        ];
        #[cfg(unix)]
        {
            let dangling = root.join("dangling");
            std::os::unix::fs::symlink(root.join("missing"), &dangling)?;
            cases.push((staging.clone(), dangling.clone()));
            cases.push((dangling, root.join("destination")));
        }
        for (source, destination) in cases {
            let error = publication_error(
                &source,
                &destination,
                std::io::Error::from_raw_os_error(rustix::io::Errno::INVAL.raw_os_error()),
            );
            assert_eq!(
                error.to_string(),
                format!(
                    "publishing generated output directory {}",
                    destination.display()
                )
            );
            assert_eq!(
                error
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .raw_os_error(),
                Some(rustix::io::Errno::INVAL.raw_os_error())
            );
        }
        assert_eq!(
            fs::read_to_string(staging.join("generated.txt"))?,
            "complete source"
        );
        assert_eq!(fs::read_to_string(regular_file)?, "existing");
        assert!(fs::read_dir(existing)?.next().is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn ordinary_publication_io_errors_keep_their_original_context_and_kind() -> anyhow::Result<()> {
        let root = publication_fixture()?;
        let destination = root.join("destination");
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::AlreadyExists,
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::Other,
        ] {
            let error = publication_error(
                &root.join("staging"),
                &destination,
                std::io::Error::new(kind, "original ordinary IO cause"),
            );
            assert_eq!(
                error.to_string(),
                format!(
                    "publishing generated output directory {}",
                    destination.display()
                )
            );
            let cause = error.downcast_ref::<std::io::Error>().unwrap();
            assert_eq!(cause.kind(), kind);
            assert_eq!(cause.to_string(), "original ordinary IO cause");
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn publish_never_replaces_an_existing_directory() -> anyhow::Result<()> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_codegen_publish_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let staging = root.join("staging");
        let destination = root.join("destination");
        fs::create_dir_all(&staging)?;
        fs::write(staging.join("generated.txt"), "generated")?;
        fs::create_dir_all(&destination)?;

        let error = publish_directory(&staging, &destination)
            .expect_err("no-replace publication must reject an existing destination");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(fs::read_dir(&destination)?.next().is_none());
        assert_eq!(
            fs::read_to_string(staging.join("generated.txt"))?,
            "generated"
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    struct CsvFacadeDirectory {
        path: PathBuf,
        complete: bool,
    }

    impl CsvFacadeDirectory {
        fn new() -> std::io::Result<Self> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_csv_facade_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self {
                path,
                complete: false,
            })
        }
    }

    impl Drop for CsvFacadeDirectory {
        fn drop(&mut self) {
            if self.complete
                && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                    != Some(std::ffi::OsStr::new("1"))
            {
                let _ = fs::remove_dir_all(&self.path);
            } else {
                eprintln!("Retained CSV facade artifacts: {}", self.path.display());
            }
        }
    }

    fn csv_facade_project() -> mapping::Project {
        use ir::{ScalarType, SchemaNode};
        mapping::Project {
            source: SchemaNode::group("Source", Vec::new()).repeating(),
            target: SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: mapping::Graph {
                nodes: std::collections::BTreeMap::from([(
                    9,
                    mapping::Node::Const {
                        value: ir::Value::String("fixed".into()),
                    },
                )]),
            },
            root: mapping::Scope {
                iteration: mapping::ScopeIteration::Source(Vec::new()),
                bindings: vec![mapping::Binding {
                    target_field: "Value".into(),
                    node: 9,
                }],
                ..Default::default()
            },
        }
    }

    fn csv_facade_targets() -> [GenerateTarget; 2] {
        [
            GenerateTarget::Rust {
                runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
            },
            GenerateTarget::CSharp,
        ]
    }

    fn write_csv_facade_project(path: &Path, project: &mapping::Project) -> anyhow::Result<()> {
        fs::write(path, serde_json::to_vec(project)?)?;
        Ok(())
    }

    #[test]
    fn explicit_csv_generation_accepts_exact_stored_format_identity() -> anyhow::Result<()> {
        let mut directory = CsvFacadeDirectory::new()?;
        for (case, stored_path, fallback) in [
            ("explicit", None, None),
            ("csv", Some("result.CsV"), None),
            ("text", Some("result.txt"), None),
            (
                "unknown",
                Some("result.dat"),
                Some(mapping::TabularBoundaryKind::Csv),
            ),
            (
                "extensionless",
                Some("result"),
                Some(mapping::TabularBoundaryKind::Csv),
            ),
        ] {
            let mut project = csv_facade_project();
            project.target_path = stored_path.map(str::to_owned);
            project.target_options = mapping::FormatOptions {
                delimiter: Some(';'),
                csv_quote: Some('\''),
                csv_utf8_bom: true,
                has_header_row: Some(false),
                tabular_kind: fallback,
                ..Default::default()
            };
            let project_path = directory.path.join(format!("{case}.json"));
            write_csv_facade_project(&project_path, &project)?;
            for (language, target) in csv_facade_targets().into_iter().enumerate() {
                let output = directory.path.join(format!("{case}-{language}"));
                let outcome = generate_project_with_csv_output(&project_path, &output, target)?;
                assert_eq!(outcome.output_directory, output);
                let entry = if language == 0 {
                    "src/lib.rs"
                } else {
                    "GeneratedMapping.Csv.cs"
                };
                let source = fs::read_to_string(output.join(entry))?;
                assert!(source.contains(if language == 0 {
                    "execute_csv_bytes"
                } else {
                    "ExecuteCsvBytes"
                }));
            }
        }
        directory.complete = true;
        Ok(())
    }

    #[test]
    fn stored_non_csv_paths_win_over_csv_fallback_before_publication() -> anyhow::Result<()> {
        let mut directory = CsvFacadeDirectory::new()?;
        for (case, path) in [
            ("json", "result.json"),
            ("xml", "result.xml"),
            ("xlsx", "result.xlsx"),
        ] {
            let mut project = csv_facade_project();
            project.target_path = Some(path.into());
            project.target_options.tabular_kind = Some(mapping::TabularBoundaryKind::Csv);
            let project_path = directory.path.join(format!("{case}.project.json"));
            write_csv_facade_project(&project_path, &project)?;
            for (language, target) in csv_facade_targets().into_iter().enumerate() {
                let output = directory.path.join(format!("{case}-{language}"));
                let error = generate_project_with_csv_output(&project_path, &output, target)
                    .expect_err("the stored physical format must not silently change");
                assert!(error.to_string().contains("requires a CSV target"));
                assert!(!output.exists());
            }
        }
        assert!(fs::read_dir(&directory.path)?.all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("ferrule-stage")
        }));
        directory.complete = true;
        Ok(())
    }

    #[test]
    fn csv_repair_and_conflicting_adapters_keep_typed_prepublication_errors() -> anyhow::Result<()>
    {
        let mut directory = CsvFacadeDirectory::new()?;
        for (case, options, expected) in [
            (
                "json_lines",
                mapping::FormatOptions {
                    json_lines: true,
                    ..Default::default()
                },
                codegen::CsvOutputError::ConflictingFormatOptions,
            ),
            (
                "xlsx",
                mapping::FormatOptions {
                    tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
                    ..Default::default()
                },
                codegen::CsvOutputError::ConflictingFormatOptions,
            ),
            (
                "repair",
                mapping::FormatOptions {
                    csv_text_repair_dependency: Some(mapping::CsvTextRepairDependency::new(
                        mapping::CsvTextRepairCause::Encoding,
                    )),
                    ..Default::default()
                },
                codegen::CsvOutputError::RepairRequired,
            ),
        ] {
            let mut project = csv_facade_project();
            project.target_options = options;
            let project_path = directory.path.join(format!("{case}.json"));
            write_csv_facade_project(&project_path, &project)?;
            for (language, target) in csv_facade_targets().into_iter().enumerate() {
                let output = directory.path.join(format!("{case}-{language}"));
                let error = generate_project_with_csv_output(&project_path, &output, target)
                    .expect_err("unsupported policy must refuse before staging");
                assert_eq!(
                    error.downcast_ref::<codegen::CsvOutputError>(),
                    Some(&expected)
                );
                assert!(!output.exists());
            }
        }
        directory.complete = true;
        Ok(())
    }

    #[test]
    fn csv_project_policy_precedes_lowering_and_valid_policy_keeps_legacy_mapping_errors()
    -> anyhow::Result<()> {
        let mut directory = CsvFacadeDirectory::new()?;
        let mut project = csv_facade_project();
        project.root.bindings[0].node = 999;
        project.target_path = Some("result.json".into());
        project.target_options.delimiter = Some('\n');
        let project_path = directory.path.join("dual-invalid.json");
        write_csv_facade_project(&project_path, &project)?;
        for (language, target) in csv_facade_targets().into_iter().enumerate() {
            let output = directory.path.join(format!("policy-first-{language}"));
            let error = generate_project_with_csv_output(&project_path, &output, target)
                .expect_err("literal CSV policy precedes Project lowering and path admission");
            assert_eq!(
                error.downcast_ref::<codegen::CsvOutputError>(),
                Some(&codegen::CsvOutputError::BadDelimiter('\n'))
            );
            assert!(!output.exists());
        }
        project.target_path = Some("result.csv".into());
        project.target_options = Default::default();
        write_csv_facade_project(&project_path, &project)?;
        for (language, target) in csv_facade_targets().into_iter().enumerate() {
            let output = directory.path.join(format!("mapping-first-{language}"));
            let legacy_output = directory.path.join(format!("legacy-invalid-{language}"));
            let legacy = generate_project(&project_path, &legacy_output, target.clone())
                .expect_err("the disconnected binding is invalid");
            let error = generate_project_with_csv_output(&project_path, &output, target)
                .expect_err("valid CSV policy preserves the original mapping failure");
            assert_eq!(format!("{error:#}"), format!("{legacy:#}"));
            assert!(!output.exists());
            assert!(!legacy_output.exists());
        }
        directory.complete = true;
        Ok(())
    }
}
