//! The ferrule-gui app: schema panes, scope editor, and mapping canvas.
//!
//! The canvas carries the visual-mapper interaction: the Source and Target
//! schemas are endpoint nodes whose pins are their scalar leaves, so a
//! mapping is wired leaf-to-function-to-leaf. Scope iteration (which
//! repeating path each scope loops over) is still edited in the side
//! panel -- connecting wires never changes iteration, only values.

use std::path::{Path, PathBuf};

#[cfg(test)]
use editor_ui::HistoryRecord;
use editor_ui::{DocumentOrigin, EditHistory, RevisionKeyed, RevisionTracker, SemanticRevision};
use editor_ui::{GraphDocument, ScopeFields, TargetId};
use egui_snarl::Snarl;
#[cfg(test)]
use egui_snarl::{InPinId, OutPinId};
#[cfg(test)]
use mapping::Node;
use mapping::{FunctionId, Graph, NodeId, Project, Scope, UserFunction};
use serde::{Deserialize, Serialize};

use crate::appearance::EditorAppearance;
use crate::appearance_editor::AppearanceTab;
use crate::auto_connect::AutoConnectPlan;
use crate::canvas::{CanvasNode, project_source_blocks, source_conflicts, target_blocks};
use crate::canvas_layout::{ArrangeMode, arrange_snarl_with_mode};
use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::document::DocumentLocation;
use crate::extra_sources::{ExtraSourceDraft, remove_extra_source};
use crate::extra_targets::ExtraTargetDraft;
use crate::graph_viewer::GraphViewer;
use crate::layout_store::{fingerprint_of_key, project_fingerprint, read_layout};
use crate::new_mapping::{NewMappingSetup, SchemaSide, blank_project};
use crate::path_picker::SourcePathCatalog;
use crate::schema_tree::{SchemaExplorerState, schema_field_count, show_schema_tree};
use crate::scope_editor::{
    ScopePath, available_static_child_scopes, binding_target_fields, create_static_child_scope,
    expand_static_target_subtree, remove_child_scope, scope_at_mut, scope_target_chain,
    show_scope_editor, show_scope_tree,
};
use crate::theme::{Palette, ThemeState};
use crate::workspace_layout::{LayoutClass, SideDock, WorkspacePane, WorkspaceVisibility};

#[path = "auto_connect.rs"]
mod auto_connect_ui;
#[path = "canvas.rs"]
mod canvas_build;
pub(crate) use canvas_build::{
    SourcePinPolicy, semantic_canvas_wires, sync_endpoint_wires_with_owned_items,
    sync_endpoint_wires_with_policy, update_endpoint_connections_with_policy,
};
#[path = "csv_dialect.rs"]
mod csv_dialect_ui;
#[path = "diagnostic_navigation.rs"]
mod diagnostic_navigation;
mod document_save;
#[path = "extra_sources.rs"]
mod extra_source_ui;
#[path = "extra_targets.rs"]
mod extra_target_ui;
mod failure_rules;
mod file_run_teardown;
mod graph_commands;
mod history;
pub(crate) use file_run_teardown::{SavedFileRunTeardownPort, SavedFileRunTeardownService};
mod filter_map_authoring;
#[path = "../workspace/functions.rs"]
mod function_workspace;
#[path = "host_parameters.rs"]
pub(crate) mod host_parameters;
mod join_authoring;
mod library_generation;
#[path = "mfd_export.rs"]
mod mfd_export_ui;
mod mfd_import;
use mfd_import::MfdImportTarget;
mod mfd_pipeline;
#[path = "new_mapping.rs"]
mod new_mapping_ui;
mod owned_worker;
mod pdf_extraction;
mod preview_service;
pub(crate) use preview_service::{PreviewService, PreviewServicePort};
#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
#[path = "pdf_preview_tests.rs"]
mod pdf_preview_tests;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
#[path = "pdf_run_tests.rs"]
mod pdf_run_tests;
#[path = "pipeline_editor.rs"]
mod pipeline_editor_ui;
mod pipeline_stage_canvas;
#[path = "pipeline.rs"]
mod pipeline_ui;
#[path = "preview.rs"]
mod preview_ui;
mod primary_document;
mod primary_xml_input;
mod rest_run;
mod route_preflight;
#[path = "run.rs"]
mod run_ui;
mod scope_retirement;
#[path = "scopes.rs"]
mod scope_ui;
pub(crate) mod sequence_consumer_duplication;
#[path = "workspace.rs"]
mod workspace_ui;

pub(crate) use canvas_build::endpoint_block_size;
#[cfg(test)]
pub(crate) use canvas_build::sync_endpoint_wires;
use canvas_build::{build_function_snarl, build_snarl, build_snarl_with_layout};

const HISTORY_COALESCE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);
pub(super) const LAYOUT_VERSION: u32 = 3;

fn history_shortcuts() -> [egui::KeyboardShortcut; 3] {
    [
        egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z),
        egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        ),
        egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y),
    ]
}

struct CanvasDocumentState {
    tools_id: egui::Id,
    snarl: Snarl<CanvasNode>,
    /// Deterministic geometry for the current semantic revision. Rewiring can
    /// change arrangement even when every node keeps its identity and position.
    default_layout_cache: std::cell::RefCell<Option<(String, Vec<CanvasNodeLayout>)>>,
    node_sizes: std::collections::BTreeMap<CanvasNode, egui::Vec2>,
    endpoint_scroll: crate::canvas_endpoints::EndpointScrollState,
    search: crate::canvas_search::CanvasSearchState,
    view_generation: u64,
    viewport_width: f32,
    pending_focus: Option<egui::Pos2>,
    pending_compact: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum MappingDocument {
    Main,
    Target(usize),
    Function(FunctionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AutoConnectTarget {
    Primary,
    Named(usize),
}

struct PendingAutoConnect {
    target: AutoConnectTarget,
    scope_path: ScopePath,
    scope_label: String,
    plan: AutoConnectPlan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SplitOrientation {
    #[default]
    Horizontal,
    Vertical,
}

struct MappingWorkspace {
    tabs: Vec<MappingDocument>,
    active: MappingDocument,
    focused: MappingDocument,
    split: Option<MappingDocument>,
    split_orientation: SplitOrientation,
    floating: std::collections::BTreeSet<FunctionId>,
    target_canvases: std::collections::BTreeMap<usize, CanvasDocumentState>,
    function_canvases: std::collections::BTreeMap<FunctionId, CanvasDocumentState>,
}

impl Default for MappingWorkspace {
    fn default() -> Self {
        Self {
            tabs: vec![MappingDocument::Main],
            active: MappingDocument::Main,
            focused: MappingDocument::Main,
            split: None,
            split_orientation: SplitOrientation::Horizontal,
            floating: std::collections::BTreeSet::new(),
            target_canvases: std::collections::BTreeMap::new(),
            function_canvases: std::collections::BTreeMap::new(),
        }
    }
}

impl MappingWorkspace {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn reconcile(
        &mut self,
        functions: &std::collections::BTreeMap<FunctionId, UserFunction>,
        target_count: usize,
    ) {
        let exists = |document: &MappingDocument| match document {
            MappingDocument::Main => true,
            MappingDocument::Target(index) => *index < target_count,
            MappingDocument::Function(id) => functions.contains_key(id),
        };
        self.tabs.retain(exists);
        if !self.tabs.contains(&MappingDocument::Main) {
            self.tabs.insert(0, MappingDocument::Main);
        }
        if !exists(&self.active) {
            self.active = MappingDocument::Main;
        }
        if !exists(&self.focused) {
            self.focused = self.active;
        }
        if self.split.is_some_and(|document| !exists(&document)) {
            self.split = None;
        }
        self.floating.retain(|id| functions.contains_key(id));
        self.target_canvases
            .retain(|index, _| *index < target_count);
        self.function_canvases
            .retain(|id, _| functions.contains_key(id));
    }

    fn remove_target(&mut self, removed: usize) {
        let remap = |document: MappingDocument| match document {
            MappingDocument::Target(index) if index == removed => None,
            MappingDocument::Target(index) if index > removed => {
                Some(MappingDocument::Target(index - 1))
            }
            document => Some(document),
        };
        self.tabs = self.tabs.iter().copied().filter_map(remap).collect();
        self.active = remap(self.active).unwrap_or(MappingDocument::Main);
        self.focused = remap(self.focused).unwrap_or(self.active);
        self.split = self.split.and_then(remap);
        self.target_canvases = std::mem::take(&mut self.target_canvases)
            .into_iter()
            .filter_map(|(index, canvas)| match index.cmp(&removed) {
                std::cmp::Ordering::Less => Some((index, canvas)),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some((index - 1, canvas)),
            })
            .collect();
    }

    fn from_layout(project: &Project, layout: Option<&CanvasLayout>) -> Self {
        let Some(layout) = layout.filter(|layout| layout.matches_project(project)) else {
            return Self::default();
        };
        let mut workspace = Self::default();
        if layout.open_documents.is_empty() {
            for function in &layout.open_functions {
                if project.user_functions.contains_key(function) {
                    workspace.tabs.push(MappingDocument::Function(*function));
                }
            }
        } else {
            for document in &layout.open_documents {
                if let Some(document) = document.into_document(project)
                    && !workspace.tabs.contains(&document)
                {
                    workspace.tabs.push(document);
                }
            }
        }
        let active = layout
            .active_document
            .and_then(|document| document.into_document(project))
            .or_else(|| layout.active_function.map(MappingDocument::Function));
        if let Some(active) = active
            && workspace.tabs.contains(&active)
        {
            workspace.active = active;
            workspace.focused = workspace.active;
        }
        for (&target, nodes) in &layout.target_nodes {
            if target >= project.extra_targets.len() {
                continue;
            }
            let mut snarl = canvas_build::build_named_target_snarl(project, target);
            CanvasLayout::apply_nodes(nodes, &mut snarl);
            workspace
                .target_canvases
                .insert(target, CanvasDocumentState::with_snarl(snarl));
        }
        for (&function, nodes) in &layout.function_nodes {
            let Some(definition) = project.user_functions.get(&function) else {
                continue;
            };
            let mut snarl = build_function_snarl(definition);
            CanvasLayout::apply_nodes(nodes, &mut snarl);
            workspace
                .function_canvases
                .insert(function, CanvasDocumentState::with_snarl(snarl));
        }
        workspace
    }
}

impl CanvasDocumentState {
    fn main(project: &Project) -> Self {
        Self::with_snarl(build_snarl(project))
    }

    fn with_snarl(snarl: Snarl<CanvasNode>) -> Self {
        static NEXT_TOOLS_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            tools_id: egui::Id::unique((
                "canvas_document_tools",
                NEXT_TOOLS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )),
            snarl,
            default_layout_cache: Default::default(),
            node_sizes: std::collections::BTreeMap::new(),
            endpoint_scroll: crate::canvas_endpoints::EndpointScrollState::default(),
            search: crate::canvas_search::CanvasSearchState::default(),
            view_generation: 0,
            viewport_width: 1_200.0,
            pending_focus: None,
            pending_compact: false,
        }
    }

    fn reset_endpoint_catalog_view(&mut self) {
        self.tools_id = self.tools_id.with("new_endpoint_catalog");
        self.node_sizes.clear();
        self.endpoint_scroll = Default::default();
        self.search = Default::default();
        self.pending_focus = None;
    }

    fn reset_view(&mut self) {
        self.view_generation = self.view_generation.wrapping_add(1);
        self.pending_focus = None;
    }

    fn focus_node(&mut self, wanted: CanvasNode) -> bool {
        let Some((position, _)) = self.snarl.nodes_pos().find(|(_, node)| **node == wanted) else {
            return false;
        };
        let size = self.node_sizes.get(&wanted).copied().unwrap_or_default();
        self.pending_focus = Some(position + size / 2.0);
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct CanvasLayout {
    pub(super) version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_fingerprint: Option<String>,
    nodes: Vec<CanvasNodeLayout>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    function_nodes: std::collections::BTreeMap<FunctionId, Vec<CanvasNodeLayout>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    target_nodes: std::collections::BTreeMap<usize, Vec<CanvasNodeLayout>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    open_functions: Vec<FunctionId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    open_documents: Vec<PersistedMappingDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_function: Option<FunctionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_document: Option<PersistedMappingDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CanvasNodeLayout {
    node: PersistedCanvasNode,
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedCanvasNode {
    Source {
        #[serde(default)]
        block: usize,
    },
    Target {
        #[serde(default)]
        block: usize,
    },
    Graph {
        id: NodeId,
    },
    Placeholder {
        id: NodeId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedMappingDocument {
    Target { index: usize },
    Function { id: FunctionId },
}

impl PersistedMappingDocument {
    fn from_document(document: MappingDocument) -> Option<Self> {
        match document {
            MappingDocument::Main => None,
            MappingDocument::Target(index) => Some(Self::Target { index }),
            MappingDocument::Function(id) => Some(Self::Function { id }),
        }
    }

    fn into_document(self, project: &Project) -> Option<MappingDocument> {
        match self {
            Self::Target { index } => {
                (index < project.extra_targets.len()).then_some(MappingDocument::Target(index))
            }
            Self::Function { id } => project
                .user_functions
                .contains_key(&id)
                .then_some(MappingDocument::Function(id)),
        }
    }
}

impl From<CanvasNode> for PersistedCanvasNode {
    fn from(node: CanvasNode) -> Self {
        match node {
            CanvasNode::SourceBlock(block) => Self::Source { block },
            CanvasNode::TargetBlock(block) => Self::Target { block },
            CanvasNode::Graph(id) => Self::Graph { id },
            CanvasNode::Placeholder(id) => Self::Placeholder { id },
        }
    }
}

#[derive(Clone)]
struct EditorSnapshot {
    project: Project,
    state: EditorState,
}

#[derive(Clone)]
struct EditorState {
    serialized_project: String,
    layout: CanvasLayout,
}

impl PartialEq for EditorState {
    fn eq(&self, other: &Self) -> bool {
        self.serialized_project == other.serialized_project && self.layout.same_view(&other.layout)
    }
}

type EditorHistory = EditHistory<EditorSnapshot, graph_commands::GraphCommandRecord>;

impl PartialEq for EditorSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

pub struct FerruleApp {
    project: Project,
    main_canvas: CanvasDocumentState,
    mapping_workspace: MappingWorkspace,
    show_function_navigator: bool,
    function_search: String,
    new_function_draft: Option<function_workspace::NewFunctionDraft>,
    show_source_panel: bool,
    show_inspector_panel: bool,
    show_minimap: bool,
    compact_dock_open: bool,
    compact_dock: SideDock,
    narrow_pane: WorkspacePane,
    last_layout_class: Option<LayoutClass>,
    show_run_setup: bool,
    file_run_breakpoint: Option<run_ui::FileBreakpoint>,
    file_run_value_condition: crate::preview::BreakpointValueConditionDraft,
    file_run_position_condition: crate::preview::BreakpointPositionConditionDraft,
    file_run_source_condition: crate::preview::BreakpointSourceConditionDraft,
    file_run_node_condition: crate::preview::BreakpointNodeConditionDraft,
    file_run_expression_condition: crate::preview::BreakpointExpressionConditionDraft,
    file_run_input_condition: crate::preview::BreakpointInputConditionDraft,
    file_run_pause_on_failure: bool,
    pending_file_run: Option<run_ui::PendingFileRun>,
    file_run_teardown: SavedFileRunTeardownPort,
    /// Session opt-in: filesystem PDF inputs of file runs and file-backed
    /// previews are extracted by an owned packaged worker. Off by default.
    owned_pdf_worker: bool,
    pdf_worker_package: pdf_extraction::PdfWorkerPackage,
    #[cfg(test)]
    pdf_reader_controls: pdf_extraction::ReaderControls,
    preview_input_source: preview_service::PreviewInputSource,
    preview_service: PreviewServicePort,
    /// Close the window once owned previews and the service have finished.
    close_after_preview: bool,
    rest_run_draft: Option<rest_run::RestRunDraft>,
    pending_rest_run: Option<rest_run::PendingRestRun>,
    rest_run_result: Option<rest_run::RestRunResult>,
    close_after_rest_run: bool,
    library_generation_draft: Option<library_generation::LibraryGenerationDraft>,
    pending_library_generation: Option<library_generation::PendingLibraryGeneration>,
    close_after_library_generation: bool,
    close_after_file_run: bool,
    preview_draft: Option<crate::preview::PreviewDraft>,
    preview_value_condition: crate::preview::BreakpointValueConditionDraft,
    preview_position_condition: crate::preview::BreakpointPositionConditionDraft,
    preview_source_condition: crate::preview::BreakpointSourceConditionDraft,
    preview_node_condition: crate::preview::BreakpointNodeConditionDraft,
    preview_expression_condition: crate::preview::BreakpointExpressionConditionDraft,
    preview_input_condition: crate::preview::BreakpointInputConditionDraft,
    preview_pause_on_failure: bool,
    pending_preview: Option<preview_ui::PendingPreview>,
    pipeline_run_draft: Option<crate::pipeline_run::PipelineRunDraft>,
    pending_pipeline_run: Option<pipeline_ui::PendingPipelineRun>,
    pipeline_run_breakpoint: Option<pipeline_ui::PipelineBreakpoint>,
    pipeline_run_value_condition: crate::preview::BreakpointValueConditionDraft,
    pipeline_run_position_condition: crate::preview::BreakpointPositionConditionDraft,
    pipeline_run_source_condition: crate::preview::BreakpointSourceConditionDraft,
    pipeline_run_node_condition: crate::preview::BreakpointNodeConditionDraft,
    pipeline_run_expression_condition: crate::preview::BreakpointExpressionConditionDraft,
    pipeline_run_expression_stage: Option<String>,
    pipeline_run_input_condition: crate::preview::BreakpointInputConditionDraft,
    pipeline_run_pause_on_failure: bool,
    pipeline_run_failure_stage: Option<String>,
    pipeline_run_input_stage: Option<String>,
    close_after_pipeline_run: bool,
    pipeline_editor: Option<pipeline_editor_ui::PipelineEditorUi>,
    pipeline_stage_canvas: Option<pipeline_stage_canvas::StageCanvasSession>,
    embedded_stage_namespace: Option<egui::Id>,
    stage_canvas_generation: u64,
    #[cfg(test)]
    embedded_stage_pin_ids: Vec<egui::Id>,
    pending_pipeline_editor_action: Option<pipeline_editor_ui::PipelineEditorAction>,
    pending_mfd_pipeline_import: Option<mfd_pipeline::PendingMfdPipelineImport>,
    pending_pipeline_mfd_export: Option<mfd_pipeline::PipelineMfdExport>,
    show_run_report: bool,
    show_appearance_editor: bool,
    appearance_tab: AppearanceTab,
    theme: ThemeState,
    appearance: EditorAppearance,
    mfd_package_manifest: Option<String>,
    mfd_item_ordered_exceptions: bool,
    mfd_import_dialog: Option<MfdImportTarget>,
    /// Recently opened or saved project paths, most recent first.
    recent_projects: Vec<String>,
    palette: Palette,
    document: DocumentLocation,
    /// The filename plan and input policy of an opened document project.
    primary_document: Option<primary_document::PrimaryDocumentSession>,
    input_path: String,
    output_path: String,
    host_parameters: host_parameters::HostParameterEditor,
    source_schema_explorer: SchemaExplorerState,
    target_schema_explorer: SchemaExplorerState,
    focused_schema: Option<diagnostic_navigation::SchemaFocus>,
    selected_scope: ScopePath,
    pending_scope_scroll: bool,
    selected_failure_rule: Option<usize>,
    pending_failure_rule_scroll: bool,
    join_authoring_draft: Option<join_authoring::JoinAuthoringDraft>,
    status: String,
    diagnostics: Diagnostics,
    run_report: Option<crate::run_report::RunReportView>,
    new_mapping_setup: Option<NewMappingSetup>,
    extra_source_draft: Option<ExtraSourceDraft>,
    pending_extra_source_removal: Option<usize>,
    extra_target_draft: Option<ExtraTargetDraft>,
    pending_extra_target_removal: Option<usize>,
    pending_auto_connect: Option<PendingAutoConnect>,
    /// Native file dialog receiver; the dialog runs outside the UI thread.
    pending_dialog: Option<(DialogKind, std::sync::mpsc::Receiver<Option<String>>)>,
    #[cfg(test)]
    save_as_dialog_override: Option<std::sync::mpsc::Receiver<Option<String>>>,
    #[cfg(test)]
    pipeline_mfd_dialog_override: Option<std::sync::mpsc::Receiver<Option<String>>>,
    pending_destructive_action: Option<DestructiveAction>,
    pending_save_continuation: Option<SaveContinuation>,
    allow_close: bool,
    observed_editor: EditorSnapshot,
    history: EditorHistory,
    /// Revisions of the observed state; the save point is content-addressed.
    revisions: RevisionTracker,
    saved_state: Option<EditorState>,
    fingerprints: RevisionKeyed<SemanticRevision, String>,
    pending_history: Option<PendingHistory>,
    /// Typed edits canvases applied this frame, awaiting the observer.
    frame_commands: Vec<editor_ui::GraphCommand>,
    /// When the newest step was recorded or extended as one coalescing
    /// command during continuing input.
    command_coalescing: Option<std::time::Instant>,
    /// Records every edit as a snapshot, as before typed commands, so tests
    /// can compare typed history against that path.
    #[cfg(test)]
    snapshot_oracle: bool,
}

struct PendingHistory {
    before: EditorSnapshot,
    last_change: std::time::Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    OpenProject,
    OpenPipeline,
    OpenPipelineEditor,
    CreatePipeline,
    AddPipelineStageProject,
    ImportMfdPipeline,
    ChooseImportedPipelineDestination,
    ExportPipelineMfd,
    SaveProjectAs,
    BrowseInput,
    BrowseOutput,
    ImportMfd,
    BrowseMfdPackageManifest,
    ExportMfd,
    ExportNativeMfd,
    BrowseSourceSchema,
    BrowseTargetSchema,
    BrowseSourceCsv,
    BrowseTargetCsvOutput,
    BrowseSourceSqlite,
    BrowseTargetSqlite,
    BrowseTargetSqliteOutput,
    BrowseExtraSourceSchema,
    BrowseExtraSourceInstance,
    BrowseExtraTargetSchema,
    BrowseExtraTargetOutput,
    BrowseLibraryParent,
    BrowseLibraryRuntime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestructiveAction {
    OpenProject,
    NewProject,
    ImportMfd,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveContinuation {
    Destructive(DestructiveAction),
    Run,
    DebugRun,
    GenerateLibrary,
}

impl Default for FerruleApp {
    fn default() -> Self {
        let project = blank_project();
        let main_canvas = CanvasDocumentState::main(&project);
        let mapping_workspace = MappingWorkspace::default();
        let snapshot = editor_snapshot(&project, &main_canvas.snarl, &mapping_workspace);
        let revisions = RevisionTracker::new(DocumentOrigin::Saved);
        let mut fingerprints = RevisionKeyed::default();
        if let Some(fingerprint) = &snapshot.state.layout.project_fingerprint {
            fingerprints.get_or_compute(revisions.current().semantic(), || fingerprint.clone());
        }
        let saved_state = Some(snapshot.state.clone());
        Self {
            project,
            main_canvas,
            mapping_workspace,
            show_function_navigator: false,
            function_search: String::new(),
            new_function_draft: None,
            show_source_panel: true,
            show_inspector_panel: true,
            show_minimap: true,
            compact_dock_open: true,
            compact_dock: SideDock::Inspector,
            narrow_pane: WorkspacePane::Canvas,
            last_layout_class: None,
            show_run_setup: false,
            file_run_breakpoint: None,
            file_run_value_condition: Default::default(),
            file_run_position_condition: Default::default(),
            file_run_source_condition: Default::default(),
            file_run_node_condition: Default::default(),
            file_run_expression_condition: Default::default(),
            file_run_input_condition: Default::default(),
            file_run_pause_on_failure: false,
            pending_file_run: None,
            file_run_teardown: SavedFileRunTeardownPort::default(),
            owned_pdf_worker: false,
            pdf_worker_package: pdf_extraction::PdfWorkerPackage::beside_current_executable(),
            #[cfg(test)]
            pdf_reader_controls: Default::default(),
            preview_input_source: Default::default(),
            preview_service: PreviewServicePort::default(),
            close_after_preview: false,
            rest_run_draft: None,
            pending_rest_run: None,
            rest_run_result: None,
            close_after_rest_run: false,
            library_generation_draft: None,
            pending_library_generation: None,
            close_after_library_generation: false,
            close_after_file_run: false,
            preview_draft: None,
            preview_value_condition: Default::default(),
            preview_position_condition: Default::default(),
            preview_source_condition: Default::default(),
            preview_node_condition: Default::default(),
            preview_expression_condition: Default::default(),
            preview_input_condition: Default::default(),
            preview_pause_on_failure: false,
            pending_preview: None,
            pipeline_run_draft: None,
            pending_pipeline_run: None,
            pipeline_run_breakpoint: None,
            pipeline_run_value_condition: Default::default(),
            pipeline_run_position_condition: Default::default(),
            pipeline_run_source_condition: Default::default(),
            pipeline_run_node_condition: Default::default(),
            pipeline_run_expression_condition: Default::default(),
            pipeline_run_expression_stage: None,
            pipeline_run_input_condition: Default::default(),
            pipeline_run_pause_on_failure: false,
            pipeline_run_failure_stage: None,
            pipeline_run_input_stage: None,
            close_after_pipeline_run: false,
            pipeline_editor: None,
            pipeline_stage_canvas: None,
            embedded_stage_namespace: None,
            stage_canvas_generation: 0,
            #[cfg(test)]
            embedded_stage_pin_ids: Vec::new(),
            pending_pipeline_editor_action: None,
            pending_mfd_pipeline_import: None,
            pending_pipeline_mfd_export: None,
            show_run_report: false,
            show_appearance_editor: false,
            appearance_tab: AppearanceTab::default(),
            theme: ThemeState::default(),
            appearance: EditorAppearance::default(),
            mfd_package_manifest: None,
            mfd_item_ordered_exceptions: false,
            mfd_import_dialog: None,
            recent_projects: Vec::new(),
            palette: crate::theme::palette(crate::theme::ResolvedTheme::Dark),
            document: DocumentLocation::untitled("project.json"),
            primary_document: None,
            input_path: String::new(),
            output_path: String::new(),
            host_parameters: Default::default(),
            source_schema_explorer: SchemaExplorerState::default(),
            target_schema_explorer: SchemaExplorerState::default(),
            focused_schema: None,
            selected_scope: Vec::new(),
            pending_scope_scroll: false,
            selected_failure_rule: None,
            pending_failure_rule_scroll: false,
            join_authoring_draft: None,
            status: String::new(),
            diagnostics: Diagnostics::default(),
            run_report: None,
            new_mapping_setup: None,
            extra_source_draft: None,
            pending_extra_source_removal: None,
            extra_target_draft: None,
            pending_extra_target_removal: None,
            pending_auto_connect: None,
            pending_dialog: None,
            #[cfg(test)]
            save_as_dialog_override: None,
            #[cfg(test)]
            pipeline_mfd_dialog_override: None,
            pending_destructive_action: None,
            pending_save_continuation: None,
            allow_close: false,
            observed_editor: snapshot,
            history: EditorHistory::default(),
            revisions,
            saved_state,
            fingerprints,
            pending_history: None,
            frame_commands: Vec::new(),
            command_coalescing: None,
            #[cfg(test)]
            snapshot_oracle: false,
        }
    }
}

fn editor_snapshot(
    project: &Project,
    snarl: &Snarl<CanvasNode>,
    workspace: &MappingWorkspace,
) -> EditorSnapshot {
    EditorSnapshot {
        project: project.clone(),
        state: editor_state(project, snarl, workspace),
    }
}

fn editor_state(
    project: &Project,
    snarl: &Snarl<CanvasNode>,
    workspace: &MappingWorkspace,
) -> EditorState {
    let serialized_project = crate::project_state::project_snapshot_key(project);
    let fingerprint = fingerprint_of_key(&serialized_project);
    EditorState {
        serialized_project,
        layout: CanvasLayout::capture_with_fingerprint(project, fingerprint, snarl, workspace),
    }
}

impl CanvasLayout {
    fn capture(project: &Project, snarl: &Snarl<CanvasNode>, workspace: &MappingWorkspace) -> Self {
        Self::capture_with_fingerprint(project, project_fingerprint(project), snarl, workspace)
    }

    fn capture_with_fingerprint(
        project: &Project,
        fingerprint: String,
        snarl: &Snarl<CanvasNode>,
        workspace: &MappingWorkspace,
    ) -> Self {
        let function_nodes = workspace
            .function_canvases
            .iter()
            .map(|(&function, canvas)| {
                (
                    function,
                    Self::capture_document_nodes(canvas, &fingerprint, || {
                        project
                            .user_functions
                            .get(&function)
                            .map(build_function_snarl)
                            .unwrap_or_default()
                    }),
                )
            })
            .collect();
        let target_nodes = workspace
            .target_canvases
            .iter()
            .map(|(&target, canvas)| {
                (
                    target,
                    Self::capture_document_nodes(canvas, &fingerprint, || {
                        canvas_build::build_named_target_snarl(project, target)
                    }),
                )
            })
            .collect();
        let mut open_functions = workspace
            .tabs
            .iter()
            .filter_map(|document| match document {
                MappingDocument::Main => None,
                MappingDocument::Target(_) => None,
                MappingDocument::Function(function) => Some(*function),
            })
            .chain(
                workspace
                    .split
                    .into_iter()
                    .filter_map(|document| match document {
                        MappingDocument::Main => None,
                        MappingDocument::Target(_) => None,
                        MappingDocument::Function(function) => Some(function),
                    }),
            )
            .chain(workspace.floating.iter().copied())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        open_functions.sort_unstable();
        let open_documents = workspace
            .tabs
            .iter()
            .filter_map(|document| PersistedMappingDocument::from_document(*document))
            .collect::<Vec<_>>();
        Self {
            version: LAYOUT_VERSION,
            project_fingerprint: Some(fingerprint),
            nodes: Self::capture_nodes(snarl),
            function_nodes,
            target_nodes,
            open_functions,
            open_documents,
            active_function: match workspace.active {
                MappingDocument::Main => None,
                MappingDocument::Target(_) => None,
                MappingDocument::Function(function) => Some(function),
            },
            active_document: PersistedMappingDocument::from_document(workspace.active),
        }
    }

    fn capture_document_nodes(
        canvas: &CanvasDocumentState,
        fingerprint: &str,
        build_default: impl FnOnce() -> Snarl<CanvasNode>,
    ) -> Vec<CanvasNodeLayout> {
        let nodes = Self::capture_nodes(&canvas.snarl);
        let mut baseline = canvas.default_layout_cache.borrow_mut();
        if baseline.as_ref().is_none_or(|(key, _)| key != fingerprint) {
            *baseline = Some((
                fingerprint.to_owned(),
                Self::capture_nodes(&build_default()),
            ));
        }
        if baseline
            .as_ref()
            .is_some_and(|(_, default)| default == &nodes)
        {
            Vec::new()
        } else {
            nodes
        }
    }

    fn capture_nodes(snarl: &Snarl<CanvasNode>) -> Vec<CanvasNodeLayout> {
        let mut nodes: Vec<_> = snarl
            .nodes_pos()
            .map(|(pos, &node)| CanvasNodeLayout {
                node: node.into(),
                x: pos.x,
                y: pos.y,
            })
            .collect();
        nodes.sort_by_key(|entry| entry.node);
        nodes
    }

    fn apply_nodes(nodes: &[CanvasNodeLayout], snarl: &mut Snarl<CanvasNode>) {
        let positions: std::collections::BTreeMap<_, _> = nodes
            .iter()
            .filter(|entry| entry.x.is_finite() && entry.y.is_finite())
            .map(|entry| (entry.node, egui::pos2(entry.x, entry.y)))
            .collect();
        let node_ids: Vec<_> = snarl
            .node_ids()
            .map(|(id, &node)| (id, PersistedCanvasNode::from(node)))
            .collect();
        for (id, node) in node_ids {
            if let Some(&pos) = positions.get(&node)
                && let Some(info) = snarl.get_node_info_mut(id)
            {
                info.pos = pos;
            }
        }
    }

    /// Equality of saved canvas geometry; navigation is persisted opportunistically
    /// on save, but opening or selecting a tab is not an edit.
    fn same_view(&self, other: &Self) -> bool {
        self.version == other.version
            && self.nodes == other.nodes
            // Empty entries remember a visited default canvas, not custom
            // geometry. Ignore only those markers when comparing edits.
            && self.function_nodes.iter().filter(|(_, nodes)| !nodes.is_empty()).eq(other.function_nodes.iter().filter(|(_, nodes)| !nodes.is_empty()))
            && self.target_nodes.iter().filter(|(_, nodes)| !nodes.is_empty()).eq(other.target_nodes.iter().filter(|(_, nodes)| !nodes.is_empty()))
    }

    fn matches_project(&self, project: &Project) -> bool {
        (1..=LAYOUT_VERSION).contains(&self.version)
            && self.project_fingerprint.as_deref() == Some(project_fingerprint(project).as_str())
    }
}

/// Runs a native open dialog on its own thread; the result arrives through
/// the returned channel (never blocking the UI, even with no dialog
/// backend available).
fn pick_file(description: &str, extensions: &[&str]) -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let description = description.to_string();
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    std::thread::spawn(move || {
        let result = rfd::FileDialog::new()
            .add_filter(description, &extensions)
            .pick_file()
            .map(|p| p.display().to_string());
        let _ = tx.send(result);
    });
    rx
}

/// Threaded native save dialog, pre-filled from `current`.
fn save_file(
    description: &str,
    extensions: &[&str],
    current: &str,
) -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let description = description.to_string();
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    let current = std::path::PathBuf::from(current);
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new().add_filter(description, &extensions);
        if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
            dialog = dialog.set_file_name(name);
        }
        let _ = tx.send(dialog.save_file().map(|p| p.display().to_string()));
    });
    rx
}

impl FerruleApp {
    pub fn from_storage(storage: Option<&dyn eframe::Storage>) -> Self {
        let preferences = crate::preferences::load(storage);
        Self {
            theme: preferences.theme,
            appearance: preferences.appearance,
            show_minimap: preferences.show_minimap,
            mfd_package_manifest: preferences.mfd_package_manifest,
            recent_projects: preferences.recent_projects,
            ..Self::default()
        }
    }

    pub(crate) fn from_storage_with_owners(
        storage: Option<&dyn eframe::Storage>,
        owner: SavedFileRunTeardownPort,
        preview_service: PreviewServicePort,
    ) -> Self {
        Self {
            file_run_teardown: owner,
            preview_service,
            ..Self::from_storage(storage)
        }
    }

    // Dropping the pending facade transfers its entire live core. The app's
    // automatic field destruction uses the same path, without a FerruleApp
    // Drop impl that would break existing struct-update constructors.
    #[cfg(test)]
    pub(super) fn handoff_saved_file_run(&mut self) {
        drop(self.pending_file_run.take());
    }

    fn refresh_source_canvases(&mut self, previous_names: &[String]) {
        self.main_canvas.snarl = canvas_build::rebuild_source_snarl(
            &self.project,
            previous_names,
            &self.main_canvas.snarl,
            None,
        );
        self.main_canvas.reset_endpoint_catalog_view();
        for (&target, canvas) in &mut self.mapping_workspace.target_canvases {
            canvas.snarl = canvas_build::rebuild_source_snarl(
                &self.project,
                previous_names,
                &canvas.snarl,
                Some(target),
            );
            canvas.reset_endpoint_catalog_view();
        }
    }

    fn ui_project_editing_enabled(&self) -> bool {
        self.project_editing_enabled() && !self.pipeline_mfd_busy()
    }

    fn handle_history_shortcuts(&mut self, context: &egui::Context, editing_enabled: bool) {
        if !editing_enabled {
            return;
        }
        let [undo, redo, redo_secondary] = history_shortcuts();
        // Logical modifier matching permits extra Shift, so redo must consume
        // Command+Shift+Z before the less specific Command+Z undo shortcut.
        if context.input_mut(|input| {
            input.consume_shortcut(&redo) || input.consume_shortcut(&redo_secondary)
        }) {
            self.redo_project();
        } else if context.input_mut(|input| input.consume_shortcut(&undo)) {
            self.undo_project();
        }
    }

    /// Returns the action when it can run immediately. Dirty projects queue
    /// the action for the shared save/discard/cancel confirmation instead.
    fn request_destructive_action(
        &mut self,
        action: DestructiveAction,
    ) -> Option<DestructiveAction> {
        if self.rest_run_busy() {
            return None;
        }
        if self.is_dirty() {
            self.pending_destructive_action = Some(action);
            None
        } else {
            Some(action)
        }
    }

    fn perform_destructive_action(&mut self, action: DestructiveAction, ctx: &egui::Context) {
        if self.rest_run_busy() {
            return;
        }
        match action {
            DestructiveAction::OpenProject => {
                self.pending_dialog = Some((
                    DialogKind::OpenProject,
                    pick_file("ferrule project", &["json"]),
                ));
            }
            DestructiveAction::NewProject => {
                self.clear_run_report();
                self.project = blank_project();
                self.primary_document = None;
                self.host_parameters = host_parameters::HostParameterEditor::default();
                self.mapping_workspace.reset();
                self.main_canvas = CanvasDocumentState::main(&self.project);
                self.reset_canvas_view();
                self.document = DocumentLocation::untitled("project.json");
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                self.diagnostics.clear();
                self.status = "new project".to_string();
                self.begin_new_mapping();
            }
            DestructiveAction::ImportMfd => {
                self.pending_dialog =
                    Some((DialogKind::ImportMfd, pick_file("MFD design", &["mfd"])));
            }
            DestructiveAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn show_unsaved_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_destructive_action else {
            return;
        };
        let name = self.document.title();
        let location = self.document.location_hint();
        let response =
            crate::ui_kit::dialog(ctx, crate::ui_kit::DialogKind::Modal, "Unsaved changes").show(
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Save changes to ");
                        ui.label(egui::RichText::new(&name).strong());
                        ui.label(" before continuing?");
                    });
                    ui.add(
                        egui::Label::new(crate::ui_kit::TextRole::Help.text(&location)).truncate(),
                    )
                    .on_hover_text(&location);
                },
                |ui| {
                    crate::ui_kit::dialog_buttons(
                        ui,
                        "Save".into(),
                        Some("Cancel".into()),
                        Some("Discard changes".into()),
                    )
                },
            );
        let choice = match response.footer {
            Some(crate::ui_kit::DialogAction::Primary) => Some(true),
            Some(crate::ui_kit::DialogAction::Destructive) => Some(false),
            Some(crate::ui_kit::DialogAction::Secondary) => {
                self.pending_destructive_action = None;
                None
            }
            None => {
                if response.close_requested {
                    self.pending_destructive_action = None;
                }
                None
            }
        };

        match choice {
            Some(true) => {
                self.save_with_continuation(Some(SaveContinuation::Destructive(action)), ctx)
            }
            Some(false) => {
                self.pending_destructive_action = None;
                self.perform_destructive_action(action, ctx);
            }
            None => {}
        }
    }

    fn load_project_from(&mut self, path: &std::path::Path) {
        if self.stage_document_actions_blocked() {
            return;
        }
        let loaded = (|| -> anyhow::Result<(Project, Option<mapping::PrimarySourcePlan>)> {
            let bytes = std::fs::read(path)?;
            match mapping::project_file::decode_bytes(&bytes) {
                Ok(project) => Ok((project, None)),
                // A document project keeps its filename plan beside the project.
                Err(error) => match mapping::document_project_file::decode_bytes(&bytes) {
                    Ok(document) => Ok((
                        document.project().clone(),
                        Some(document.primary_source().clone()),
                    )),
                    Err(_) => Err(error.into()),
                },
            }
        })();
        match loaded {
            Ok((project, plan)) => {
                self.primary_document =
                    plan.map(|plan| primary_document::PrimaryDocumentSession::new(plan, path));
                let (layout, layout_warning) = match read_layout(path) {
                    Ok(layout) => (layout, None),
                    Err(error) => (None, Some(error.to_string())),
                };
                self.main_canvas = CanvasDocumentState::with_snarl(build_snarl_with_layout(
                    &project,
                    layout.as_ref(),
                ));
                let mapping_workspace = MappingWorkspace::from_layout(&project, layout.as_ref());
                self.reset_canvas_view();
                self.clear_run_report();
                self.project = project;
                self.host_parameters = host_parameters::HostParameterEditor::default();
                self.mapping_workspace = mapping_workspace;
                self.document = DocumentLocation::saved(path);
                crate::preferences::remember_recent_project(
                    &mut self.recent_projects,
                    path.display().to_string(),
                );
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                let validation = cli::validate(&self.project);
                let fingerprint = project_fingerprint(&self.project);
                let mut diagnostics = validation
                    .into_iter()
                    .map(|issue| {
                        Diagnostic::validation_with_fingerprint(&self.project, issue, &fingerprint)
                    })
                    .collect::<Vec<_>>();
                diagnostics.extend(layout_warning.map(|warning| {
                    Diagnostic::warning(format!("using default canvas layout: {warning}"))
                }));
                if diagnostics.is_empty() {
                    self.diagnostics.clear();
                } else {
                    self.diagnostics.replace("Loaded project", diagnostics);
                }
                self.status = format!("loaded {}", crate::ui_kit::file_label(path));
            }
            Err(error) => {
                self.status = format!("failed to load {}", crate::ui_kit::file_label(path));
                self.diagnostics.error("Open failed", error.to_string());
            }
        }
    }

    fn start_save_as(&mut self, continuation: Option<SaveContinuation>) {
        if self.stage_document_actions_blocked() {
            return;
        }
        self.pending_save_continuation = continuation;
        #[cfg(test)]
        let receiver = self.save_as_dialog_override.take().unwrap_or_else(|| {
            save_file("ferrule project", &["json"], &self.document.display_path())
        });
        #[cfg(not(test))]
        let receiver = save_file("ferrule project", &["json"], &self.document.display_path());
        self.pending_dialog = Some((DialogKind::SaveProjectAs, receiver));
    }

    fn save_with_continuation(
        &mut self,
        continuation: Option<SaveContinuation>,
        ctx: &egui::Context,
    ) {
        if self.rest_run_busy() {
            return;
        }
        let Some(path) = self.document.saved_path().map(std::path::Path::to_path_buf) else {
            self.pending_destructive_action = None;
            self.start_save_as(continuation);
            return;
        };
        match self.save_document_to(&path) {
            Ok(outcome) => {
                self.apply_save_outcome(&path, outcome);
                self.pending_destructive_action = None;
                self.complete_save_continuation(continuation, ctx);
            }
            Err(error) => {
                if continuation == Some(SaveContinuation::GenerateLibrary) {
                    self.library_generation_save_failed(&error);
                }
                self.status = format!("failed to save {}", path.display());
                self.diagnostics.error("Save failed", error.to_string());
            }
        }
    }

    fn complete_save_continuation(
        &mut self,
        continuation: Option<SaveContinuation>,
        ctx: &egui::Context,
    ) {
        match continuation {
            Some(SaveContinuation::Destructive(action)) => {
                self.perform_destructive_action(action, ctx)
            }
            Some(SaveContinuation::Run) => self.run_saved(false),
            Some(SaveContinuation::DebugRun) => self.run_saved(true),
            Some(SaveContinuation::GenerateLibrary) => self.generate_saved_library(),
            None => {}
        }
    }

    /// Cancel the selected file action before dropping its result receiver.
    pub(super) fn cancel_pending_file_dialog(&mut self) {
        let Some(kind) = self.pending_dialog.as_ref().map(|(kind, _)| *kind) else {
            return;
        };
        self.cancel_pipeline_mfd_dialog(kind);
        self.library_generation_save_cancelled();
        self.pending_dialog = None;
        self.pending_save_continuation = None;
        self.status = "file dialog cancelled".to_string();
    }

    fn poll_dialog_with_close_guard(&mut self, ctx: &egui::Context, close_requested: bool) {
        if close_requested {
            self.cancel_pipeline_mfd_for_app_close();
        }
        self.poll_dialog(ctx);
    }

    /// Applies the result of a finished file dialog, if any.
    fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some((kind, rx)) = &self.pending_dialog else {
            return;
        };
        let kind = *kind;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.pending_dialog = None;
        let Some(path) = result else {
            if kind == DialogKind::SaveProjectAs {
                self.library_generation_save_cancelled();
                self.pending_save_continuation = None;
            }
            self.cancel_pipeline_mfd_dialog(kind);
            return; // cancelled or no dialog backend
        };
        match kind {
            DialogKind::OpenProject => {
                self.load_project_from(std::path::Path::new(&path));
            }
            DialogKind::OpenPipeline => {
                self.load_pipeline_for_run(std::path::Path::new(&path));
            }
            DialogKind::OpenPipelineEditor => {
                self.request_pipeline_editor_action(
                    pipeline_editor_ui::PipelineEditorAction::Open(PathBuf::from(path)),
                );
            }
            DialogKind::CreatePipeline => {
                self.request_pipeline_editor_action(
                    pipeline_editor_ui::PipelineEditorAction::Create(PathBuf::from(path)),
                );
            }
            DialogKind::ImportMfdPipeline => {
                if let Err(error) = self.stage_mfd_pipeline_source(PathBuf::from(&path)) {
                    self.status = format!("failed to import pipeline {path}");
                    self.diagnostics
                        .error("MFD pipeline import failed", error.to_string());
                }
            }
            DialogKind::ChooseImportedPipelineDestination => {
                if let Err(error) = self.finish_mfd_pipeline_import(std::path::Path::new(&path)) {
                    self.status = "failed to create imported pipeline".into();
                    self.diagnostics
                        .error("MFD pipeline import failed", format!("{error:#}"));
                }
            }
            DialogKind::ExportPipelineMfd => {
                self.finish_pipeline_mfd_export(std::path::Path::new(&path));
            }
            DialogKind::AddPipelineStageProject => {
                self.add_pipeline_stage_from_path(std::path::Path::new(&path));
            }
            DialogKind::SaveProjectAs => {
                let continuation = self.pending_save_continuation.take();
                let path = PathBuf::from(path);
                match self.save_document_to(&path) {
                    Ok(outcome) => {
                        self.apply_save_outcome(&path, outcome);
                        self.complete_save_continuation(continuation, ctx);
                    }
                    Err(error) => {
                        if continuation == Some(SaveContinuation::GenerateLibrary) {
                            self.library_generation_save_failed(&error);
                        }
                        self.status = format!("failed to save {}", path.display());
                        self.diagnostics.error("Save failed", error.to_string());
                    }
                }
            }
            DialogKind::BrowseLibraryParent => {
                self.stage_library_parent_folder(PathBuf::from(path))
            }
            DialogKind::BrowseLibraryRuntime => self.stage_library_runtime_folder(path),
            DialogKind::BrowseInput => self.input_path = path,
            DialogKind::BrowseOutput => self.output_path = path,
            DialogKind::BrowseSourceSchema => {
                self.stage_mapping_schema(SchemaSide::Source, PathBuf::from(path));
            }
            DialogKind::BrowseTargetSchema => {
                self.stage_mapping_schema(SchemaSide::Target, PathBuf::from(path));
            }
            DialogKind::BrowseSourceCsv => {
                self.stage_mapping_csv_source(PathBuf::from(path));
            }
            DialogKind::BrowseTargetCsvOutput => {
                self.stage_mapping_csv_target_output(path);
            }
            DialogKind::BrowseSourceSqlite => {
                self.stage_mapping_sqlite(SchemaSide::Source, PathBuf::from(path));
            }
            DialogKind::BrowseTargetSqlite => {
                self.stage_mapping_sqlite(SchemaSide::Target, PathBuf::from(path));
            }
            DialogKind::BrowseTargetSqliteOutput => {
                self.stage_mapping_sqlite_target_output(path);
            }
            DialogKind::BrowseExtraSourceSchema => {
                self.stage_extra_source_schema(PathBuf::from(path));
            }
            DialogKind::BrowseExtraSourceInstance => {
                if let Some(draft) = &mut self.extra_source_draft {
                    draft.set_instance_path(path);
                }
            }
            DialogKind::BrowseExtraTargetSchema => {
                self.stage_extra_target_schema(PathBuf::from(path));
            }
            DialogKind::BrowseExtraTargetOutput => {
                if let Some(draft) = &mut self.extra_target_draft {
                    draft.output_path = path;
                }
            }
            DialogKind::BrowseMfdPackageManifest => {
                match mfd::PackageManifest::load(std::path::Path::new(&path)) {
                    Ok(manifest) => {
                        let canonical = manifest.path().display().to_string();
                        self.mfd_package_manifest = Some(canonical.clone());
                        self.status = format!("selected MFD package manifest {canonical}");
                        self.diagnostics.clear();
                    }
                    Err(error) => {
                        self.status = format!("invalid MFD package manifest {path}");
                        self.diagnostics
                            .error("MFD package manifest", error.to_string());
                    }
                }
            }
            DialogKind::ImportMfd => match self.import_mfd_with_resources(&path) {
                Ok(imported) => {
                    let project_path = imported.mapping_path.with_extension("json");
                    self.clear_run_report();
                    self.main_canvas = CanvasDocumentState::main(&imported.project);
                    self.reset_canvas_view();
                    self.project = imported.project;
                    self.primary_document = None;
                    self.host_parameters = host_parameters::HostParameterEditor::default();
                    self.mapping_workspace.reset();
                    self.mark_unsaved();
                    self.selected_scope.clear();
                    self.rebase_history();
                    self.document = DocumentLocation::untitled(project_path);
                    let validation = cli::validate(&self.project);
                    let fingerprint = project_fingerprint(&self.project);
                    let mut diagnostics = imported
                        .warnings
                        .iter()
                        .cloned()
                        .map(|message| Diagnostic::import_warning(message, &imported.mapping_path))
                        .collect::<Vec<_>>();
                    diagnostics.extend(validation.into_iter().map(|issue| {
                        Diagnostic::validation_with_fingerprint(&self.project, issue, &fingerprint)
                    }));
                    if diagnostics.is_empty() {
                        self.diagnostics.clear();
                    } else {
                        self.diagnostics.replace("MFD import", diagnostics);
                    }
                    self.status = if imported.warnings.is_empty() {
                        format!("imported {}", crate::ui_kit::file_label(&path))
                    } else {
                        format!(
                            "imported {} with {}",
                            crate::ui_kit::file_label(&path),
                            crate::ui_kit::plural(imported.warnings.len(), "warning")
                        )
                    };
                }
                Err(error) => {
                    self.status = format!("failed to import {path}");
                    self.diagnostics
                        .error("MFD import failed", error.to_string());
                }
            },
            DialogKind::ExportMfd => self.finish_mfd_export(
                std::path::Path::new(&path),
                mfd::ExportProfile::FerruleExtensions,
            ),
            DialogKind::ExportNativeMfd => {
                self.finish_mfd_export(std::path::Path::new(&path), mfd::ExportProfile::NativeMfd)
            }
        }
    }
}

impl eframe::App for FerruleApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        crate::preferences::store(
            storage,
            &crate::preferences::EditorPreferences::new(
                self.theme,
                self.appearance,
                self.show_minimap,
                self.mfd_package_manifest.clone(),
            )
            .with_recent_projects(self.recent_projects.clone()),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.palette = self.theme.apply(ui.ctx());
        let layout_class = LayoutClass::from_width(ui.available_width());
        if self.last_layout_class != Some(layout_class) {
            self.last_layout_class = Some(layout_class);
        }
        let close_requested = ui.ctx().input(|input| input.viewport().close_requested());
        self.poll_dialog_with_close_guard(ui.ctx(), close_requested);
        self.poll_library_generation(ui.ctx());
        self.poll_pipeline_run(ui.ctx());
        self.poll_preview(ui.ctx());
        self.poll_preview_service(ui.ctx());
        self.poll_file_run(ui.ctx());
        self.poll_rest_run(ui.ctx());
        if !self.guard_library_generation_close_requested(ui.ctx(), close_requested) {
            self.guard_app_close_requested(ui.ctx(), close_requested);
        }
        if self.pipeline_stage_canvas.is_some() {
            self.show_pipeline_stage_canvas(ui);
            self.show_pipeline_stage_close_guard(ui.ctx());
            return;
        }
        let project_editing_enabled = self.ui_project_editing_enabled();
        let [undo_shortcut, redo_shortcut, _] = history_shortcuts();
        let coalesce_history_change = ui.ctx().input(|input| {
            input.pointer.primary_down()
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Cut
                            | egui::Event::Paste(_)
                            | egui::Event::Text(_)
                            | egui::Event::Key { .. }
                            | egui::Event::Ime(_)
                    )
                })
        });
        self.handle_history_shortcuts(ui.ctx(), project_editing_enabled);
        if self.pending_dialog.is_some() {
            // Keep polling even without input events.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::Panel::top("top_panel").show(ui, |ui| {
            self.show_command_bar(
                ui,
                project_editing_enabled,
                layout_class,
                &undo_shortcut,
                &redo_shortcut,
            );
        });

        egui::Panel::bottom("status_bar")
            .exact_size(crate::theme::METRICS.status_height)
            .show(ui, |ui| self.show_status_bar(ui));
        if !self.diagnostics.is_empty() {
            let mut navigate = None;
            egui::Panel::bottom("diagnostics_panel")
                .resizable(true)
                .default_size(crate::theme::METRICS.diagnostics_height)
                .show(ui, |ui| navigate = self.diagnostics.show(ui));
            if let Some(diagnostic) = navigate {
                self.navigate_to_diagnostic(&diagnostic);
            }
        }
        crate::ui_kit::set_workspace_rect(ui.ctx(), ui.available_rect_before_wrap());

        if self.is_start_state() {
            egui::CentralPanel::default().show(ui, |ui| self.show_start_view(ui));
            self.show_editor_windows(ui, project_editing_enabled, coalesce_history_change);
            return;
        }

        let visibility = WorkspaceVisibility::resolve(
            layout_class,
            self.show_source_panel,
            self.show_inspector_panel,
            self.compact_dock_open,
            self.compact_dock,
            self.narrow_pane,
        );
        let source_size = crate::theme::METRICS.source_panel;
        if visibility.source_dock {
            egui::Panel::left("source_schema")
                .resizable(true)
                .default_size(source_size.default)
                .min_size(source_size.min)
                .max_size(source_size.max)
                .show(ui, |ui| {
                    crate::ui_kit::pane(ui, "source", |ui| {
                        self.show_source_explorer(ui, project_editing_enabled)
                    })
                });
        }

        let inspector_size = crate::theme::METRICS.inspector_panel;
        if visibility.inspector_dock {
            egui::Panel::right("target_schema_and_scopes")
                .resizable(true)
                .default_size(inspector_size.default)
                .min_size(inspector_size.min)
                .max_size(inspector_size.max)
                .show(ui, |ui| {
                    crate::ui_kit::pane(ui, "inspector", |ui| {
                        self.show_inspector(ui, project_editing_enabled)
                    })
                });
        }

        egui::CentralPanel::default().show(ui, |ui| match visibility.center {
            WorkspacePane::Source => crate::ui_kit::pane(ui, "source", |ui| {
                self.show_source_explorer(ui, project_editing_enabled)
            }),
            WorkspacePane::Canvas => {
                self.show_mapping_workspace_canvas(ui, project_editing_enabled)
            }
            WorkspacePane::Inspector => crate::ui_kit::pane(ui, "inspector", |ui| {
                self.show_inspector(ui, project_editing_enabled)
            }),
        });
        self.show_editor_windows(ui, project_editing_enabled, coalesce_history_change);
    }
}

impl FerruleApp {
    /// Windows and dialogs drawn over the workspace, then the per-frame
    /// history observation and window title.
    fn show_editor_windows(
        &mut self,
        ui: &mut egui::Ui,
        project_editing_enabled: bool,
        coalesce_history_change: bool,
    ) {
        self.show_run_settings(ui.ctx(), project_editing_enabled);
        self.show_mfd_import_dialog(ui.ctx());
        crate::appearance_editor::show(
            ui.ctx(),
            &mut self.show_appearance_editor,
            &mut self.appearance_tab,
            &mut self.theme,
            &mut self.appearance,
            self.palette,
        );

        if let Some(report) = &mut self.run_report {
            crate::run_report::show(ui.ctx(), &mut self.show_run_report, report);
        } else {
            self.show_run_report = false;
        }

        self.show_library_generation(ui.ctx());
        self.show_unsaved_confirmation(ui.ctx());
        self.show_new_mapping_setup(ui.ctx());
        self.show_extra_source_setup(ui.ctx());
        self.show_extra_source_removal_confirmation(ui.ctx());
        self.show_extra_target_setup(ui.ctx());
        self.show_extra_target_removal_confirmation(ui.ctx());
        self.show_auto_connect_confirmation(ui.ctx());
        self.show_preview_setup(ui.ctx());
        self.show_file_run_progress(ui.ctx());
        self.show_rest_run(ui.ctx());
        self.show_pipeline_run_setup(ui.ctx());
        self.show_pipeline_editor(ui.ctx());
        if self.pipeline_stage_canvas.is_some() {
            return;
        }
        self.show_pipeline_editor_guard(ui.ctx());
        self.show_new_function_dialog(ui.ctx(), project_editing_enabled);
        self.show_function_navigator(ui.ctx(), project_editing_enabled);
        self.show_floating_function_windows(ui.ctx(), project_editing_enabled);
        let was_dirty = self.observed_dirty();
        if let Some(repaint_after) =
            self.observe_editor_history(std::time::Instant::now(), coalesce_history_change)
        {
            ui.ctx().request_repaint_after(repaint_after);
        }
        if self.observed_dirty() != was_dirty {
            // The status bar was drawn before this frame's edits were observed.
            ui.ctx().request_repaint();
        }
        let marker = if self.observed_dirty() { " *" } else { "" };
        let name = self.document.title();
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "{name}{marker} - ferrule"
            )));
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod canvas_presentation_tests;

#[cfg(test)]
mod sort_controls_tests;

#[cfg(test)]
mod window_controls_tests;

#[cfg(test)]
mod copy_scope_tests;

#[cfg(test)]
mod scope_retirement_tests;

#[cfg(test)]
mod xml_output_controls_tests;

#[cfg(test)]
mod primary_root_authoring_tests;

#[cfg(test)]
mod target_xml_type_tests;

#[cfg(test)]
mod pipeline_stage_canvas_tests;

#[cfg(test)]
mod raise_creation_tests;

#[cfg(test)]
mod runtime_value_creation_tests;

#[cfg(test)]
mod computed_aggregate_authoring_tests;

#[cfg(test)]
mod value_map_input_type_tests;

#[cfg(test)]
mod computed_property_tests;

#[cfg(test)]
mod value_map_default_presence_tests;

#[cfg(test)]
mod typed_value_map_cell_tests;

#[cfg(test)]
mod named_source_endpoint_tests;

#[cfg(test)]
mod typed_edit_oracle;

#[cfg(test)]
mod inspector_commands_tests;

#[cfg(test)]
mod project_commands_tests;

#[cfg(test)]
mod primary_document_tests;
