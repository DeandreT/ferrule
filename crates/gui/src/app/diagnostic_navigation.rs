use super::*;

use engine::{
    ValidationEndpoint, ValidationOwner, ValidationSchemaLocation, ValidationSchemaStep,
    ValidationScopeLocation, ValidationScopeStep,
};
use ir::{SchemaKind, SchemaNode};
use mapping::ScopeIteration;

use crate::diagnostics::{Diagnostic, DiagnosticLocation};

/// A visual schema location. Constraint-only descendants focus their nearest
/// visible ancestor while the full typed owner remains in the diagnostic.
#[derive(Clone, Debug)]
pub(super) struct SchemaFocus {
    pub(super) endpoint: ValidationEndpoint,
    pub(super) visible_path: Vec<usize>,
    pub(super) pending_scroll: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NavigationDestination {
    GraphNode {
        function: Option<FunctionId>,
        node: NodeId,
    },
    UserFunction(FunctionId),
    Schema {
        endpoint: ValidationEndpoint,
        visible_path: Vec<usize>,
        exact: bool,
    },
    Scope {
        target: Option<usize>,
        visible_path: ScopePath,
        exact: bool,
    },
    FailureRule(usize),
}

fn endpoint_schema<'a>(
    project: &'a Project,
    endpoint: &ValidationEndpoint,
) -> Option<&'a SchemaNode> {
    match endpoint {
        ValidationEndpoint::Source => Some(&project.source),
        ValidationEndpoint::Target => Some(&project.target),
        ValidationEndpoint::NamedSource { index, name } => project
            .extra_sources
            .get(*index)
            .filter(|source| source.name == *name)
            .map(|source| &source.schema),
        ValidationEndpoint::NamedTarget { index, name } => project
            .extra_targets
            .get(*index)
            .filter(|target| target.name == *name)
            .map(|target| &target.schema),
    }
}

fn target_scope<'a>(
    project: &'a Project,
    endpoint: &ValidationEndpoint,
) -> Option<(Option<usize>, &'a Scope)> {
    match endpoint {
        ValidationEndpoint::Target => Some((None, &project.root)),
        ValidationEndpoint::NamedTarget { index, name } => project
            .extra_targets
            .get(*index)
            .filter(|target| target.name == *name)
            .map(|target| (Some(*index), &target.root)),
        ValidationEndpoint::Source | ValidationEndpoint::NamedSource { .. } => None,
    }
}

fn schema_destination(
    project: &Project,
    location: &ValidationSchemaLocation,
) -> Option<NavigationDestination> {
    let mut schema = endpoint_schema(project, &location.endpoint)?;
    let mut visible_path = Vec::new();
    let mut exact = true;
    for step in &location.path {
        schema = match step {
            ValidationSchemaStep::Child(index) => {
                let SchemaKind::Group { children, .. } = &schema.kind else {
                    return None;
                };
                if exact {
                    visible_path.push(*index);
                }
                children.get(*index)?
            }
            ValidationSchemaStep::DynamicField => {
                exact = false;
                schema.dynamic_fields()?
            }
            ValidationSchemaStep::Contains(index) => {
                exact = false;
                schema
                    .json_contains
                    .as_ref()?
                    .as_slice()
                    .get(*index)?
                    .predicate()
                    .as_schema()?
            }
            ValidationSchemaStep::DependentSchema(index) => {
                exact = false;
                schema
                    .json_dependent_schemas
                    .as_ref()?
                    .as_slice()
                    .get(*index)?
                    .predicate()
                    .as_schema()?
            }
        };
    }
    Some(NavigationDestination::Schema {
        endpoint: location.endpoint.clone(),
        visible_path,
        exact,
    })
}

fn scope_destination(
    project: &Project,
    location: &ValidationScopeLocation,
) -> Option<NavigationDestination> {
    let (target, mut scope) = target_scope(project, &location.target)?;
    let mut visible_path = Vec::new();
    let mut exact = true;
    for step in &location.path {
        scope = match step {
            ValidationScopeStep::Child(index) => {
                if exact {
                    visible_path.push(*index);
                }
                scope.children.get(*index)?
            }
            ValidationScopeStep::DynamicChild(index) => {
                exact = false;
                &scope.dynamic_children.get(*index)?.scope
            }
            ValidationScopeStep::Segment(index) => {
                exact = false;
                let ScopeIteration::Concatenate(segments) = &scope.iteration else {
                    return None;
                };
                segments.iter().nth(*index)?
            }
        };
    }
    Some(NavigationDestination::Scope {
        target,
        visible_path,
        exact,
    })
}

fn resolve_destination(
    project: &Project,
    owner: &ValidationOwner,
) -> Option<NavigationDestination> {
    match owner {
        ValidationOwner::GraphNode { function, node } => {
            let exists = function.map_or_else(
                || project.graph.nodes.contains_key(node),
                |id| {
                    project
                        .user_functions
                        .get(&id)
                        .is_some_and(|function| function.body.nodes.contains_key(node))
                },
            );
            exists.then_some(NavigationDestination::GraphNode {
                function: *function,
                node: *node,
            })
        }
        ValidationOwner::UserFunction(id) => project
            .user_functions
            .contains_key(id)
            .then_some(NavigationDestination::UserFunction(*id)),
        ValidationOwner::Endpoint(endpoint) => {
            endpoint_schema(project, endpoint).map(|_| NavigationDestination::Schema {
                endpoint: endpoint.clone(),
                visible_path: Vec::new(),
                exact: true,
            })
        }
        ValidationOwner::SchemaNode(location) => schema_destination(project, location),
        ValidationOwner::Scope(location) => scope_destination(project, location),
        ValidationOwner::FailureRule { index } => (*index < project.failure_rules.len())
            .then_some(NavigationDestination::FailureRule(*index)),
    }
}

impl FerruleApp {
    pub(super) fn clear_diagnostic_navigation(&mut self) {
        self.focused_schema = None;
        self.pending_scope_scroll = false;
        self.selected_failure_rule = None;
        self.pending_failure_rule_scroll = false;
        self.join_authoring_draft = None;
    }

    /// Revalidation makes a diagnostic's snapshot owner safe to use after edits.
    pub(super) fn navigate_to_diagnostic(&mut self, diagnostic: &Diagnostic) -> bool {
        let Some(DiagnosticLocation::Validation(owner)) = &diagnostic.location else {
            return false;
        };
        if diagnostic.project_fingerprint.as_deref()
            != Some(project_fingerprint(&self.project).as_str())
        {
            self.status = "diagnostic is out of date; validate again".to_string();
            return false;
        }
        let still_current = cli::validate(&self.project).into_iter().any(|issue| {
            issue.owner.as_ref() == Some(owner)
                && crate::diagnostics::validation_display_message(&self.project, &issue)
                    == diagnostic.message
        });
        if !still_current {
            self.status = "diagnostic is out of date; validate again".to_string();
            return false;
        }
        let Some(destination) = resolve_destination(&self.project, owner) else {
            self.status = "diagnostic location is no longer available".to_string();
            return false;
        };
        self.navigate_to_destination(destination)
    }

    fn show_canvas_for_navigation(&mut self) {
        self.narrow_pane = WorkspacePane::Canvas;
    }

    fn show_source_for_navigation(&mut self) {
        self.show_source_panel = true;
        self.compact_dock_open = true;
        self.compact_dock = SideDock::Source;
        self.narrow_pane = WorkspacePane::Source;
    }

    fn show_inspector_for_navigation(&mut self) {
        self.show_inspector_panel = true;
        self.compact_dock_open = true;
        self.compact_dock = SideDock::Inspector;
        self.narrow_pane = WorkspacePane::Inspector;
    }

    fn focus_graph_node(&mut self, function: Option<FunctionId>, node: NodeId) -> bool {
        match function {
            Some(id) => {
                self.open_function_tab(id);
                if !self.ensure_function_canvas(id) {
                    return false;
                }
                let canvas = &mut self
                    .mapping_workspace
                    .function_canvases
                    .get_mut(&id)
                    .expect("ensured function canvas");
                if !canvas.focus_node(CanvasNode::Graph(node)) {
                    let Some(definition) = self.project.user_functions.get(&id) else {
                        return false;
                    };
                    let saved = CanvasLayout::capture_nodes(&canvas.snarl);
                    canvas.snarl = build_function_snarl(definition);
                    CanvasLayout::apply_nodes(&saved, &mut canvas.snarl);
                    if !canvas.focus_node(CanvasNode::Graph(node)) {
                        return false;
                    }
                }
            }
            None => {
                self.mapping_workspace.active = MappingDocument::Main;
                self.mapping_workspace.focused = MappingDocument::Main;
                if !self.main_canvas.focus_node(CanvasNode::Graph(node)) {
                    let saved = CanvasLayout::capture_nodes(&self.main_canvas.snarl);
                    self.main_canvas.snarl = build_snarl(&self.project);
                    CanvasLayout::apply_nodes(&saved, &mut self.main_canvas.snarl);
                    if !self.main_canvas.focus_node(CanvasNode::Graph(node)) {
                        return false;
                    }
                }
            }
        }
        self.show_canvas_for_navigation();
        true
    }

    fn navigate_to_destination(&mut self, destination: NavigationDestination) -> bool {
        match destination {
            NavigationDestination::GraphNode { function, node } => {
                if !self.focus_graph_node(function, node) {
                    self.status = "graph node is not visible on its canvas".to_string();
                    return false;
                }
                self.status = format!("focused graph node {node}");
            }
            NavigationDestination::UserFunction(id) => {
                self.open_function_tab(id);
                self.show_canvas_for_navigation();
                self.status = format!("opened function {}", id.get());
            }
            NavigationDestination::Schema {
                endpoint,
                visible_path,
                exact,
            } => {
                match &endpoint {
                    ValidationEndpoint::Source | ValidationEndpoint::NamedSource { .. } => {
                        self.source_schema_explorer.clear();
                        self.show_source_for_navigation();
                    }
                    ValidationEndpoint::Target => {
                        self.mapping_workspace.active = MappingDocument::Main;
                        self.mapping_workspace.focused = MappingDocument::Main;
                        self.target_schema_explorer.clear();
                        self.show_inspector_for_navigation();
                    }
                    ValidationEndpoint::NamedTarget { index, .. } => {
                        self.open_target_tab(*index);
                        self.target_schema_explorer.clear();
                        self.show_inspector_for_navigation();
                    }
                }
                self.focused_schema = Some(SchemaFocus {
                    endpoint,
                    visible_path,
                    pending_scroll: true,
                });
                self.status = if exact {
                    "focused schema item"
                } else {
                    "focused nearest visible schema item"
                }
                .to_string();
            }
            NavigationDestination::Scope {
                target,
                visible_path,
                exact,
            } => {
                match target {
                    Some(index) => self.open_target_tab(index),
                    None => {
                        self.mapping_workspace.active = MappingDocument::Main;
                        self.mapping_workspace.focused = MappingDocument::Main;
                    }
                }
                self.selected_scope = visible_path;
                self.pending_scope_scroll = true;
                self.show_inspector_for_navigation();
                self.status = if exact {
                    "selected target scope"
                } else {
                    "selected nearest visible target scope"
                }
                .to_string();
            }
            NavigationDestination::FailureRule(index) => {
                self.mapping_workspace.active = MappingDocument::Main;
                self.mapping_workspace.focused = MappingDocument::Main;
                self.selected_failure_rule = Some(index);
                self.pending_failure_rule_scroll = true;
                self.show_inspector_for_navigation();
                self.status = format!("selected failure rule {}", index + 1);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;
    use mapping::{
        DynamicChild, FailureIteration, FailureRule, FailureSelection, FormatOptions, NamedSource,
        NamedTarget, Node, ScopeSequence,
    };

    fn schema(name: &str) -> SchemaNode {
        SchemaNode::group(name, vec![SchemaNode::scalar("field", ScalarType::String)])
    }

    #[test]
    fn typed_owners_route_to_their_exact_endpoint_and_nearest_visible_scope() {
        let mut project = blank_project();
        project.source = SchemaNode::group(
            "source",
            vec![
                SchemaNode::scalar("duplicate", ScalarType::String),
                SchemaNode::scalar("duplicate", ScalarType::String),
            ],
        );
        project.extra_sources.push(NamedSource {
            name: "lookup".into(),
            path: "lookup.json".into(),
            schema: schema("lookup"),
            options: FormatOptions::default(),
            dynamic_path: None,
        });
        let mut root = Scope::default();
        root.children.push(Scope {
            target_field: "group".into(),
            ..Scope::default()
        });
        root.children[0].dynamic_children.push(DynamicChild {
            key: 0,
            scope: Scope::default(),
        });
        project.extra_targets.push(NamedTarget {
            name: "audit".into(),
            path: None,
            schema: schema("audit"),
            options: FormatOptions::default(),
            root,
        });

        assert_eq!(
            resolve_destination(
                &project,
                &ValidationOwner::SchemaNode(ValidationSchemaLocation {
                    endpoint: ValidationEndpoint::Source,
                    path: vec![ValidationSchemaStep::Child(1)],
                })
            ),
            Some(NavigationDestination::Schema {
                endpoint: ValidationEndpoint::Source,
                visible_path: vec![1],
                exact: true,
            })
        );
        assert_eq!(
            resolve_destination(
                &project,
                &ValidationOwner::Endpoint(ValidationEndpoint::NamedSource {
                    index: 0,
                    name: "lookup".into(),
                })
            ),
            Some(NavigationDestination::Schema {
                endpoint: ValidationEndpoint::NamedSource {
                    index: 0,
                    name: "lookup".into(),
                },
                visible_path: Vec::new(),
                exact: true,
            })
        );
        assert_eq!(
            resolve_destination(
                &project,
                &ValidationOwner::Scope(ValidationScopeLocation {
                    target: ValidationEndpoint::NamedTarget {
                        index: 0,
                        name: "audit".into(),
                    },
                    path: vec![
                        ValidationScopeStep::Child(0),
                        ValidationScopeStep::DynamicChild(0),
                    ],
                })
            ),
            Some(NavigationDestination::Scope {
                target: Some(0),
                visible_path: vec![0],
                exact: false,
            })
        );
        assert!(
            resolve_destination(
                &project,
                &ValidationOwner::Endpoint(ValidationEndpoint::NamedSource {
                    index: 0,
                    name: "renamed".into(),
                })
            )
            .is_none()
        );
    }

    #[test]
    fn graph_function_rule_and_concatenated_segment_routes_are_distinct() {
        let mut app = FerruleApp::default();
        let function = FunctionId::new(9);
        app.project.graph.nodes.insert(
            7,
            Node::Const {
                value: ir::Value::Int(1),
            },
        );
        let mut body = Graph::default();
        body.nodes.insert(
            7,
            Node::Const {
                value: ir::Value::Int(2),
            },
        );
        app.project.user_functions.insert(
            function,
            UserFunction {
                library: "local".into(),
                name: "sample".into(),
                description: None,
                parameters: Vec::new(),
                output_name: "result".into(),
                output_type: ScalarType::Int,
                body,
                output: 7,
            },
        );
        app.project.failure_rules.push(FailureRule {
            iteration: FailureIteration::Source {
                collection: Vec::new(),
            },
            selection: FailureSelection::All,
            message: None,
        });
        app.project.root.iteration = ScopeIteration::Concatenate(ScopeSequence::new(
            Scope::default(),
            vec![Scope::default()],
        ));

        assert_eq!(
            resolve_destination(
                &app.project,
                &ValidationOwner::GraphNode {
                    function: None,
                    node: 7
                }
            ),
            Some(NavigationDestination::GraphNode {
                function: None,
                node: 7
            })
        );
        assert_eq!(
            resolve_destination(
                &app.project,
                &ValidationOwner::GraphNode {
                    function: Some(function),
                    node: 7
                }
            ),
            Some(NavigationDestination::GraphNode {
                function: Some(function),
                node: 7
            })
        );
        assert_eq!(
            resolve_destination(&app.project, &ValidationOwner::UserFunction(function)),
            Some(NavigationDestination::UserFunction(function))
        );
        assert_eq!(
            resolve_destination(&app.project, &ValidationOwner::FailureRule { index: 0 }),
            Some(NavigationDestination::FailureRule(0))
        );
        assert_eq!(
            resolve_destination(
                &app.project,
                &ValidationOwner::Scope(ValidationScopeLocation {
                    target: ValidationEndpoint::Target,
                    path: vec![ValidationScopeStep::Segment(1)]
                })
            ),
            Some(NavigationDestination::Scope {
                target: None,
                visible_path: Vec::new(),
                exact: false
            })
        );

        assert!(
            app.navigate_to_destination(NavigationDestination::GraphNode {
                function: Some(function),
                node: 7
            })
        );
        assert_eq!(
            app.mapping_workspace.active,
            MappingDocument::Function(function)
        );
        assert!(
            app.mapping_workspace.function_canvases[&function]
                .pending_focus
                .is_some()
        );
        assert!(
            app.navigate_to_destination(NavigationDestination::GraphNode {
                function: None,
                node: 7
            })
        );
        assert_eq!(app.mapping_workspace.active, MappingDocument::Main);
        assert!(app.main_canvas.pending_focus.is_some());
        assert!(app.navigate_to_destination(NavigationDestination::FailureRule(0)));
        assert_eq!(app.selected_failure_rule, Some(0));
    }

    #[test]
    fn stale_diagnostic_is_rejected_before_it_changes_navigation() {
        let mut app = FerruleApp::default();
        app.project.graph.nodes.insert(
            42,
            Node::Call {
                function: "missing-function".into(),
                args: Vec::new(),
            },
        );
        let issue = cli::validate(&app.project)
            .into_iter()
            .find(|issue| {
                issue.owner
                    == Some(ValidationOwner::GraphNode {
                        function: None,
                        node: 42,
                    })
            })
            .expect("invalid graph node has a diagnostic");
        let diagnostic = Diagnostic::validation(&app.project, issue);
        assert!(app.navigate_to_diagnostic(&diagnostic));
        assert!(app.main_canvas.pending_focus.is_some());

        app.project.target.name.push_str("-edited");
        assert!(cli::validate(&app.project).iter().any(|issue| {
            issue.owner
                == Some(ValidationOwner::GraphNode {
                    function: None,
                    node: 42,
                })
                && issue.to_string() == diagnostic.message
        }));
        app.main_canvas.pending_focus = None;
        assert!(!app.navigate_to_diagnostic(&diagnostic));
        assert_eq!(app.main_canvas.pending_focus, None);

        app.project.graph.nodes.insert(
            42,
            Node::Const {
                value: ir::Value::String("fixed".into()),
            },
        );
        app.main_canvas.pending_focus = None;
        assert!(!app.navigate_to_diagnostic(&diagnostic));
        assert_eq!(app.main_canvas.pending_focus, None);
        assert_eq!(app.status, "diagnostic is out of date; validate again");
    }

    #[test]
    fn missing_xml_type_display_keeps_raw_issue_project_and_navigation_identity() {
        for named in [false, true] {
            let mut app = FerruleApp {
                project: crate::target_xml_type::tests::project(),
                ..Default::default()
            };
            let scope = if named {
                &mut app.project.extra_targets[0].root
            } else {
                &mut app.project.root.children[0]
            };
            let binding = scope
                .bindings
                .iter_mut()
                .find(|binding| binding.target_field == ir::XML_TYPE_FIELD)
                .unwrap();
            binding.node = 41;
            let before = mapping::project_file::encode_pretty(&app.project).unwrap();
            let raw = cli::validate(&app.project);
            let issue = raw
                .iter()
                .find(|issue| {
                    issue.message
                        == format!(
                            "binding for `{}` references missing node 41",
                            ir::XML_TYPE_FIELD
                        )
                })
                .expect("actual engine missing virtual binding issue");
            assert!(issue.to_string().contains(ir::XML_TYPE_FIELD));
            let mut unresolved = issue.clone();
            if let Some(ValidationOwner::Scope(owner)) = &mut unresolved.owner {
                owner.path.push(ValidationScopeStep::Child(999));
            }
            assert_eq!(
                crate::diagnostics::validation_display_message(&app.project, &unresolved),
                unresolved.to_string(),
                "unresolved owners are not rewritten"
            );
            let mut diagnostics = crate::diagnostics::Diagnostics::default();
            diagnostics.validation(&app.project, raw.clone());
            let display = diagnostics
                .items()
                .iter()
                .find(|diagnostic| {
                    diagnostic.location == issue.owner.clone().map(DiagnosticLocation::Validation)
                })
                .unwrap()
                .clone();
            assert_eq!(
                display.message,
                format!(
                    "{}: binding for `XML type` references missing node 41",
                    issue.location
                )
            );
            assert!(!display.message.contains(ir::XML_TYPE_FIELD));
            assert_eq!(
                display.project_fingerprint.as_deref(),
                Some(project_fingerprint(&app.project).as_str())
            );

            let context = egui::Context::default();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let _ = diagnostics.show(ui);
                },
            );
            fn texts(shape: &egui::epaint::Shape, result: &mut Vec<String>) {
                match shape {
                    egui::epaint::Shape::Text(text) => result.push(text.galley.text().to_string()),
                    egui::epaint::Shape::Vec(shapes) => {
                        for shape in shapes {
                            texts(shape, result);
                        }
                    }
                    _ => {}
                }
            }
            let mut labels = Vec::new();
            for shape in output.shapes {
                texts(&shape.shape, &mut labels);
            }
            assert!(labels.iter().any(|label| label.contains("XML type")));
            assert!(
                !labels
                    .iter()
                    .any(|label| label.contains(ir::XML_TYPE_FIELD))
            );
            assert_eq!(cli::validate(&app.project), raw);
            assert_eq!(
                mapping::project_file::encode_pretty(&app.project).unwrap(),
                before
            );

            assert!(app.navigate_to_diagnostic(&display));
            assert_eq!(
                app.mapping_workspace.active,
                if named {
                    MappingDocument::Target(0)
                } else {
                    MappingDocument::Main
                }
            );
            assert_eq!(app.selected_scope, if named { vec![] } else { vec![0] });
            assert_eq!(
                mapping::project_file::encode_pretty(&app.project).unwrap(),
                before
            );
            app.project.graph.nodes.insert(
                41,
                Node::Const {
                    value: ir::Value::String(crate::target_xml_type::tests::BASE.into()),
                },
            );
            assert!(!app.navigate_to_diagnostic(&display));
        }
    }
}
