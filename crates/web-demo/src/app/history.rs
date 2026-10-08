//! Browser project snapshots, independent of text buffers and canvas geometry.

mod finite;

use std::collections::VecDeque;

use mapping::{NodeId, Project};
use std::io::Write;

pub(super) const MAX_TRANSITIONS: usize = 64;
pub(super) const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecordResult {
    Unchanged,
    Recorded,
}

pub(super) struct ProjectHistory {
    undo: VecDeque<String>,
    current: Option<String>,
    redo: Vec<String>,
    bytes: usize,
    coalescing: Option<NodeId>,
    transition_limit: usize,
    byte_limit: usize,
}

impl Default for ProjectHistory {
    fn default() -> Self {
        Self::with_limits(MAX_TRANSITIONS, MAX_SNAPSHOT_BYTES)
    }
}

impl ProjectHistory {
    pub(super) fn with_limits(transition_limit: usize, byte_limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            current: None,
            redo: Vec::new(),
            bytes: 0,
            coalescing: None,
            transition_limit,
            byte_limit,
        }
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.current = None;
        self.redo.clear();
        self.bytes = 0;
        self.coalescing = None;
    }

    pub(super) fn capture_and_record(
        &mut self,
        project: &Project,
        constant: Option<NodeId>,
    ) -> Result<RecordResult, SnapshotError> {
        match capture(project, self.byte_limit) {
            Ok(json) => Ok(self.record(json, constant)),
            Err(error) => {
                self.clear();
                Err(error)
            }
        }
    }

    fn record(&mut self, json: String, constant: Option<NodeId>) -> RecordResult {
        if self.current.as_deref() == Some(json.as_str()) {
            // An unchanged Apply still separates two canvas editing sessions.
            if constant.is_none() {
                self.coalescing = None;
            }
            return RecordResult::Unchanged;
        }

        while let Some(snapshot) = self.redo.pop() {
            self.bytes -= snapshot.len();
        }
        if let Some(previous) = self.current.take() {
            if constant.is_some() && self.coalescing == constant && !self.undo.is_empty() {
                self.bytes -= previous.len();
            } else {
                self.undo.push_back(previous);
            }
        }
        // Reserve room before retaining the admitted current. Every before/redo
        // snapshot shares this ledger; serialization outside it is temporary.
        while self.undo.len() + self.redo.len() > self.transition_limit
            || self.bytes > self.byte_limit - json.len()
        {
            let Some(oldest) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= oldest.len();
        }
        self.bytes += json.len();
        self.current = Some(json);
        self.coalescing = constant;
        RecordResult::Recorded
    }

    pub(super) fn finish_focus(&mut self, focused_constant: Option<NodeId>) {
        if self.coalescing != focused_constant {
            self.coalescing = None;
        }
    }

    pub(super) fn undo_json(&self) -> Option<&str> {
        self.undo.back().map(String::as_str)
    }

    pub(super) fn redo_json(&self) -> Option<&str> {
        self.redo.last().map(String::as_str)
    }

    // The app decodes the selected full snapshot before moving either stack.
    // A decode refusal leaves both the project and every transition intact.
    pub(super) fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        if let Some(current) = self.current.replace(previous) {
            self.redo.push(current);
        }
        self.coalescing = None;
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        if let Some(current) = self.current.replace(next) {
            self.undo.push_back(current);
        }
        self.coalescing = None;
        true
    }

    #[cfg(test)]
    pub(super) fn state(&self) -> HistoryState {
        HistoryState {
            undo: self.undo.iter().cloned().collect(),
            current: self.current.clone(),
            redo: self.redo.clone(),
            bytes: self.bytes,
            coalescing: self.coalescing,
        }
    }

    #[cfg(test)]
    pub(super) fn retained(&self) -> (usize, usize) {
        let actual = self
            .undo
            .iter()
            .chain(self.current.iter())
            .chain(self.redo.iter())
            .map(String::len)
            .sum::<usize>();
        assert_eq!(self.bytes, actual);
        (self.undo.len() + self.redo.len(), actual)
    }
}

#[derive(Debug)]
pub(super) enum SnapshotError {
    TooLarge { max: usize },
    Serialize(serde_json::Error),
    Decode(mapping::FileCodecError),
    Changed,
    Utf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { max } => write!(f, "snapshot exceeds {max} UTF-8 bytes"),
            Self::Serialize(error) => write!(f, "snapshot serialization failed: {error}"),
            Self::Decode(error) => write!(f, "snapshot decoding failed: {error}"),
            Self::Changed => f.write_str("snapshot would change the project"),
            Self::Utf8(error) => write!(f, "snapshot UTF-8 failed: {error}"),
        }
    }
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    limit_hit: bool,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            self.limit_hit = true;
            return Err(std::io::Error::other("project history snapshot byte limit"));
        }
        // Request only the admitted length, rather than exponential growth past
        // the logical cap. Allocator/model overhead is outside the UTF-8 ledger.
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            let requested = needed
                .max(self.bytes.capacity().saturating_mul(2))
                .max(256.min(self.limit))
                .min(self.limit);
            self.bytes
                .try_reserve_exact(requested - self.bytes.len())
                .map_err(std::io::Error::other)?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct MatchWriter<'a> {
    original: &'a [u8],
    offset: usize,
}
impl Write for MatchWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let remaining = &self.original[self.offset..];
        if !remaining.starts_with(bytes) {
            return Err(std::io::Error::other("project history roundtrip changed"));
        }
        self.offset += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Compact history is private. The public project editor/download codec keeps
// its existing pretty/V2 behavior. Both typed decode and the whole second wire
// pass must agree; a nonfinite or otherwise unfaithful intermediate is untracked.
fn capture(project: &Project, limit: usize) -> Result<String, SnapshotError> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
        limit_hit: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, &finite::Checked(project)) {
        return Err(if writer.limit_hit {
            SnapshotError::TooLarge { max: limit }
        } else {
            SnapshotError::Serialize(error)
        });
    }
    let decoded =
        mapping::project_file::decode_bytes(&writer.bytes).map_err(SnapshotError::Decode)?;
    let mut check = MatchWriter {
        original: &writer.bytes,
        offset: 0,
    };
    if serde_json::to_writer(&mut check, &finite::Checked(&decoded)).is_err()
        || check.offset != writer.bytes.len()
    {
        return Err(SnapshotError::Changed);
    }
    String::from_utf8(writer.bytes).map_err(SnapshotError::Utf8)
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct HistoryState {
    undo: Vec<String>,
    current: Option<String>,
    redo: Vec<String>,
    bytes: usize,
    coalescing: Option<NodeId>,
}
