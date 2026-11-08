use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, bail};
use engine::{TraceEvent, TraceSink};

use crate::WrittenOutput;

const TRACE_SCHEMA_VERSION: u64 = 5;

// These bounds apply only to loader-request records, not existing graph events.
#[derive(Clone, Copy)]
struct RequestLimits {
    source_bytes: usize,
    path_bytes: usize,
    line_bytes: usize,
    requests: u64,
    bytes: u64,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            source_bytes: 4096,
            path_bytes: 4096,
            line_bytes: 65536,
            requests: 4096,
            bytes: 268435456,
        }
    }
}

struct RequestLineWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for RequestLineWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
            std::io::Error::other("dynamic source request line exceeds its byte limit")
        })?;
        if length > self.limit {
            return Err(std::io::Error::other(
                "dynamic source request line exceeds its byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(serde::Serialize)]
struct RequestEvent<'a> {
    kind: &'static str,
    path: &'a str,
    source: &'a str,
}

#[derive(serde::Serialize)]
struct RequestEnvelope<'a> {
    event: RequestEvent<'a>,
    schema_version: u64,
    sequence: u64,
}

fn request_line(
    sequence: u64,
    source: &str,
    path: &str,
    limits: RequestLimits,
) -> Result<Vec<u8>, String> {
    // Check borrowed UTF-8 byte lengths before allocating the record buffer.
    if source.len() > limits.source_bytes {
        return Err("dynamic source request source exceeds its byte limit".into());
    }
    if path.len() > limits.path_bytes {
        return Err("dynamic source request path exceeds its byte limit".into());
    }
    let mut writer = RequestLineWriter {
        bytes: Vec::new(),
        limit: limits.line_bytes,
    };
    // Fixed field order is independent of serde_json map feature unification.
    // Strings are serialized directly from their borrowed identities.
    let result = (|| -> std::io::Result<()> {
        let envelope = RequestEnvelope {
            event: RequestEvent {
                kind: "dynamic_source_load_requested",
                path,
                source,
            },
            schema_version: TRACE_SCHEMA_VERSION,
            sequence,
        };
        serde_json::to_writer(&mut writer, &envelope).map_err(std::io::Error::other)?;
        writer.write_all(b"\n")
    })();
    result.map_err(|error| error.to_string())?;
    Ok(writer.bytes)
}
const STAGE_ATTEMPTS: usize = 64;
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A streaming JSON Lines trace that replaces its destination only after a
/// successful mapping execution.
///
/// Create this before running the mapping, pass it as the run's `TraceSink`,
/// then call [`Self::finish`] after the run succeeds. Dropping it at any other
/// point removes the private staging file and leaves the destination untouched.
pub struct JsonTraceFile {
    destination: PathBuf,
    staged: Option<PathBuf>,
    state: RefCell<TraceWriterState>,
}

struct TraceWriterState {
    writer: Option<BufWriter<File>>,
    sequence: u64,
    error: Option<String>,
    request_limits: RequestLimits,
    requests: u64,
    request_bytes: u64,
}

impl JsonTraceFile {
    /// Preflights `destination` and creates a private sibling staging file.
    pub fn create(destination: &Path) -> anyhow::Result<Self> {
        Self::create_with_request_limits(destination, RequestLimits::default())
    }

    fn create_with_request_limits(
        destination: &Path,
        request_limits: RequestLimits,
    ) -> anyhow::Result<Self> {
        if destination == Path::new("-") {
            bail!(
                "`--trace-json -` is not supported because `run` reserves stdout for its result report"
            );
        }
        require_valid_destination(destination)?;
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating trace directory {}", parent.display()))?;

        let (staged, file) = create_stage_file(parent)?;
        Ok(Self {
            destination: destination.to_path_buf(),
            staged: Some(staged),
            state: RefCell::new(TraceWriterState {
                writer: Some(BufWriter::new(file)),
                sequence: 0,
                error: None,
                request_limits,
                requests: 0,
                request_bytes: 0,
            }),
        })
    }

    /// Returns the final path reserved for this trace artifact.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Flushes and atomically publishes the completed trace.
    ///
    /// `artifacts` must be the successful run's published artifact list. This
    /// prevents a trace destination from replacing one of the mapping outputs.
    pub fn finish(mut self, artifacts: &[WrittenOutput]) -> anyhow::Result<()> {
        reject_artifact_collision(&self.destination, artifacts)?;

        let mut state = self.state.borrow_mut();
        if let Some(error) = state.error.take() {
            bail!("writing JSON trace: {error}");
        }
        let mut writer = state
            .writer
            .take()
            .context("JSON trace writer was already finalized")?;
        writer.flush().context("flushing JSON trace staging file")?;
        writer
            .get_ref()
            .sync_all()
            .context("synchronizing JSON trace staging file")?;
        drop(writer);
        drop(state);

        require_valid_destination(&self.destination)?;
        let staged = self
            .staged
            .as_deref()
            .context("JSON trace staging file was already published")?;
        std::fs::rename(staged, &self.destination).with_context(|| {
            format!(
                "atomically publishing JSON trace {}",
                self.destination.display()
            )
        })?;
        self.staged = None;
        Ok(())
    }
}

impl TraceSink for JsonTraceFile {
    fn dynamic_source_load_requested(&self, source_name: &str, requested_path: &str) {
        let mut state = self.state.borrow_mut();
        if state.error.is_some() {
            return;
        }
        let result = (|| -> Result<(u64, u64, u64), String> {
            let requests = state
                .requests
                .checked_add(1)
                .filter(|count| *count <= state.request_limits.requests)
                .ok_or_else(|| "dynamic source request count exceeds its limit".to_string())?;
            let sequence = state
                .sequence
                .checked_add(1)
                .ok_or_else(|| "trace event sequence overflowed".to_string())?;
            let line = request_line(
                state.sequence,
                source_name,
                requested_path,
                state.request_limits,
            )?;
            let bytes = state
                .request_bytes
                .checked_add(line.len() as u64)
                .filter(|bytes| *bytes <= state.request_limits.bytes)
                .ok_or_else(|| "dynamic source request bytes exceed their limit".to_string())?;
            state
                .writer
                .as_mut()
                .ok_or_else(|| "JSON trace writer was already finalized".to_string())?
                .write_all(&line)
                .map_err(|error| error.to_string())?;
            Ok((sequence, requests, bytes))
        })();
        match result {
            Ok((sequence, requests, bytes)) => {
                state.sequence = sequence;
                state.requests = requests;
                state.request_bytes = bytes;
            }
            Err(error) => state.error = Some(error),
        }
    }

    fn record(&self, event: TraceEvent) {
        let mut state = self.state.borrow_mut();
        if state.error.is_some() {
            return;
        }
        let sequence = state.sequence;
        let line = trace_line(sequence, &event);
        let result = state
            .writer
            .as_mut()
            .ok_or_else(|| "JSON trace writer was already finalized".to_string())
            .and_then(|writer| {
                serde_json::to_writer(&mut *writer, &line).map_err(|error| error.to_string())?;
                writer.write_all(b"\n").map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => match sequence.checked_add(1) {
                Some(next) => state.sequence = next,
                None => state.error = Some("trace event sequence overflowed".into()),
            },
            Err(error) => state.error = Some(error),
        }
    }
}

impl Drop for JsonTraceFile {
    fn drop(&mut self) {
        self.state.get_mut().writer.take();
        if let Some(staged) = self.staged.take() {
            let _ = std::fs::remove_file(staged);
        }
    }
}

fn require_valid_destination(destination: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "JSON trace destination {} cannot be a symlink",
                destination.display()
            )
        }
        Ok(metadata) if metadata.is_dir() => {
            bail!(
                "JSON trace destination {} is a directory",
                destination.display()
            )
        }
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => bail!(
            "JSON trace destination {} is not a regular file",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("checking JSON trace destination {}", destination.display())),
    }
}

fn create_stage_file(parent: &Path) -> anyhow::Result<(PathBuf, File)> {
    for _ in 0..STAGE_ATTEMPTS {
        let id = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".ferrule-trace-stage-{}-{id}.jsonl",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("creating JSON trace staging file {}", path.display())
                });
            }
        }
    }
    bail!("could not allocate a unique JSON trace staging file")
}

fn reject_artifact_collision(
    destination: &Path,
    artifacts: &[WrittenOutput],
) -> anyhow::Result<()> {
    let destination = resolved_file_identity(destination)?;
    for artifact in artifacts {
        let artifact_path = std::fs::canonicalize(&artifact.path).with_context(|| {
            format!(
                "resolving published mapping artifact {}",
                artifact.path.display()
            )
        })?;
        if destination == artifact_path {
            bail!(
                "JSON trace destination {} is also a mapped output",
                destination.display()
            );
        }
    }
    Ok(())
}

fn resolved_file_identity(path: &Path) -> anyhow::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let file_name = path
                .file_name()
                .context("JSON trace destination must name a file")?;
            Ok(std::fs::canonicalize(parent)
                .with_context(|| format!("resolving JSON trace directory {}", parent.display()))?
                .join(file_name))
        }
        Err(error) => Err(error)
            .with_context(|| format!("resolving JSON trace destination {}", path.display())),
    }
}

/// One JSON Lines record. Field order is fixed by the struct declaration.
#[derive(serde::Serialize)]
struct TraceLine<'a> {
    schema_version: u64,
    sequence: u64,
    event: &'a TraceEvent,
}

fn trace_line(sequence: u64, event: &TraceEvent) -> TraceLine<'_> {
    TraceLine {
        schema_version: TRACE_SCHEMA_VERSION,
        sequence,
        event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{
        TraceFilterPhase, TraceGrouping, TraceIteration, TraceOutputKind, TracePosition,
        TraceScope, TraceSortKey, TraceSourceField, TraceSourceRow, TraceTarget,
        TraceTargetFieldBinding, TraceValue, TraceWindow,
    };
    use serde_json::{Value as JsonValue, json};

    fn to_json(value: impl serde::Serialize) -> JsonValue {
        serde_json::to_value(value).expect("trace values serialize")
    }

    fn trace_line(sequence: u64, event: &TraceEvent) -> JsonValue {
        to_json(super::trace_line(sequence, event))
    }

    fn event_value(event: &TraceEvent) -> JsonValue {
        to_json(event)
    }

    fn iteration_value(iteration: &TraceIteration) -> JsonValue {
        to_json(iteration)
    }

    fn scope() -> TraceScope {
        TraceScope {
            source_scope: mapping::SourceScopeLocation {
                target: mapping::SourceScopeTarget::Named {
                    index: 0,
                    name: "audit".into(),
                },
                path: vec![mapping::SourceScopeStep::Child(2)],
            },
            target: TraceTarget::Named("audit".into()),
            target_path: vec!["Rows".into()],
            structural_path: vec![2],
        }
    }

    #[test]
    fn every_trace_event_has_a_stable_kind_and_envelope() {
        let position = TracePosition {
            collection: vec!["Rows".into()],
            index: 3,
            grouped: false,
            join: Some(mapping::JoinId::new(7)),
            join_position: Some((mapping::JoinId::new(7), 2)),
            document_path: Some("input/3.json".into()),
        };
        let events = vec![
            TraceEvent::NodeValue {
                node: 1,
                positions: vec![position.clone()],
                value: TraceValue {
                    value_type: "float",
                    preview: "inf".into(),
                    truncated: false,
                },
            },
            TraceEvent::NodeInputValue {
                consumer: 8,
                input: 1,
                input_index: 2,
                positions: vec![position.clone()],
                value: TraceValue {
                    value_type: "float",
                    preview: "inf".into(),
                    truncated: false,
                },
            },
            TraceEvent::ScopeStarted {
                scope: scope(),
                iteration: TraceIteration::Generated { kind: "tokenize" },
                positions: vec![],
            },
            TraceEvent::IterationCandidate {
                scope: scope(),
                ordinal: 1,
                positions: vec![position.clone()],
                source_row: None,
            },
            TraceEvent::FilterDecision {
                scope: scope(),
                node: 2,
                phase: TraceFilterPhase::AfterSort,
                positions: vec![position.clone()],
                passed: true,
            },
            TraceEvent::SortCandidate {
                scope: scope(),
                positions: vec![position.clone()],
                keys: vec![TraceSortKey {
                    node: 3,
                    descending: true,
                    value: TraceValue {
                        value_type: "int",
                        preview: "4".into(),
                        truncated: false,
                    },
                }],
            },
            TraceEvent::SortPosition {
                scope: scope(),
                positions: vec![position.clone()],
                output_index: 1,
            },
            TraceEvent::GroupProduced {
                scope: scope(),
                grouping: TraceGrouping::IntoBlocks { node: 4, size: 2 },
                group_index: 1,
                member_count: 2,
                key: None,
                retained: true,
                positions: vec![position.clone()],
            },
            TraceEvent::WindowApplied {
                scope: scope(),
                window_index: 0,
                window: TraceWindow::First(1),
                before: 2,
                after: 1,
            },
            TraceEvent::TargetFieldWritten {
                scope: scope(),
                field: "status".into(),
                binding: TraceTargetFieldBinding::DynamicBinding { key: 5, value: 6 },
                positions: vec![position.clone()],
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue {
                    value_type: "string",
                    preview: "accepted".into(),
                    truncated: false,
                }),
            },
            TraceEvent::TargetProduced {
                scope: scope(),
                positions: vec![position],
                output_path: Some("audit.json".into()),
                kind: TraceOutputKind::Group,
            },
            TraceEvent::ScopeFinished {
                scope: scope(),
                candidates: 4,
                produced: 1,
                kind: TraceOutputKind::Repeated,
            },
        ];
        let kinds = events
            .iter()
            .enumerate()
            .map(|(sequence, event)| trace_line(sequence as u64, event))
            .map(|line| {
                assert_eq!(line["schema_version"], TRACE_SCHEMA_VERSION);
                line["event"]["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                "node_value",
                "node_input_value",
                "scope_started",
                "iteration_candidate",
                "filter_decision",
                "sort_candidate",
                "sort_position",
                "group_produced",
                "window_applied",
                "target_field_written",
                "target_produced",
                "scope_finished",
            ]
        );
        let input = trace_line(1, &events[1]);
        assert_eq!(input["event"]["consumer"], 8);
        assert_eq!(input["event"]["input"], 1);
        assert_eq!(input["event"]["input_index"], 2);
        assert_eq!(input["event"]["positions"][0]["join"], 7);
        let field = trace_line(9, &events[9]);
        assert_eq!(field["event"]["field"], "status");
        assert_eq!(field["event"]["binding"]["kind"], "dynamic_binding");
        assert_eq!(field["event"]["binding"]["key_node"], 5);
        assert_eq!(field["event"]["binding"]["value_node"], 6);
        assert_eq!(field["event"]["output_kind"], "scalar");
        assert_eq!(field["event"]["value"]["preview"], "accepted");
    }

    #[test]
    fn source_row_is_an_optional_addition_to_v3_candidates() {
        let source_row = TraceSourceRow {
            kind: TraceOutputKind::Group,
            value: None,
            fields: vec![TraceSourceField {
                name: "Value".into(),
                name_truncated: false,
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue {
                    value_type: "string",
                    preview: "a".into(),
                    truncated: false,
                }),
            }],
            omitted_fields: 2,
            structure: None,
        };
        let event = TraceEvent::IterationCandidate {
            scope: scope(),
            ordinal: 3,
            positions: Vec::new(),
            source_row: Some(source_row),
        };

        let line = trace_line(9, &event);

        assert_eq!(line["schema_version"], 5);
        assert_eq!(line["sequence"], 9);
        assert_eq!(line["event"]["kind"], "iteration_candidate");
        assert_eq!(line["event"]["source_row"]["kind"], "group");
        assert_eq!(line["event"]["source_row"]["fields"][0]["name"], "Value");
        assert_eq!(
            line["event"]["source_row"]["fields"][0]["value"]["preview"],
            "a"
        );
        assert_eq!(line["event"]["source_row"]["omitted_fields"], 2);

        let absent = TraceEvent::IterationCandidate {
            scope: scope(),
            ordinal: 1,
            positions: Vec::new(),
            source_row: None,
        };
        assert!(trace_line(10, &absent)["event"].get("source_row").is_none());
    }

    #[test]
    fn function_body_events_use_v4_and_qualify_local_node_ids() {
        let function = mapping::FunctionId::new(12);
        let value = TraceValue {
            value_type: "string",
            preview: "done".into(),
            truncated: false,
        };
        let output = trace_line(
            2,
            &TraceEvent::FunctionNodeValue {
                function,
                node: 3,
                positions: Vec::new(),
                value: value.clone(),
            },
        );
        let input = trace_line(
            3,
            &TraceEvent::FunctionNodeInputValue {
                function,
                consumer: 4,
                input: 3,
                input_index: 1,
                positions: Vec::new(),
                value,
            },
        );

        assert_eq!(output["schema_version"], 5);
        assert_eq!(output["event"]["kind"], "function_node_value");
        assert_eq!(output["event"]["function"], 12);
        assert_eq!(output["event"]["node"], 3);
        assert_eq!(input["event"]["kind"], "function_node_input_value");
        assert_eq!(input["event"]["function"], 12);
        assert_eq!(input["event"]["consumer"], 4);
        assert_eq!(input["event"]["input"], 3);
        assert_eq!(input["event"]["input_index"], 1);
    }

    #[test]
    fn owned_source_iteration_json_distinguishes_primary_and_named_root() {
        for (owner, expected_owner) in [
            (mapping::SourceOwner::Primary, json!({"kind": "primary"})),
            (
                mapping::SourceOwner::Named {
                    name: "primary".into(),
                },
                json!({"kind": "named", "name": "primary"}),
            ),
        ] {
            let iteration = TraceIteration::OwnedSource {
                source: mapping::OwnedSourceRef {
                    owner,
                    path: Vec::new(),
                },
            };
            assert_eq!(
                iteration_value(&iteration),
                json!({"kind": "owned_source", "owner": expected_owner, "path": []})
            );
        }
    }

    #[test]
    fn owned_source_iteration_json_preserves_exact_name_and_path_segments() {
        for (name, path) in [
            (" RéF/West ", vec!["", "Rows/Lines", "é", " \t "]),
            ("\"Desk\"\\North\n", vec!["\"leaf\"", "a\\b", "\n"]),
        ] {
            let iteration = TraceIteration::OwnedSource {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Named { name: name.into() },
                    path: path.iter().map(|segment| (*segment).into()).collect(),
                },
            };
            let value = iteration_value(&iteration);
            assert_eq!(
                value,
                json!({
                    "kind": "owned_source",
                    "owner": {"kind": "named", "name": name},
                    "path": path,
                })
            );
            assert_eq!(
                serde_json::from_str::<JsonValue>(&value.to_string()).expect("JSON round trip"),
                value
            );
        }
    }

    #[test]
    fn owned_source_scope_lifecycle_json_keeps_context_and_finish_counts() {
        let started = TraceEvent::ScopeStarted {
            scope: scope(),
            iteration: TraceIteration::OwnedSource {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Named {
                        name: "primary".into(),
                    },
                    path: vec!["Rows/Lines".into()],
                },
            },
            positions: vec![TracePosition {
                collection: vec!["Outer".into()],
                index: 7,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            }],
        };
        assert_eq!(
            trace_line(7, &started),
            json!({
                "schema_version": 5,
                "sequence": 7,
                "event": {
                    "kind": "scope_started",
                    "scope": {
                        "target": {"kind": "named", "name": "audit"},
                        "target_path": ["Rows"],
                        "structural_path": [2],
                    },
                    "iteration": {
                        "kind": "owned_source",
                        "owner": {"kind": "named", "name": "primary"},
                        "path": ["Rows/Lines"],
                    },
                    "positions": [{
                        "collection": ["Outer"],
                        "index": 7,
                        "grouped": false,
                        "join": null,
                        "join_position": null,
                        "document_path": null,
                    }],
                },
            })
        );
        assert_eq!(
            event_value(&TraceEvent::ScopeFinished {
                scope: scope(),
                candidates: 2,
                produced: 1,
                kind: TraceOutputKind::Repeated,
            }),
            json!({
                "kind": "scope_finished",
                "scope": {
                    "target": {"kind": "named", "name": "audit"},
                    "target_path": ["Rows"],
                    "structural_path": [2],
                },
                "candidates": 2,
                "produced": 1,
                "output_kind": "repeated",
            })
        );
    }

    #[test]
    fn legacy_iteration_json_is_unchanged_with_owned_source_metadata() {
        for (iteration, expected) in [
            (TraceIteration::Once, json!({"kind": "once"})),
            (
                TraceIteration::Source { path: Vec::new() },
                json!({"kind": "source", "path": []}),
            ),
            (
                TraceIteration::Source {
                    path: vec!["Rows".into(), "Lines".into()],
                },
                json!({"kind": "source", "path": ["Rows", "Lines"]}),
            ),
            (
                TraceIteration::DynamicDocuments { source: Vec::new() },
                json!({"kind": "dynamic_documents", "source": []}),
            ),
            (
                TraceIteration::DynamicDocuments {
                    source: vec!["Rows".into(), "Lines".into()],
                },
                json!({"kind": "dynamic_documents", "source": ["Rows", "Lines"]}),
            ),
            (
                TraceIteration::Generated { kind: "tokenize" },
                json!({"kind": "generated", "sequence": "tokenize"}),
            ),
            (
                TraceIteration::Join {
                    join: mapping::JoinId::new(7),
                },
                json!({"kind": "join", "join": 7}),
            ),
            (
                TraceIteration::Concatenate { segments: 2 },
                json!({"kind": "concatenate", "segments": 2}),
            ),
        ] {
            assert_eq!(iteration_value(&iteration), expected);
        }
    }
}

#[cfg(test)]
#[path = "trace_json_request_tests.rs"]
mod request_tests;

#[cfg(test)]
#[path = "trace_json_serialize_tests.rs"]
mod serialize_tests;
