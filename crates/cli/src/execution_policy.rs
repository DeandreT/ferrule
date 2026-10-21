//! Host-supplied restricted execution policies for filesystem runs. See
//! `docs/design/execution-profiles.md`; this is policy enforcement, not a sandbox.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use engine::{ExecutionBudgets, ExecutionPolicy, RestrictedPolicy, RootAccess};
use mapping::FormatOptions;
use serde::Deserialize;

/// The profile every route uses when the host supplies none.
pub(crate) static TRUSTED: ExecutionPolicy = ExecutionPolicy::Trusted;

const POLICY_FILE_VERSION: u32 = 1;
const MAX_POLICY_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    version: u32,
    #[serde(default)]
    filesystem: FilesystemRoots,
    #[serde(default)]
    http_origins: Vec<String>,
    budgets: Budgets,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesystemRoots {
    #[serde(default)]
    read: Vec<PathBuf>,
    #[serde(default)]
    read_write: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budgets {
    work: u64,
    depth: u64,
    join_rows: u64,
}

/// Loads a versioned JSON restricted policy. Relative roots resolve from the
/// policy file's directory; omitted roots and origins deny those rights.
pub fn load_execution_policy_file(path: &Path) -> anyhow::Result<ExecutionPolicy> {
    let mut text = String::new();
    std::fs::File::open(path)
        .with_context(|| format!("opening restricted policy {}", path.display()))?
        .take(MAX_POLICY_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .with_context(|| format!("reading restricted policy {}", path.display()))?;
    if text.len() as u64 > MAX_POLICY_FILE_BYTES {
        bail!(
            "restricted policy {} exceeds {} KiB",
            path.display(),
            MAX_POLICY_FILE_BYTES / 1024
        );
    }
    let base = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parse_execution_policy(&text, base)
        .with_context(|| format!("loading restricted policy {}", path.display()))
}

/// Parses a policy document, resolving relative roots from `base`.
pub fn parse_execution_policy(json: &str, base: &Path) -> anyhow::Result<ExecutionPolicy> {
    let file: PolicyFile = serde_json::from_str(json)?;
    if file.version != POLICY_FILE_VERSION {
        bail!(
            "unsupported restricted policy version {}; expected {POLICY_FILE_VERSION}",
            file.version
        );
    }
    let mut policy = RestrictedPolicy::new(ExecutionBudgets::new(
        file.budgets.work,
        file.budgets.depth,
        file.budgets.join_rows,
    ));
    let roots = file
        .filesystem
        .read
        .into_iter()
        .map(|root| (root, RootAccess::Read))
        .chain(
            file.filesystem
                .read_write
                .into_iter()
                .map(|root| (root, RootAccess::ReadWrite)),
        );
    for (root, access) in roots {
        policy = policy.allow_root(base.join(root), access)?;
    }
    for origin in &file.http_origins {
        policy = policy.allow_origin(origin)?;
    }
    Ok(ExecutionPolicy::Restricted(policy))
}

/// The location a read acquires: a local XML file set reads its base
/// directory, which the reader already confines its members to.
pub(crate) fn read_location<'a>(path: &'a Path, options: &FormatOptions) -> &'a Path {
    if options.xml_document && options.local_xml_file_set {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    } else {
        path
    }
}
