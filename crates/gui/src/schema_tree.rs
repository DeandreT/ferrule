//! Searchable, read-only rendering of a `SchemaNode` tree.

use egui::{RichText, Ui};
use ir::{SchemaKind, SchemaNode};

/// Session-local search state for one schema explorer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaExplorerState {
    query: String,
}

impl SchemaExplorerState {
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    pub fn is_filtering(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn clear(&mut self) {
        self.query.clear();
    }

    pub fn match_count(&self, schema: &SchemaNode) -> usize {
        let query = SearchQuery::new(&self.query);
        MatchPlan::build(schema, &query, &mut Vec::new()).matches
    }
}

/// Renders a schema tree and returns whether any node was visible.
pub fn show_schema_tree(
    ui: &mut Ui,
    schema: &SchemaNode,
    state: &SchemaExplorerState,
    id_salt: impl egui::AsIdSalt,
    x12_descriptions: bool,
) -> bool {
    let query = SearchQuery::new(state.query());
    let plan = MatchPlan::build(schema, &query, &mut Vec::new());
    if state.is_filtering() && plan.matches == 0 {
        return false;
    }
    ui.push_id(id_salt, |ui| {
        show_node(ui, &plan, 0, state.is_filtering(), false, x12_descriptions)
    });
    true
}

pub fn schema_field_count(schema: &SchemaNode) -> usize {
    match &schema.kind {
        SchemaKind::Scalar { .. } => 1,
        SchemaKind::Group { children, .. } => children.iter().map(schema_field_count).sum(),
    }
}

struct SearchQuery {
    terms: Vec<String>,
}

impl SearchQuery {
    fn new(query: &str) -> Self {
        Self {
            terms: query
                .split_whitespace()
                .map(|term| term.to_lowercase())
                .collect(),
        }
    }

    fn matches(&self, node: &SchemaNode, path: &[String]) -> bool {
        if self.terms.is_empty() {
            return false;
        }
        let path = path.join("/").to_lowercase();
        let label = node_label(node).to_lowercase();
        self.terms.iter().all(|term| {
            if term.contains('/') {
                path.contains(term)
            } else {
                label.contains(term)
            }
        })
    }
}

struct MatchPlan<'a> {
    node: &'a SchemaNode,
    self_matches: bool,
    matches: usize,
    children: Vec<MatchPlan<'a>>,
}

impl<'a> MatchPlan<'a> {
    fn build(node: &'a SchemaNode, query: &SearchQuery, path: &mut Vec<String>) -> Self {
        path.push(if node.attribute {
            format!("@{}", node.name)
        } else {
            node.name.clone()
        });
        let self_matches = query.matches(node, path);
        let children = match &node.kind {
            SchemaKind::Scalar { .. } => Vec::new(),
            SchemaKind::Group { children, .. } => children
                .iter()
                .map(|child| Self::build(child, query, path))
                .collect(),
        };
        path.pop();
        let matches =
            usize::from(self_matches) + children.iter().map(|child| child.matches).sum::<usize>();
        Self {
            node,
            self_matches,
            matches,
            children,
        }
    }

    fn descendant_matches(&self) -> bool {
        self.matches > usize::from(self.self_matches)
    }
}

fn show_node(
    ui: &mut Ui,
    plan: &MatchPlan<'_>,
    depth: usize,
    filtering: bool,
    reveal_subtree: bool,
    x12_descriptions: bool,
) {
    if filtering && !reveal_subtree && plan.matches == 0 {
        return;
    }
    let label = node_label(plan.node);
    match &plan.node.kind {
        SchemaKind::Scalar { .. } => {
            ui.label(match (filtering, plan.self_matches) {
                (true, true) => RichText::new(label).strong(),
                _ => RichText::new(label),
            });
        }
        SchemaKind::Group { .. } => {
            let leaves = schema_field_count(plan.node);
            let header = match (filtering, plan.self_matches) {
                (true, true) => RichText::new(label).strong(),
                _ => RichText::new(label),
            };
            let force_open = filtering && plan.descendant_matches();
            let response = egui::CollapsingHeader::new(header)
                .id_salt((depth, plan.node.name.as_str()))
                .default_open(depth == 0 || leaves <= 12)
                .open(force_open.then_some(true))
                .show(ui, |ui| {
                    let reveal_children = reveal_subtree || plan.self_matches;
                    for child in &plan.children {
                        show_node(
                            ui,
                            child,
                            depth + 1,
                            filtering,
                            reveal_children,
                            x12_descriptions,
                        );
                    }
                });
            response.header_response.on_hover_text(node_hover_text(
                plan.node,
                leaves,
                x12_descriptions,
            ));
        }
    }
}

fn node_hover_text(node: &SchemaNode, leaves: usize, x12_descriptions: bool) -> String {
    crate::x12_tooltips::segment_description(x12_descriptions, &node.name).map_or_else(
        || format!("{leaves} scalar field(s)"),
        |description| format!("X12 {}: {description}\n{leaves} scalar field(s)", node.name),
    )
}

fn node_label(node: &SchemaNode) -> String {
    let prefix = if node.attribute { "@" } else { "" };
    let suffix = if node.repeating { " []" } else { "" };
    match &node.kind {
        SchemaKind::Scalar { ty } => format!("{prefix}{}{suffix}: {ty:?}", node.name),
        SchemaKind::Group { .. } => format!("{prefix}{}{suffix}", node.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Orders",
            vec![
                SchemaNode::scalar("CreatedAt", ScalarType::Float),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::Int).attribute(),
                        SchemaNode::scalar("CustomerName", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        )
    }

    #[test]
    fn counts_nested_scalar_fields_for_expansion_policy() {
        assert_eq!(schema_field_count(&schema()), 3);
    }

    #[test]
    fn matcher_is_case_insensitive_and_searches_paths_types_and_markers() {
        let schema = schema();
        let mut state = SchemaExplorerState::default();

        *state.query_mut() = "customer STRING".to_string();
        assert_eq!(state.match_count(&schema), 1);
        *state.query_mut() = "orders/order/@id".to_string();
        assert_eq!(state.match_count(&schema), 1);
        *state.query_mut() = "order []".to_string();
        assert_eq!(state.match_count(&schema), 1);
        *state.query_mut() = "float".to_string();
        assert_eq!(state.match_count(&schema), 1);
        *state.query_mut() = "order".to_string();
        assert_eq!(state.match_count(&schema), 2);
    }

    #[test]
    fn matching_descendants_mark_every_ancestor_for_expansion() {
        let schema = schema();
        let query = SearchQuery::new("customername");
        let plan = MatchPlan::build(&schema, &query, &mut Vec::new());

        assert_eq!(plan.matches, 1);
        assert!(plan.descendant_matches());
        assert_eq!(plan.children.len(), 2);
        assert!(plan.children[1].descendant_matches());
        assert!(plan.children[1].children[1].self_matches);
    }

    #[test]
    fn whitespace_only_queries_do_not_filter_and_clear_resets_state() {
        let mut state = SchemaExplorerState {
            query: "  \t".to_string(),
        };
        assert!(!state.is_filtering());
        state.query_mut().push_str("name");
        assert!(state.is_filtering());
        state.clear();
        assert_eq!(state, SchemaExplorerState::default());
    }

    #[test]
    fn x12_group_hover_adds_descriptions_only_when_enabled() {
        let segment =
            SchemaNode::group("BEG", vec![SchemaNode::scalar("BEG01", ScalarType::String)]);

        assert_eq!(
            node_hover_text(&segment, 1, true),
            "X12 BEG: Beginning Segment for Purchase Order\n1 scalar field(s)"
        );
        assert_eq!(node_hover_text(&segment, 1, false), "1 scalar field(s)");
    }
}
