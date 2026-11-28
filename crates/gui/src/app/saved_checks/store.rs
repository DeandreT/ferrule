//! Check sets belong to the saved mapping filename. No sidecar means an older
//! mapping (or a new one) with no checks. A save replaces one complete sidecar;
//! it never writes the mapping, the original reference, or the preview.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use editor_ui::mapping_checks::NamedCheck;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::preview_table::PreviewTable;
use crate::run_report::MAX_PREVIEW_BYTES;

const VERSION: u32 = 1;
const MAX_SIDECAR_BYTES: usize = 16 * MAX_PREVIEW_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Output {
    pub target: String,
    pub artifact: PathBuf,
}

impl Output {
    pub fn matches(&self, target: &str, path: &Path) -> bool {
        self.target == target && self.artifact == path
    }

    pub fn label(&self) -> String {
        format!("{} · {}", self.target, self.artifact.display())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    pub path: PathBuf,
    // Keep the original bytes, including the BOM and any quoted newlines.
    bytes_base64: String,
    decoded_sha256: [u8; 32],
}

impl Reference {
    pub fn capture(
        path: &Path,
        options: &mapping::FormatOptions,
        schema: &ir::SchemaNode,
    ) -> Result<(Self, PreviewTable)> {
        let bytes = read_bounded(path, MAX_PREVIEW_BYTES)?;
        let table = PreviewTable::parse(&bytes, options, schema).map_err(anyhow::Error::msg)?;
        let reference = Self {
            path: path.to_owned(),
            bytes_base64: STANDARD.encode(&bytes),
            decoded_sha256: table_digest(&table),
        };
        Ok((reference, table))
    }

    /// Only used on explicit request to add an assertion in the editor. Simply
    /// reopening or running saved checks never reads or decodes the reference.
    pub fn rows_for_edit(
        &self,
        options: &mapping::FormatOptions,
        schema: &ir::SchemaNode,
    ) -> Result<PreviewTable> {
        let bytes = STANDARD
            .decode(&self.bytes_base64)
            .context("Invalid saved reference bytes")?;
        let table = PreviewTable::parse(&bytes, options, schema).map_err(anyhow::Error::msg)?;
        ensure!(
            table_digest(&table) == self.decoded_sha256,
            "The current CSV settings no longer decode the saved reference the same way."
        );
        Ok(table)
    }
}

/// Include row boundaries, positions and raw decoded strings, so replacing a
/// capture at the same path cannot turn an old result into a link to new data.
pub(super) fn table_digest(table: &PreviewTable) -> [u8; 32] {
    fn record(hash: &mut Sha256, cells: &[String]) {
        hash.update((cells.len() as u64).to_le_bytes());
        for cell in cells {
            hash.update((cell.len() as u64).to_le_bytes());
            hash.update(cell.as_bytes());
        }
    }
    let mut hash = Sha256::new();
    hash.update([u8::from(table.has_headers)]);
    record(&mut hash, &table.headers);
    for row in &table.rows {
        record(&mut hash, row);
    }
    hash.finalize().into()
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "{} exceeds the {} MiB limit.",
        path.display(),
        limit / (1024 * 1024)
    );
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckSet {
    pub name: String,
    pub output: Output,
    pub reference: Reference,
    pub checks: Vec<NamedCheck>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Collection {
    version: u32,
    sets: Vec<CheckSet>,
}

pub(super) struct Store {
    pub path: PathBuf,
    pub sets: Vec<CheckSet>,
    stamp: Option<[u8; 32]>,
    // Undo/redo the last explicit check-file edit, independent of mapping undo.
    previous: Option<Vec<CheckSet>>,
    pub redo: bool,
}

impl Store {
    pub fn open(mapping: &Path) -> Result<Self> {
        let path = mapping.with_added_extension("checks.json");
        let bytes = read_sidecar(&path)?;
        let stamp = bytes.as_deref().map(|bytes| Sha256::digest(bytes).into());
        let sets = if let Some(bytes) = bytes {
            let saved: Collection = serde_json::from_slice(&bytes).with_context(|| {
                format!("Could not read saved check sets in {}", path.display())
            })?;
            ensure!(
                saved.version == VERSION,
                "Unsupported check-set version {}.",
                saved.version
            );
            validate(&saved.sets)?;
            saved.sets
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            sets,
            stamp,
            previous: None,
            redo: false,
        })
    }

    pub fn save(&mut self, index: Option<usize>, set: CheckSet) -> Result<usize> {
        let mut sets = self.sets.clone();
        let index = if let Some(index) = index {
            *sets
                .get_mut(index)
                .context("The edited check set is gone.")? = set;
            index
        } else {
            sets.push(set);
            sets.len() - 1
        };
        validate(&sets)?;
        if sets != self.sets {
            self.publish(sets)?;
            self.redo = false;
        }
        Ok(index)
    }

    pub fn can_undo_or_redo(&self) -> bool {
        self.previous.is_some()
    }

    pub fn undo_or_redo(&mut self) -> Result<()> {
        let previous = self
            .previous
            .as_ref()
            .context("No check edit to undo.")?
            .clone();
        self.publish(previous)?;
        self.redo = !self.redo;
        Ok(())
    }

    fn publish(&mut self, sets: Vec<CheckSet>) -> Result<()> {
        // Keep the lock file's identity stable across atomic sidecar replacement.
        // The OS releases the lock if the writer exits, including after a crash.
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_added_extension("lock"))
            .context("Opening check-set writer lock")?;
        lock.try_lock()
            .context("Another writer is saving these check sets")?;
        let current = read_sidecar(&self.path)?
            .as_deref()
            .map(|bytes| Sha256::digest(bytes).into());
        ensure!(
            self.stamp == current,
            "The check file changed on disk. Reopen Saved checks before saving."
        );
        let saved = Collection {
            version: VERSION,
            sets,
        };
        let bytes = serde_json::to_vec(&saved)?;
        ensure!(
            bytes.len() <= MAX_SIDECAR_BYTES,
            "Check file would exceed the 16 MiB limit."
        );
        crate::document_file::replace(&self.path, &bytes)?;
        self.stamp = Some(Sha256::digest(&bytes).into());
        self.previous = Some(std::mem::replace(&mut self.sets, saved.sets));
        Ok(())
    }
}

fn read_sidecar(path: &Path) -> Result<Option<Vec<u8>>> {
    match read_bounded(path, MAX_SIDECAR_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.context(format!("Reading {}", path.display()))),
    }
}

fn validate(sets: &[CheckSet]) -> Result<()> {
    let mut names = BTreeSet::new();
    for set in sets {
        ensure!(!set.name.trim().is_empty(), "Name the check set.");
        ensure!(
            names.insert(set.name.trim()),
            "A check set named {:?} already exists.",
            set.name.trim()
        );
        ensure!(!set.checks.is_empty(), "Add at least one check.");
        let mut checks = BTreeSet::new();
        for check in &set.checks {
            ensure!(!check.name.trim().is_empty(), "Give each check a name.");
            ensure!(
                checks.insert(check.name.trim()),
                "Duplicate check name {:?}.",
                check.name
            );
        }
    }
    Ok(())
}

pub(super) fn require_independent_file(
    mapping: &Path,
    output: &Output,
    reference: &Path,
) -> Result<()> {
    let reference = reference
        .canonicalize()
        .with_context(|| format!("Cannot open the chosen reference {}", reference.display()))?;
    let output_path = mapping
        .parent()
        .unwrap_or(Path::new("."))
        .join(&output.artifact);
    match same_file::is_same_file(&output_path, &reference) {
        Ok(true) => Err(anyhow!(
            "Choose an independent known-good file, separate from the selected output."
        )),
        Ok(false) => Ok(()),
        // In-memory preview artifacts need not exist on disk.
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).context("Could not verify that the reference is separate from the output")
        }
    }
}
