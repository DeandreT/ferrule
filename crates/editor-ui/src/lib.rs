//! Platform-neutral state for ferrule editor hosts.
//!
//! This crate owns document history, save-point tracking, semantic and
//! layout revisions, typed workspace selection, and atomic editor commands.
//! Rendering, filesystem access, dialogs, and runtime execution remain
//! responsibilities of host crates.

mod command;
mod document;
mod edit_history;
mod graph_command;
mod graph_edit;
mod node_input;
mod project_command;
mod project_edit;
mod revision;
mod scope_command;
mod scope_setting;
mod selection;
mod snapshot_history;

pub use command::{EditorCommand, EditorTransaction};
pub use document::{CommandOutcome, DocumentOrigin, EditorDocument, HistoryLimit, HistoryOutcome};
pub use edit_history::{EditHistory, HistoryRecord};
pub use graph_command::{
    CanvasPosition, GraphAction, GraphCommand, NodeProperty, TargetBinding, WireSink,
};
pub use graph_edit::{
    BindingsChange, ChildrenChange, GraphDocument, GraphEditError, GraphPatch, NodeChange,
    ScopeLocation, ScopeStep, SettingChange,
};
pub use node_input::{is_hidden_input, set_scalar_input};
pub use project_edit::{PartChange, ProjectPart, ProjectParts};
pub use revision::{
    ComponentChange, DocumentRevision, LayoutRevision, RevisionChange, RevisionKeyed,
    RevisionTracker, SemanticRevision,
};
pub use scope_setting::{
    Grouping, Iteration, ScopeFields, ScopeSetting, ScopeSettingValue, Sorting,
};
pub use selection::{BoundaryId, GraphNodeSelection, SourceId, TargetId, WorkspaceSelection};
pub use snapshot_history::{HistoryStep, SnapshotHistory};
