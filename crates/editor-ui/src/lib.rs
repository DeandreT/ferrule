//! Platform-neutral state for ferrule editor hosts.
//!
//! This crate owns document history, save-point tracking, typed workspace
//! selection, and atomic editor commands. Rendering, filesystem access,
//! dialogs, and runtime execution remain responsibilities of host crates.

mod command;
mod document;
mod selection;
mod snapshot_history;

pub use command::{EditorCommand, EditorTransaction};
pub use document::{CommandOutcome, DocumentOrigin, EditorDocument, HistoryLimit, HistoryOutcome};
pub use selection::{BoundaryId, GraphNodeSelection, SourceId, TargetId, WorkspaceSelection};
pub use snapshot_history::{HistoryStep, SnapshotHistory};
