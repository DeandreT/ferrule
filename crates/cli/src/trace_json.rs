use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, bail};
use engine::{
    TraceEvent, TraceFilterPhase, TraceGrouping, TraceIteration, TraceOutputKind, TracePosition,
    TraceScope, TraceSink, TraceTarget, TraceTargetFieldBinding, TraceValue, TraceWindow,
};
use serde_json::{Value as JsonValue, json};

use crate::WrittenOutput;

const TRACE_SCHEMA_VERSION: u64 = 1;
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
}

impl JsonTraceFile {
    /// Preflights `destination` and creates a private sibling staging file.
    pub fn create(destination: &Path) -> anyhow::Result<Self> {
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

fn trace_line(sequence: u64, event: &TraceEvent) -> JsonValue {
    json!({
        "schema_version": TRACE_SCHEMA_VERSION,
        "sequence": sequence,
        "event": event_value(event),
    })
}

fn event_value(event: &TraceEvent) -> JsonValue {
    match event {
        TraceEvent::NodeValue {
            node,
            positions,
            value,
        } => json!({
            "kind": "node_value",
            "node": node,
            "positions": positions_value(positions),
            "value": scalar_value(value),
        }),
        TraceEvent::ScopeStarted {
            scope,
            iteration,
            positions,
        } => json!({
            "kind": "scope_started",
            "scope": scope_value(scope),
            "iteration": iteration_value(iteration),
            "positions": positions_value(positions),
        }),
        TraceEvent::IterationCandidate {
            scope,
            ordinal,
            positions,
        } => json!({
            "kind": "iteration_candidate",
            "scope": scope_value(scope),
            "ordinal": ordinal,
            "positions": positions_value(positions),
        }),
        TraceEvent::FilterDecision {
            scope,
            node,
            phase,
            positions,
            passed,
        } => json!({
            "kind": "filter_decision",
            "scope": scope_value(scope),
            "node": node,
            "phase": filter_phase(*phase),
            "positions": positions_value(positions),
            "passed": passed,
        }),
        TraceEvent::SortCandidate {
            scope,
            positions,
            keys,
        } => json!({
            "kind": "sort_candidate",
            "scope": scope_value(scope),
            "positions": positions_value(positions),
            "keys": keys.iter().map(|key| json!({
                "node": key.node,
                "descending": key.descending,
                "value": trace_value(&key.value),
            })).collect::<Vec<_>>(),
        }),
        TraceEvent::SortPosition {
            scope,
            positions,
            output_index,
        } => json!({
            "kind": "sort_position",
            "scope": scope_value(scope),
            "positions": positions_value(positions),
            "output_index": output_index,
        }),
        TraceEvent::GroupProduced {
            scope,
            grouping,
            group_index,
            member_count,
            key,
            retained,
            positions,
        } => json!({
            "kind": "group_produced",
            "scope": scope_value(scope),
            "grouping": grouping_value(*grouping),
            "group_index": group_index,
            "member_count": member_count,
            "key": key.as_ref().map(trace_value),
            "retained": retained,
            "positions": positions_value(positions),
        }),
        TraceEvent::WindowApplied {
            scope,
            window_index,
            window,
            before,
            after,
        } => json!({
            "kind": "window_applied",
            "scope": scope_value(scope),
            "window_index": window_index,
            "window": window_value(*window),
            "before": before,
            "after": after,
        }),
        TraceEvent::TargetFieldWritten {
            scope,
            field,
            binding,
            positions,
            kind,
            value,
        } => json!({
            "kind": "target_field_written",
            "scope": scope_value(scope),
            "field": field,
            "binding": target_field_binding(*binding),
            "positions": positions_value(positions),
            "output_kind": output_kind(*kind),
            "value": value.as_ref().map(trace_value),
        }),
        TraceEvent::TargetProduced {
            scope,
            positions,
            output_path,
            kind,
        } => json!({
            "kind": "target_produced",
            "scope": scope_value(scope),
            "positions": positions_value(positions),
            "output_path": output_path,
            "output_kind": output_kind(*kind),
        }),
        TraceEvent::ScopeFinished {
            scope,
            candidates,
            produced,
            kind,
        } => json!({
            "kind": "scope_finished",
            "scope": scope_value(scope),
            "candidates": candidates,
            "produced": produced,
            "output_kind": output_kind(*kind),
        }),
    }
}

fn scalar_value(value: &ir::Value) -> JsonValue {
    match value {
        ir::Value::Null => json!({"type": "absent", "value": null}),
        ir::Value::JsonNull(_) => json!({"type": "json_null", "value": null}),
        ir::Value::XmlNil(_) => json!({"type": "xml_nil", "value": null}),
        ir::Value::Bool(value) => json!({"type": "bool", "value": value}),
        ir::Value::Int(value) => json!({"type": "int", "value": value}),
        ir::Value::Float(value) if value.is_finite() => {
            json!({"type": "float", "value": value})
        }
        ir::Value::Float(value) => json!({
            "type": "float",
            "value": if value.is_nan() {
                "NaN"
            } else if value.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            },
        }),
        ir::Value::String(value) => json!({"type": "string", "value": value}),
    }
}

fn positions_value(positions: &[TracePosition]) -> Vec<JsonValue> {
    positions
        .iter()
        .map(|position| {
            json!({
                "collection": position.collection,
                "index": position.index,
                "grouped": position.grouped,
                "join": position.join.map(mapping::JoinId::get),
                "join_position": position.join_position.map(|(join, index)| json!({
                    "join": join.get(),
                    "index": index,
                })),
                "document_path": position.document_path,
            })
        })
        .collect()
}

fn scope_value(scope: &TraceScope) -> JsonValue {
    json!({
        "target": match &scope.target {
            TraceTarget::Primary => json!({"kind": "primary"}),
            TraceTarget::Named(name) => json!({"kind": "named", "name": name}),
        },
        "target_path": scope.target_path,
        "structural_path": scope.structural_path,
    })
}

fn iteration_value(iteration: &TraceIteration) -> JsonValue {
    match iteration {
        TraceIteration::Once => json!({"kind": "once"}),
        TraceIteration::Source { path } => json!({"kind": "source", "path": path}),
        TraceIteration::DynamicDocuments { source } => {
            json!({"kind": "dynamic_documents", "source": source})
        }
        TraceIteration::Generated { kind } => {
            json!({"kind": "generated", "sequence": kind})
        }
        TraceIteration::Join { join } => json!({"kind": "join", "join": join.get()}),
        TraceIteration::Concatenate { segments } => {
            json!({"kind": "concatenate", "segments": segments})
        }
    }
}

fn filter_phase(phase: TraceFilterPhase) -> &'static str {
    match phase {
        TraceFilterPhase::BeforeSort => "before_sort",
        TraceFilterPhase::AfterSort => "after_sort",
        TraceFilterPhase::Selection => "selection",
        TraceFilterPhase::GroupStarting => "group_starting",
        TraceFilterPhase::GroupEnding => "group_ending",
        TraceFilterPhase::PostGroupMember => "post_group_member",
    }
}

fn grouping_value(grouping: TraceGrouping) -> JsonValue {
    match grouping {
        TraceGrouping::By { node } => json!({"kind": "by", "node": node}),
        TraceGrouping::AdjacentBy { node } => {
            json!({"kind": "adjacent_by", "node": node})
        }
        TraceGrouping::StartingWith { node } => {
            json!({"kind": "starting_with", "node": node})
        }
        TraceGrouping::EndingWith { node } => {
            json!({"kind": "ending_with", "node": node})
        }
        TraceGrouping::IntoBlocks { node, size } => {
            json!({"kind": "into_blocks", "node": node, "size": size})
        }
    }
}

fn trace_value(value: &TraceValue) -> JsonValue {
    json!({
        "type": value.value_type,
        "preview": value.preview,
        "truncated": value.truncated,
    })
}

fn window_value(window: TraceWindow) -> JsonValue {
    match window {
        TraceWindow::SkipFirst(count) => json!({"kind": "skip_first", "count": count}),
        TraceWindow::First(count) => json!({"kind": "first", "count": count}),
        TraceWindow::From(first) => json!({"kind": "from", "first": first}),
        TraceWindow::FromTo { first, last } => {
            json!({"kind": "from_to", "first": first, "last": last})
        }
        TraceWindow::Last(count) => json!({"kind": "last", "count": count}),
    }
}

fn output_kind(kind: TraceOutputKind) -> &'static str {
    match kind {
        TraceOutputKind::Scalar => "scalar",
        TraceOutputKind::Group => "group",
        TraceOutputKind::Repeated => "repeated",
        TraceOutputKind::MappedSequence => "mapped_sequence",
        TraceOutputKind::DocumentSet => "document_set",
    }
}

fn target_field_binding(binding: TraceTargetFieldBinding) -> JsonValue {
    match binding {
        TraceTargetFieldBinding::StaticBinding { value } => {
            json!({"kind": "static_binding", "value_node": value})
        }
        TraceTargetFieldBinding::DynamicBinding { key, value } => {
            json!({"kind": "dynamic_binding", "key_node": key, "value_node": value})
        }
        TraceTargetFieldBinding::StaticChild => json!({"kind": "static_child"}),
        TraceTargetFieldBinding::DynamicChild { key } => {
            json!({"kind": "dynamic_child", "key_node": key})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{TraceSortKey, TraceTarget};

    fn scope() -> TraceScope {
        TraceScope {
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
                value: ir::Value::Float(f64::INFINITY),
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
        let field = trace_line(8, &events[8]);
        assert_eq!(field["event"]["field"], "status");
        assert_eq!(field["event"]["binding"]["kind"], "dynamic_binding");
        assert_eq!(field["event"]["binding"]["key_node"], 5);
        assert_eq!(field["event"]["binding"]["value_node"], 6);
        assert_eq!(field["event"]["output_kind"], "scalar");
        assert_eq!(field["event"]["value"]["preview"], "accepted");
    }
}
