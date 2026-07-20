//! Local-only reference-output generation.
//!
//! This survey is ignored by default and never runs in CI. It launches the
//! locally installed reference application under a dedicated Xvfb display,
//! rejects mappings that could publish outside the redirected directory, and
//! writes one manifest below a brand-new caller-supplied output root.
//!
//! ```text
//! FERRULE_REFERENCE_OUTPUT_DIR=/tmp/ferrule-reference-outputs \
//! FERRULE_REFERENCE_OUTPUT_FILTER=Hierarchical_JSON,SuppressNAFields \
//! cargo test -p mfd --test samples_reference_survey -- --ignored --nocapture
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const OUTPUT_DIR_ENV: &str = "FERRULE_REFERENCE_OUTPUT_DIR";
const FILTER_ENV: &str = "FERRULE_REFERENCE_OUTPUT_FILTER";
const LIMIT_ENV: &str = "FERRULE_REFERENCE_OUTPUT_LIMIT";
const DEFAULT_LIMIT: usize = 3;
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_STAGED_CONTEXT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct PreflightSkip(String);

#[derive(Debug)]
struct SafeMapping {
    database_sources: Vec<PathBuf>,
}

struct StagedMapping {
    root: PathBuf,
    design: PathBuf,
}

impl StagedMapping {
    fn design(&self) -> &Path {
        &self.design
    }
}

impl Drop for StagedMapping {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct XvfbGuard {
    child: Child,
    display: String,
}

impl Drop for XvfbGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "requires the local MapForce reference app, Wine, xdotool, and Xvfb"]
fn generate_reference_outputs() -> Result<(), Box<dyn Error>> {
    let workspace = workspace_root();
    let samples_root = workspace.join("samples");
    let samples = samples_root.join("ReferenceSamples");
    let reference_app = workspace.join("ref-app/MapForce.exe");
    if !samples.is_dir() || !reference_app.is_file() {
        println!("local samples/reference application unavailable; skipped");
        return Ok(());
    }
    require_commands(["Xvfb", "wine", "xdotool", "setsid", "kill"])?;
    require_registered_server()?;

    let output_root = std::env::var_os(OUTPUT_DIR_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{OUTPUT_DIR_ENV} must name a brand-new output directory"))?;
    create_safe_output_root(&workspace, &samples_root, &output_root)?;
    let script_path = output_root.join(".reference-output.vbs");
    fs::write(&script_path, automation_script())?;

    let filter = requested_filter();
    let limit = requested_limit()?;
    let mut designs = Vec::new();
    collect_designs(&samples, &mut designs)?;
    designs.sort();
    let designs = designs
        .into_iter()
        .filter(|path| filter_matches(path, &filter))
        .take(limit)
        .collect::<Vec<_>>();
    if designs.is_empty() {
        return Err("the reference filter selected no .mfd files".into());
    }

    let xvfb = start_xvfb()?;
    let mut records = Vec::with_capacity(designs.len());
    for (index, design) in designs.iter().enumerate() {
        let relative = design.strip_prefix(&samples)?.to_string_lossy().to_string();
        let output_dir = output_root.join(format!(
            "{index:03}-{}",
            safe_stem(
                design
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .unwrap_or("mapping")
            )
        ));
        let directory = output_dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("mapping-output")
            .to_string();
        let record = match safe_mapping(design, &samples) {
            Ok(safety) => {
                fs::create_dir(&output_dir)?;
                let staging = if safety.database_sources.is_empty() {
                    Ok(None)
                } else {
                    stage_mapping_context(
                        design,
                        &output_root.join(format!(".input-{index:03}")),
                        &safety,
                    )
                    .map(Some)
                };
                match staging {
                    Ok(staging) => {
                        let runnable_design = staging
                            .as_ref()
                            .map_or(design.as_path(), StagedMapping::design);
                        match run_mapping(&xvfb.display, &script_path, runnable_design, &output_dir)
                        {
                            Ok(output) => {
                                let outputs = collect_relative_outputs(&output_dir)?;
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let succeeded = stdout.contains("succeeded=True")
                                    && stdout.contains("result_code=0")
                                    && !outputs.is_empty();
                                serde_json::json!({
                                    "file": relative,
                                    "directory": directory,
                                    "status": if succeeded { "passed" } else { "failed" },
                                    "outputs": outputs,
                                    "host_exit_code": output.status.code(),
                                    "stdout": stdout.trim(),
                                    "stderr": String::from_utf8_lossy(&output.stderr).trim(),
                                })
                            }
                            Err(error) => serde_json::json!({
                                "file": relative,
                                "directory": directory,
                                "status": "failed",
                                "outputs": [],
                                "reason": error.to_string(),
                            }),
                        }
                    }
                    Err(error) => serde_json::json!({
                        "file": relative,
                        "directory": directory,
                        "status": "skipped",
                        "outputs": [],
                        "reason": format!("database input staging failed: {error}"),
                    }),
                }
            }
            Err(PreflightSkip(reason)) => serde_json::json!({
                "file": relative,
                "directory": directory,
                "status": "skipped",
                "outputs": [],
                "reason": reason,
            }),
        };
        println!(
            "{}: {}",
            record["status"].as_str().unwrap_or("failed"),
            design
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("mapping.mfd")
        );
        records.push(record);
    }
    let display = xvfb.display.clone();
    drop(xvfb);
    fs::remove_file(&script_path)?;

    let passed = records
        .iter()
        .filter(|record| record["status"] == "passed")
        .count();
    let failed = records
        .iter()
        .filter(|record| record["status"] == "failed")
        .count();
    let skipped = records.len() - passed - failed;
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": "ferrule.reference_outputs",
        "safety": {
            "display": display,
            "wayland_unset": true,
            "sequential_processes": true,
            "output_root_was_new": true,
            "unsafe_and_dynamic_outputs_rejected": true,
            "database_sources_copied_to_temporary_context": true,
        },
        "summary": {
            "selected": records.len(),
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
        },
        "samples": records,
    });
    let manifest_path = output_root.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    println!(
        "reference generation: {passed} passed, {failed} failed, {skipped} skipped; {}",
        manifest_path.display()
    );
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn create_safe_output_root(
    workspace: &Path,
    samples: &Path,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err(format!("reference output root already exists: {}", output.display()).into());
    }
    fs::create_dir(output)?;
    let output = output.canonicalize()?;
    let samples = samples.canonicalize()?;
    let reference_app = workspace.join("ref-app").canonicalize()?;
    if output.starts_with(samples) || output.starts_with(reference_app) {
        fs::remove_dir(&output)?;
        return Err("reference outputs cannot be placed below samples/ or ref-app/".into());
    }
    Ok(())
}

fn require_commands<const N: usize>(commands: [&str; N]) -> Result<(), Box<dyn Error>> {
    for command in commands {
        let status = Command::new("sh")
            .args(["-c", &format!("command -v '{command}' >/dev/null 2>&1")])
            .status()?;
        if !status.success() {
            return Err(format!("required local command `{command}` is unavailable").into());
        }
    }
    Ok(())
}

fn require_registered_server() -> Result<(), Box<dyn Error>> {
    let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
    let prefix = std::env::var_os("WINEPREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".wine"));
    let server = prefix.join("drive_c/Program Files/ReferenceApp/MapForce.exe");
    if !server.is_file() {
        return Err(format!(
            "the registered Wine COM server is unavailable at {}; link the local ref-app directory there without copying it",
            server.display()
        )
        .into());
    }
    Ok(())
}

fn requested_filter() -> Vec<String> {
    std::env::var(FILTER_ENV)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn requested_limit() -> Result<usize, Box<dyn Error>> {
    let limit = std::env::var(LIMIT_ENV)
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > 120 {
        return Err(format!("{LIMIT_ENV} must be between 1 and 120").into());
    }
    Ok(limit)
}

fn filter_matches(path: &Path, filter: &[String]) -> bool {
    filter.is_empty()
        || path
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|stem| filter.iter().any(|candidate| candidate == stem))
}

fn safe_stem(stem: &str) -> String {
    stem.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn collect_designs(directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_designs(&path, output)?;
        } else if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn safe_mapping(path: &Path, samples_root: &Path) -> Result<SafeMapping, PreflightSkip> {
    reject_symlink(path, "mapping design")?;
    let text = fs::read_to_string(path)
        .map_err(|error| PreflightSkip(format!("cannot read mapping: {error}")))?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|error| PreflightSkip(format!("cannot parse mapping: {error}")))?;
    for node in document.descendants().filter(|node| node.is_element()) {
        let output_instance = node.attribute("outputinstance").or_else(|| {
            (node.has_tag_name("file") && node.attribute("role") == Some("outputinstance"))
                .then(|| node.attribute("name"))
                .flatten()
        });
        if output_instance.is_some_and(|value| !is_safe_relative_output(value)) {
            return Err(PreflightSkip(
                "an output path could bypass the redirected base directory".into(),
            ));
        }
        if node.attribute("updateexistingfile") == Some("1") {
            return Err(PreflightSkip(
                "update-in-place workbook output is not reference-safe".into(),
            ));
        }
        if node
            .attribute("inputinstance")
            .is_some_and(|value| value.contains("://"))
        {
            return Err(PreflightSkip("network input is disabled".into()));
        }
        if node.has_tag_name("component") && node.attribute("library") == Some("webservice") {
            return Err(PreflightSkip("web-service components are excluded".into()));
        }
        if node.has_tag_name("entry")
            && node.attribute("name") == Some("FileInstance")
            && node.attribute("inpkey").is_some()
        {
            return Err(PreflightSkip(
                "connected target FileInstance paths are dynamic".into(),
            ));
        }
    }

    let database_sources = inspect_database_sources(path, samples_root, &document)?;
    Ok(SafeMapping { database_sources })
}

fn inspect_database_sources(
    mapping: &Path,
    samples_root: &Path,
    document: &roxmltree::Document<'_>,
) -> Result<Vec<PathBuf>, PreflightSkip> {
    let database_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("db"))
        .collect::<Vec<_>>();
    if database_components.is_empty() {
        return Ok(Vec::new());
    }
    reject_database_side_effects(document)?;

    let mut references = BTreeSet::new();
    let mut saw_boundary = false;
    for component in database_components
        .iter()
        .filter(|component| component.attribute("kind") == Some("15"))
    {
        saw_boundary = true;
        let has_input = component
            .descendants()
            .any(|node| node.has_tag_name("entry") && node.attribute("inpkey").is_some());
        let has_output = component
            .descendants()
            .any(|node| node.has_tag_name("entry") && node.attribute("outkey").is_some());
        if has_input && has_output {
            return Err(PreflightSkip(
                "a database component mixes connected source and target ports".into(),
            ));
        }
        if has_input {
            return Err(PreflightSkip(
                "connected database targets are not reference-safe".into(),
            ));
        }
        if !has_output {
            return Err(PreflightSkip(
                "database component direction is ambiguous".into(),
            ));
        }
        let refs = component
            .descendants()
            .filter(|node| node.has_tag_name("database"))
            .filter_map(|node| node.attribute("ref"))
            .collect::<BTreeSet<_>>();
        if refs.len() != 1 {
            return Err(PreflightSkip(
                "database source must reference exactly one connection".into(),
            ));
        }
        references.extend(refs.into_iter().map(str::to_owned));
    }
    if !saw_boundary || references.is_empty() {
        return Err(PreflightSkip(
            "database mapping has no connected source-only table boundary".into(),
        ));
    }

    let samples_root = fs::canonicalize(samples_root)
        .map_err(|error| PreflightSkip(format!("cannot resolve sample root: {error}")))?;
    let mapping_parent = mapping
        .parent()
        .ok_or_else(|| PreflightSkip("mapping has no parent directory".into()))?;
    let mapping_parent = fs::canonicalize(mapping_parent)
        .map_err(|error| PreflightSkip(format!("cannot resolve mapping directory: {error}")))?;
    if !mapping_parent.starts_with(&samples_root) {
        return Err(PreflightSkip(
            "mapping directory is outside the sample root".into(),
        ));
    }

    let mut sources = BTreeSet::new();
    for reference in references {
        let connections = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("database_connection")
                    && node.attribute("name") == Some(reference.as_str())
            })
            .collect::<Vec<_>>();
        if connections.len() != 1 {
            return Err(PreflightSkip(format!(
                "database source `{reference}` does not resolve to exactly one connection"
            )));
        }
        let relative = sqlite_connection_path(connections[0])?;
        reject_symlinked_relative_path(&mapping_parent, &relative)?;
        let source = fs::canonicalize(mapping_parent.join(&relative)).map_err(|error| {
            PreflightSkip(format!(
                "cannot resolve SQLite source `{}`: {error}",
                relative.display()
            ))
        })?;
        if !source.starts_with(&samples_root) || !source.is_file() {
            return Err(PreflightSkip(format!(
                "SQLite source `{}` is outside the sample root or is not a file",
                relative.display()
            )));
        }
        reject_sqlite_sidecars(&source)?;
        sources.insert(source);
    }
    Ok(sources.into_iter().collect())
}

fn reject_database_side_effects(document: &roxmltree::Document<'_>) -> Result<(), PreflightSkip> {
    for node in document.descendants().filter(|node| node.is_element()) {
        if matches!(node.tag_name().name(), "tableactions" | "action")
            && node.ancestors().any(|ancestor| {
                ancestor.has_tag_name("component") && ancestor.attribute("library") == Some("db")
            })
        {
            return Err(PreflightSkip(
                "database table actions can mutate the source".into(),
            ));
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            if name.starts_with("dbbefore")
                || name.starts_with("dbafter")
                || name.starts_with("dbsql")
                || name == "valuekeygeneration"
            {
                return Err(PreflightSkip(
                    "database mutation metadata is not reference-safe".into(),
                ));
            }
            if name == "sql" && !read_only_sql(attribute.value()) {
                return Err(PreflightSkip(
                    "database query is not a single read-only SELECT".into(),
                ));
            }
        }
        if node.has_tag_name("property")
            && node.attribute("name") == Some("Connect Script")
            && node
                .attribute("value")
                .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(PreflightSkip(
                "database connection scripts are not reference-safe".into(),
            ));
        }
    }
    Ok(())
}

fn read_only_sql(sql: &str) -> bool {
    let sql = sql.trim();
    let statement = sql.strip_suffix(';').unwrap_or(sql).trim();
    if statement.contains(';') {
        return false;
    }
    let lower = statement.to_ascii_lowercase();
    if !(lower.starts_with("select ") || lower.starts_with("with ")) {
        return false;
    }
    !lower
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| {
            matches!(
                token,
                "insert"
                    | "update"
                    | "delete"
                    | "replace"
                    | "create"
                    | "alter"
                    | "drop"
                    | "attach"
                    | "detach"
                    | "vacuum"
                    | "reindex"
                    | "pragma"
            )
        })
}

fn sqlite_connection_path(connection: roxmltree::Node<'_, '_>) -> Result<PathBuf, PreflightSkip> {
    let kinds = [
        connection.attribute("database_kind"),
        connection.attribute("import_kind"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if kinds.is_empty()
        || kinds
            .iter()
            .any(|kind| !kind.eq_ignore_ascii_case("SQLite"))
    {
        return Err(PreflightSkip(
            "only local SQLite database sources are reference-safe".into(),
        ));
    }

    let mut declared = Vec::new();
    if let Some(value) = connection.attribute("ConnectionString") {
        declared.push(value);
    }
    if let Some(datasource) = connection
        .ancestors()
        .find(|ancestor| ancestor.has_tag_name("datasource"))
        && let Some(properties) = datasource
            .children()
            .find(|child| child.has_tag_name("properties"))
    {
        if let Some(value) = properties.attribute("DBDataSource") {
            declared.push(value);
        }
        if let Some(value) = properties.attribute("JDBCDatabaseURL") {
            declared.push(
                value.strip_prefix("jdbc:sqlite:").ok_or_else(|| {
                    PreflightSkip("SQLite JDBC URL is not a local file path".into())
                })?,
            );
        }
    }
    declared.extend(
        connection
            .descendants()
            .filter(|node| {
                node.has_tag_name("property") && node.attribute("name") == Some("Data Source")
            })
            .filter_map(|node| node.attribute("value")),
    );
    let paths = declared
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .map(portable_database_path)
        .collect::<Result<BTreeSet<_>, _>>()?;
    if paths.len() != 1 {
        return Err(PreflightSkip(
            "SQLite connection path is missing or ambiguous".into(),
        ));
    }
    paths
        .into_iter()
        .next()
        .ok_or_else(|| PreflightSkip("SQLite connection path is missing".into()))
}

fn portable_database_path(value: &str) -> Result<PathBuf, PreflightSkip> {
    let portable = value.trim().replace('\\', "/");
    if portable.is_empty()
        || portable.contains("://")
        || portable.starts_with('/')
        || portable
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
    {
        return Err(PreflightSkip(format!(
            "SQLite source path `{value}` is not a contained relative path"
        )));
    }
    let path = Path::new(&portable);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PreflightSkip(format!(
            "SQLite source path `{value}` is not a contained relative path"
        )));
    }
    Ok(path.to_path_buf())
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), PreflightSkip> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PreflightSkip(format!("cannot inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() {
        return Err(PreflightSkip(format!("{label} cannot be a symlink")));
    }
    Ok(())
}

fn reject_symlinked_relative_path(root: &Path, relative: &Path) -> Result<(), PreflightSkip> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(PreflightSkip(
                "SQLite source path is not a contained relative path".into(),
            ));
        };
        current.push(segment);
        reject_symlink(&current, "SQLite source path")?;
    }
    Ok(())
}

fn reject_sqlite_sidecars(source: &Path) -> Result<(), PreflightSkip> {
    let Some(name) = source.file_name() else {
        return Err(PreflightSkip("SQLite source has no filename".into()));
    };
    let name = name.to_string_lossy();
    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = source.with_file_name(format!("{name}{suffix}"));
        if sidecar.exists() {
            return Err(PreflightSkip(format!(
                "SQLite source has an active sidecar `{}`",
                sidecar.display()
            )));
        }
    }
    Ok(())
}

fn stage_mapping_context(
    design: &Path,
    stage_root: &Path,
    safety: &SafeMapping,
) -> Result<StagedMapping, Box<dyn Error>> {
    if stage_root.exists() {
        return Err(format!(
            "database input staging directory already exists: {}",
            stage_root.display()
        )
        .into());
    }
    let source_root = fs::canonicalize(
        design
            .parent()
            .ok_or("mapping design has no parent directory")?,
    )?;
    let context = stage_root.join("context");
    let mut copied_bytes = 0_u64;
    if let Err(error) = copy_context(&source_root, &context, &mut copied_bytes) {
        let _ = fs::remove_dir_all(stage_root);
        return Err(error.into());
    }
    let staged_design = context.join(design.file_name().ok_or("mapping design has no filename")?);
    for source in &safety.database_sources {
        let relative = source.strip_prefix(&source_root).map_err(|_| {
            format!(
                "SQLite source `{}` is outside the staged mapping context",
                source.display()
            )
        })?;
        let staged = context.join(relative);
        if !staged.is_file() || fs::canonicalize(&staged)? == fs::canonicalize(source)? {
            let _ = fs::remove_dir_all(stage_root);
            return Err(format!(
                "SQLite source `{}` was not copied into the temporary context",
                source.display()
            )
            .into());
        }
    }
    Ok(StagedMapping {
        root: stage_root.to_path_buf(),
        design: staged_design,
    })
}

fn copy_context(source: &Path, target: &Path, copied_bytes: &mut u64) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source = entry.path();
        let target = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "staged mapping context contains symlink `{}`",
                    source.display()
                ),
            ));
        }
        if metadata.is_dir() {
            copy_context(&source, &target, copied_bytes)?;
        } else if metadata.is_file() {
            *copied_bytes = copied_bytes.checked_add(metadata.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "staged context size overflow")
            })?;
            if *copied_bytes > MAX_STAGED_CONTEXT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "staged mapping context exceeds the 256 MiB safety limit",
                ));
            }
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

fn is_safe_relative_output(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with(['/', '\\'])
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
        || value.split_once("://").is_some()
    {
        return false;
    }
    !value
        .replace('\\', "/")
        .split('/')
        .any(|segment| segment == "..")
}

fn start_xvfb() -> Result<XvfbGuard, Box<dyn Error>> {
    for number in 120..200 {
        let display = format!(":{number}");
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let lock = PathBuf::from(format!("/tmp/.X{number}-lock"));
        if socket.exists() || lock.exists() {
            continue;
        }
        let mut child = Command::new("Xvfb")
            .args([&display, "-screen", "0", "1600x900x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if socket.exists() {
                return Ok(XvfbGuard { child, display });
            }
            if child.try_wait()?.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    Err("could not allocate an isolated Xvfb display".into())
}

fn run_mapping(
    display: &str,
    script: &Path,
    design: &Path,
    output_dir: &Path,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new("setsid");
    command
        .arg("wine")
        .arg("cscript.exe")
        .arg("//Nologo")
        .arg(wine_path(script)?)
        .arg(wine_path(design)?)
        .arg(wine_path(output_dir)?)
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", display)
        .env("WINEDEBUG", "-all")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let process_group = child.id();
    let deadline = Instant::now() + SAMPLE_TIMEOUT;
    loop {
        dismiss_completion_modal(display);
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(Into::into);
        }
        if Instant::now() >= deadline {
            terminate_process_group(process_group);
            let _ = child.wait();
            return Err(format!("MapForce generation exceeded {SAMPLE_TIMEOUT:?}").into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn dismiss_completion_modal(display: &str) {
    let search = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "^MapForce$"])
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", display)
        .output();
    let Ok(search) = search else {
        return;
    };
    for id in String::from_utf8_lossy(&search.stdout).split_whitespace() {
        let focused = Command::new("xdotool")
            .args(["windowfocus", id])
            .env_remove("WAYLAND_DISPLAY")
            .env("DISPLAY", display)
            .status()
            .is_ok_and(|status| status.success());
        if focused {
            let _ = Command::new("xdotool")
                .args(["key", "Return"])
                .env_remove("WAYLAND_DISPLAY")
                .env("DISPLAY", display)
                .status();
        }
    }
}

fn terminate_process_group(process_group: u32) {
    let _ = Command::new("kill")
        .args(["--", &format!("-{process_group}")])
        .status();
}

fn wine_path(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = path.canonicalize()?;
    let path = path
        .to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))?;
    Ok(format!("Z:{}", path.replace('/', "\\")))
}

fn collect_relative_outputs(directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let root = directory.canonicalize()?;
    let mut paths = Vec::new();
    collect_output_files(directory, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let path = path.canonicalize()?;
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| "generated output escaped its assigned directory")?;
            Ok(relative.to_string_lossy().to_string())
        })
        .collect()
}

fn collect_output_files(directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_output_files(&path, output)?;
        } else {
            output.push(path);
        }
    }
    Ok(())
}

fn automation_script() -> &'static str {
    r#"Option Explicit
If WScript.Arguments.Count <> 2 Then WScript.Quit 2
Dim application, document, result, succeeded
On Error Resume Next
Set application = CreateObject("MapForce_x64.Application")
If Err.Number <> 0 Then
  WScript.Echo "create application failed: " & Err.Description
  WScript.Quit 3
End If
application.Visible = True
Set document = application.OpenMapping(WScript.Arguments(0))
If Err.Number <> 0 Then
  WScript.Echo "open mapping failed: " & Err.Description
  application.Quit
  WScript.Quit 4
End If
Set result = document.GenerateOutputEx2(WScript.Arguments(1))
If Err.Number <> 0 Then
  WScript.Echo "generate output failed: " & Err.Description
  application.Quit
  WScript.Quit 5
End If
succeeded = result.Succeeded
WScript.Echo "succeeded=" & CStr(succeeded)
WScript.Echo "result_code=" & CStr(result.ResultCode)
WScript.Echo "messages=" & result.MessageText
application.Quit
If CBool(succeeded) = False Then WScript.Quit 6
"#
}

#[test]
fn safety_preflight_rejects_external_publication_and_network_inputs() {
    let cases = [
        r#"<mapping><document outputinstance="../outside.xml"/></mapping>"#,
        r#"<mapping><document outputinstance="C:\outside.xml"/></mapping>"#,
        r#"<mapping><document inputinstance="https://example.test/input.xml"/></mapping>"#,
        r#"<mapping><entry name="FileInstance" inpkey="7"/></mapping>"#,
        r#"<mapping><component library="db"/></mapping>"#,
        r#"<mapping><excel updateexistingfile="1"/></mapping>"#,
    ];
    let directory = TestDir::new("unsafe");
    for (index, contents) in cases.iter().enumerate() {
        let path = directory.0.join(format!("case-{index}.mfd"));
        fs::write(&path, contents).unwrap();
        assert!(safe_mapping(&path, &directory.0).is_err(), "{contents}");
    }
}

#[test]
fn safety_preflight_accepts_local_inputs_and_pathless_outputs() {
    let directory = TestDir::new("safe");
    let path = directory.0.join("safe.mfd");
    fs::write(
        &path,
        r#"<mapping><component library="xml"><document inputinstance="input.xml" outputinstance="unused-source.xml"/></component><component library="json"><json outputinstance="result.json"/></component></mapping>"#,
    )
    .unwrap();
    assert!(safe_mapping(&path, &directory.0).is_ok());
}

#[test]
fn database_source_preflight_stages_an_independent_local_context() {
    let directory = TestDir::new("database_source");
    let samples = directory.0.join("samples");
    fs::create_dir(&samples).unwrap();
    let database = samples.join("source.sqlite");
    fs::write(&database, b"SQLite format 3\0self-authored fixture").unwrap();
    fs::write(samples.join("source.xsd"), "<schema/>").unwrap();
    let mapping = samples.join("source.mfd");
    fs::write(
        &mapping,
        database_mapping("source.sqlite", r#"<entry name="Rows" outkey="1"/>"#, ""),
    )
    .unwrap();

    let safety = safe_mapping(&mapping, &samples).unwrap();
    assert_eq!(
        safety.database_sources,
        vec![database.canonicalize().unwrap()]
    );
    let stage_root = directory.0.join("stage");
    let staged = stage_mapping_context(&mapping, &stage_root, &safety).unwrap();
    assert!(staged.design().is_file());
    assert_eq!(
        fs::read(stage_root.join("context/source.sqlite")).unwrap(),
        b"SQLite format 3\0self-authored fixture"
    );
    assert!(stage_root.join("context/source.xsd").is_file());
    fs::write(stage_root.join("context/source.sqlite"), b"changed").unwrap();
    assert_eq!(
        fs::read(&database).unwrap(),
        b"SQLite format 3\0self-authored fixture"
    );
    drop(staged);
    assert!(!stage_root.exists());
}

#[test]
fn database_source_preflight_rejects_target_mixed_and_mutating_components() {
    let directory = TestDir::new("database_direction");
    let database = directory.0.join("source.sqlite");
    fs::write(&database, b"SQLite format 3\0self-authored fixture").unwrap();
    let cases = [
        (r#"<entry name="Rows" inpkey="1"/>"#, "", "connected target"),
        (
            r#"<entry name="Rows" inpkey="1" outkey="2"/>"#,
            "",
            "mixed boundary",
        ),
        (
            r#"<entry name="Rows" outkey="1"/>"#,
            r#"<LocalViewStorage><LocalViewElement SQL="DELETE FROM Rows"/></LocalViewStorage>"#,
            "mutating query",
        ),
        (
            r#"<entry name="Rows" outkey="1"><tableactions><action operation="update"/></tableactions></entry>"#,
            "",
            "table action",
        ),
    ];
    for (index, (entry, connection_extra, label)) in cases.iter().enumerate() {
        let mapping = directory.0.join(format!("case-{index}.mfd"));
        fs::write(
            &mapping,
            database_mapping("source.sqlite", entry, connection_extra),
        )
        .unwrap();
        assert!(
            safe_mapping(&mapping, &directory.0).is_err(),
            "{label} was accepted"
        );
    }
}

#[test]
fn database_source_preflight_rejects_missing_ambiguous_and_sidecar_paths() {
    let directory = TestDir::new("database_paths");
    let database = directory.0.join("source.sqlite");
    fs::write(&database, b"SQLite format 3\0self-authored fixture").unwrap();

    let missing = directory.0.join("missing.mfd");
    fs::write(
        &missing,
        database_mapping("missing.sqlite", r#"<entry outkey="1"/>"#, ""),
    )
    .unwrap();
    assert!(safe_mapping(&missing, &directory.0).is_err());

    let outside = directory.0.join("outside.mfd");
    fs::write(
        &outside,
        database_mapping("../source.sqlite", r#"<entry outkey="1"/>"#, ""),
    )
    .unwrap();
    assert!(safe_mapping(&outside, &directory.0).is_err());

    let ambiguous = directory.0.join("ambiguous.mfd");
    let text = database_mapping("source.sqlite", r#"<entry outkey="1"/>"#, "").replace(
        "DBDataSource=\"source.sqlite\"",
        "DBDataSource=\"other.sqlite\"",
    );
    fs::write(&ambiguous, text).unwrap();
    assert!(safe_mapping(&ambiguous, &directory.0).is_err());

    let sidecar = directory.0.join("sidecar.mfd");
    fs::write(
        &sidecar,
        database_mapping("source.sqlite", r#"<entry outkey="1"/>"#, ""),
    )
    .unwrap();
    fs::write(directory.0.join("source.sqlite-wal"), b"active").unwrap();
    assert!(safe_mapping(&sidecar, &directory.0).is_err());
}

#[cfg(unix)]
#[test]
fn database_source_preflight_rejects_symlinked_database_paths() {
    let directory = TestDir::new("database_symlink");
    let database = directory.0.join("real.sqlite");
    fs::write(&database, b"SQLite format 3\0self-authored fixture").unwrap();
    std::os::unix::fs::symlink(&database, directory.0.join("source.sqlite")).unwrap();
    let mapping = directory.0.join("source.mfd");
    fs::write(
        &mapping,
        database_mapping("source.sqlite", r#"<entry outkey="1"/>"#, ""),
    )
    .unwrap();
    assert!(safe_mapping(&mapping, &directory.0).is_err());
}

#[test]
fn wine_paths_use_the_local_z_drive_without_loss() {
    let directory = TestDir::new("wine_path");
    let path = directory.0.join("mapping.mfd");
    fs::write(&path, "<mapping/>").unwrap();
    let converted = wine_path(&path).unwrap();
    assert!(converted.starts_with("Z:\\"), "{converted}");
    assert!(converted.ends_with("\\mapping.mfd"), "{converted}");
}

fn database_mapping(database: &str, entries: &str, connection_extra: &str) -> String {
    format!(
        r#"<mapping>
  <resources><datasources><datasource name="Source">
    <properties JDBCDriver="org.sqlite.JDBC" JDBCDatabaseURL="jdbc:sqlite:{database}" DBDataSource="{database}"/>
    <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="{database}" name="Source">
      <properties><property name="Data Source" value="{database}"/><property name="Connect Script"/></properties>
      {connection_extra}
    </database_connection>
  </datasource></datasources></resources>
  <component library="db" kind="15"><data><root><entry name="document">{entries}</entry></root><database ref="Source"/></data></component>
</mapping>"#
    )
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_reference_output_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
