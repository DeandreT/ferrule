//! Trusted and restricted execution profiles. See
//! `docs/design/execution-profiles.md`: a restricted policy is enforced inside
//! the process at acquisition and evaluation points; it is not a sandbox.

use std::cell::Cell;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::EngineError;

/// Host authority for one execution. Trusted is the default and performs no
/// checks; restricted admits only the listed roots, origins and budgets.
#[derive(Debug, Clone, Default)]
pub enum ExecutionPolicy {
    #[default]
    Trusted,
    Restricted(RestrictedPolicy),
}

/// A deny-by-default host policy: no roots, no origins and explicit budgets.
#[derive(Debug, Clone)]
pub struct RestrictedPolicy {
    roots: Vec<AllowedRoot>,
    origins: Vec<HttpOrigin>,
    budgets: ExecutionBudgets,
}

#[derive(Debug, Clone)]
struct AllowedRoot {
    path: PathBuf,
    access: RootAccess,
}

/// What a filesystem root admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootAccess {
    Read,
    ReadWrite,
}

/// The right one acquisition needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAccess {
    Read,
    Write,
}

/// Explicit per-evaluation limits. Each admits exactly its count; the next
/// unit fails with [`EngineError::ExecutionBudgetExceeded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudgets {
    work: u64,
    depth: u64,
    join_rows: u64,
}

impl ExecutionBudgets {
    pub const fn new(work: u64, depth: u64, join_rows: u64) -> Self {
        Self {
            work,
            depth,
            join_rows,
        }
    }

    pub const fn work(self) -> u64 {
        self.work
    }

    pub const fn depth(self) -> u64 {
        self.depth
    }

    pub const fn join_rows(self) -> u64 {
        self.join_rows
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBudgetKind {
    /// Graph-node evaluation attempts, including user-function body nodes.
    Work,
    /// Simultaneously active graph-node evaluations.
    Depth,
    /// Candidate tuples examined by inner-join stages.
    JoinRows,
}

/// A capability a restricted run cannot grant because the policy cannot check it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestrictedCapability {
    /// Following an HTTP redirect to a location the policy has not checked.
    HttpRedirect,
    /// A host dynamic-source loader that does not implement `load_restricted`.
    UnconfinedDynamicSourceLoader,
}

/// A typed refusal raised before acquisition.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PolicyRefusal {
    #[error("restricted policy refuses {access:?} access to `{}`: it resolves to `{}`, outside every allowed root", .path.display(), .resolved.display())]
    PathOutsideRoots {
        path: PathBuf,
        resolved: PathBuf,
        access: PathAccess,
    },
    #[error("restricted policy cannot resolve `{}` for {access:?} access: {reason}", .path.display())]
    UnresolvablePath {
        path: PathBuf,
        access: PathAccess,
        reason: String,
    },
    #[error("restricted policy refuses HTTP origin `{origin}`")]
    HttpOriginDenied { origin: String },
    #[error("restricted policy refuses malformed HTTP URL `{url}`")]
    InvalidHttpUrl { url: String },
    #[error("restricted policy does not support {capability:?}")]
    UnsupportedCapability { capability: RestrictedCapability },
}

/// Why a policy-aware dynamic-source load did not produce a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicSourceLoadError {
    /// The policy refused the location before acquisition.
    Refused(PolicyRefusal),
    /// The admitted location could not be read or parsed.
    Failed(String),
}

/// A policy that cannot be constructed.
#[derive(Debug, Error)]
pub enum PolicyConfigError {
    #[error("allowed root `{}` cannot be resolved: {source}", .path.display())]
    RootIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("allowed root `{}` is not a directory", .path.display())]
    RootNotDirectory { path: PathBuf },
    #[error("allowed HTTP origin `{origin}` must be `http(s)://host[:port]`")]
    InvalidOrigin { origin: String },
}

impl RestrictedPolicy {
    /// Starts with no filesystem roots and no HTTP origins.
    pub fn new(budgets: ExecutionBudgets) -> Self {
        Self {
            roots: Vec::new(),
            origins: Vec::new(),
            budgets,
        }
    }

    /// Admits `access` beneath an existing directory, canonicalized now.
    pub fn allow_root(
        mut self,
        path: impl AsRef<Path>,
        access: RootAccess,
    ) -> Result<Self, PolicyConfigError> {
        let path = path.as_ref();
        let canonical =
            std::fs::canonicalize(path).map_err(|source| PolicyConfigError::RootIo {
                path: path.to_path_buf(),
                source,
            })?;
        if !canonical.is_dir() {
            return Err(PolicyConfigError::RootNotDirectory {
                path: path.to_path_buf(),
            });
        }
        self.roots.push(AllowedRoot {
            path: canonical,
            access,
        });
        Ok(self)
    }

    /// Admits one exact `scheme://host[:port]` origin.
    pub fn allow_origin(mut self, origin: &str) -> Result<Self, PolicyConfigError> {
        let invalid = || PolicyConfigError::InvalidOrigin {
            origin: origin.to_string(),
        };
        let (parsed, rest) = HttpOrigin::parse(origin).ok_or_else(invalid)?;
        if !matches!(rest, "" | "/") {
            return Err(invalid());
        }
        self.origins.push(parsed);
        Ok(self)
    }

    pub const fn budgets(&self) -> ExecutionBudgets {
        self.budgets
    }

    /// Checks one local filesystem location without opening it.
    pub fn authorize_path(&self, path: &Path, access: PathAccess) -> Result<(), PolicyRefusal> {
        let resolved = match access {
            PathAccess::Read => std::path::absolute(path)
                .and_then(std::fs::canonicalize)
                .map_err(|error| unresolvable(path, access, error.to_string()))?,
            PathAccess::Write => resolve_for_write(path)?,
        };
        let admitted = self.roots.iter().any(|root| {
            (access == PathAccess::Read || root.access == RootAccess::ReadWrite)
                && resolved.starts_with(&root.path)
        });
        if admitted {
            Ok(())
        } else {
            Err(PolicyRefusal::PathOutsideRoots {
                path: path.to_path_buf(),
                resolved,
                access,
            })
        }
    }

    /// Checks a read location: an `http(s)://` URL by origin, anything else as
    /// a local path.
    pub fn authorize_read(&self, location: &Path) -> Result<(), PolicyRefusal> {
        match http_location(location) {
            Some(url) => self.authorize_url(url),
            None => self.authorize_path(location, PathAccess::Read),
        }
    }

    /// Checks the origin of one HTTP(S) URL without sending a request.
    pub fn authorize_url(&self, url: &str) -> Result<(), PolicyRefusal> {
        let (origin, _) = HttpOrigin::parse(url).ok_or_else(|| PolicyRefusal::InvalidHttpUrl {
            url: redact_url(url),
        })?;
        if self.origins.contains(&origin) {
            Ok(())
        } else {
            Err(PolicyRefusal::HttpOriginDenied {
                origin: origin.to_string(),
            })
        }
    }
}

impl ExecutionPolicy {
    pub const fn is_restricted(&self) -> bool {
        matches!(self, Self::Restricted(_))
    }

    pub const fn restricted(&self) -> Option<&RestrictedPolicy> {
        match self {
            Self::Trusted => None,
            Self::Restricted(policy) => Some(policy),
        }
    }

    /// Trusted admits every read without touching the filesystem.
    pub fn authorize_read(&self, location: &Path) -> Result<(), PolicyRefusal> {
        self.restricted()
            .map_or(Ok(()), |policy| policy.authorize_read(location))
    }

    pub fn authorize_write(&self, path: &Path) -> Result<(), PolicyRefusal> {
        self.restricted().map_or(Ok(()), |policy| {
            policy.authorize_path(path, PathAccess::Write)
        })
    }

    pub fn authorize_url(&self, url: &str) -> Result<(), PolicyRefusal> {
        self.restricted()
            .map_or(Ok(()), |policy| policy.authorize_url(url))
    }

    /// Refuses `capability` under a restricted policy.
    pub fn authorize_capability(
        &self,
        capability: RestrictedCapability,
    ) -> Result<(), PolicyRefusal> {
        match self {
            Self::Trusted => Ok(()),
            Self::Restricted(_) => Err(PolicyRefusal::UnsupportedCapability { capability }),
        }
    }
}

/// Matches the CLI's notion of an HTTP location: an `http` or `https` scheme
/// followed by `://`, compared case-insensitively.
fn http_location(path: &Path) -> Option<&str> {
    let value = path.to_str()?;
    let (scheme, _) = value.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")).then_some(value)
}

fn unresolvable(path: &Path, access: PathAccess, reason: String) -> PolicyRefusal {
    PolicyRefusal::UnresolvablePath {
        path: path.to_path_buf(),
        access,
        reason,
    }
}

/// Canonicalizes the longest existing ancestor and appends the missing tail,
/// which may contain only plain names: `..` after a missing directory has no
/// filesystem meaning to check.
fn resolve_for_write(path: &Path) -> Result<PathBuf, PolicyRefusal> {
    let access = PathAccess::Write;
    let absolute =
        std::path::absolute(path).map_err(|error| unresolvable(path, access, error.to_string()))?;
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = existing.parent() else {
                    return Err(unresolvable(path, access, "no existing ancestor".into()));
                };
                missing.push(
                    existing
                        .components()
                        .next_back()
                        .ok_or_else(|| unresolvable(path, access, "empty path".into()))?,
                );
                existing = parent;
            }
            Err(error) => return Err(unresolvable(path, access, error.to_string())),
        }
    }
    let mut resolved = std::fs::canonicalize(existing)
        .map_err(|error| unresolvable(path, access, error.to_string()))?;
    for component in missing.into_iter().rev() {
        match component {
            Component::Normal(name) => resolved.push(name),
            Component::CurDir => {}
            _ => {
                return Err(unresolvable(
                    path,
                    access,
                    "`..` follows a directory that does not exist".into(),
                ));
            }
        }
    }
    Ok(resolved)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpOrigin {
    https: bool,
    host: String,
    port: u16,
}

impl HttpOrigin {
    /// Parses `scheme://authority` and returns the origin with the remaining
    /// path, query and fragment. User-info and characters outside the
    /// authority grammar are refused rather than normalized.
    fn parse(url: &str) -> Option<(Self, &str)> {
        let (scheme, rest) = url.split_once("://")?;
        let https = if scheme.eq_ignore_ascii_case("https") {
            true
        } else if scheme.eq_ignore_ascii_case("http") {
            false
        } else {
            return None;
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, remainder) = rest.split_at(end);
        if authority.is_empty()
            || !authority
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_~:[]".contains(&byte))
        {
            return None;
        }
        let (host, port) = if authority.starts_with('[') {
            let close = authority.find(']')?;
            if close < 2 || authority[1..close].contains('[') {
                return None;
            }
            let host = &authority[..=close];
            match &authority[close + 1..] {
                "" => (host, None),
                after => (host, Some(after.strip_prefix(':')?)),
            }
        } else {
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if host.is_empty() || (!host.starts_with('[') && host.contains(['[', ']', ':'])) {
            return None;
        }
        let port = match port {
            None => {
                if https {
                    443
                } else {
                    80
                }
            }
            Some(port) if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) => {
                port.parse().ok()?
            }
            Some(_) => return None,
        };
        Some((
            Self {
                https,
                host: host.to_ascii_lowercase(),
                port,
            },
            remainder,
        ))
    }
}

impl std::fmt::Display for HttpOrigin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        write!(formatter, "{scheme}://{}:{}", self.host, self.port)
    }
}

/// Drops query, fragment and any user-info before a URL enters an error.
fn redact_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let url = &url[..end];
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let authority_end = rest.find('/').unwrap_or(rest.len());
            let (authority, path) = rest.split_at(authority_end);
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            format!("{scheme}://{host}{path}")
        }
        None => url.to_string(),
    }
}

/// Per-evaluation budget counters for a restricted run.
pub(crate) struct BudgetMeter {
    budgets: ExecutionBudgets,
    work: Cell<u64>,
    depth: Cell<u64>,
    join_rows: Cell<u64>,
}

/// Releases one level of active evaluation depth when dropped.
pub(crate) struct DepthGuard<'a>(&'a BudgetMeter);

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get().saturating_sub(1));
    }
}

impl BudgetMeter {
    pub(crate) fn new(budgets: ExecutionBudgets) -> Self {
        Self {
            budgets,
            work: Cell::new(0),
            depth: Cell::new(0),
            join_rows: Cell::new(0),
        }
    }

    /// Charges one node evaluation and enters one depth level. Both limits are
    /// checked before either counter changes; work is reported first.
    pub(crate) fn enter_node(&self) -> Result<DepthGuard<'_>, EngineError> {
        let work = self.work.get();
        if work >= self.budgets.work {
            return Err(exceeded(ExecutionBudgetKind::Work, self.budgets.work));
        }
        let depth = self.depth.get();
        if depth >= self.budgets.depth {
            return Err(exceeded(ExecutionBudgetKind::Depth, self.budgets.depth));
        }
        self.work.set(work + 1);
        self.depth.set(depth + 1);
        Ok(DepthGuard(self))
    }

    /// Charges one candidate join tuple before its keys are compared.
    pub(crate) fn charge_join_row(&self) -> Result<(), EngineError> {
        let rows = self.join_rows.get();
        if rows >= self.budgets.join_rows {
            return Err(exceeded(
                ExecutionBudgetKind::JoinRows,
                self.budgets.join_rows,
            ));
        }
        self.join_rows.set(rows + 1);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn counters(&self) -> (u64, u64, u64) {
        (self.work.get(), self.depth.get(), self.join_rows.get())
    }
}

fn exceeded(kind: ExecutionBudgetKind, limit: u64) -> EngineError {
    EngineError::ExecutionBudgetExceeded { kind, limit }
}

#[cfg(test)]
#[path = "tests/policy.rs"]
mod tests;
