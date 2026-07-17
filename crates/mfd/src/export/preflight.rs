use mapping::{Project, Scope, ScopeConstruction};

use crate::MfdError;

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    if !project.extra_targets.is_empty() {
        return Err(MfdError::Unsupported(
            "projects with additional targets cannot be exported to .mfd yet".to_string(),
        ));
    }
    if has_concatenated_scope(&project.root) {
        return Err(MfdError::Unsupported(
            "concatenated target scope export is not supported".to_string(),
        ));
    }
    if has_recursive_sequence(&project.root) {
        return Err(MfdError::Unsupported(
            "recursive scalar sequence export is not supported".to_string(),
        ));
    }
    if has_scalar_construction(&project.root) {
        return Err(MfdError::Unsupported(
            "scalar scope construction export is not supported".to_string(),
        ));
    }
    if has_recursive_filter(&project.root) {
        return Err(MfdError::Unsupported(
            "recursive-filter scope construction export is not supported".to_string(),
        ));
    }
    if has_path_hierarchy(&project.root) {
        return Err(MfdError::Unsupported(
            "path-hierarchy scope construction export is not supported".to_string(),
        ));
    }
    if has_adjacency_tree(&project.root) {
        return Err(MfdError::Unsupported(
            "adjacency-tree scope construction export is not supported".to_string(),
        ));
    }
    if project
        .graph
        .nodes
        .values()
        .any(|node| matches!(node, mapping::Node::DynamicSourceField { .. }))
    {
        return Err(MfdError::Unsupported(
            "runtime-named JSON source field export is not supported".to_string(),
        ));
    }
    if project.source_options.xbrl.is_some()
        || project.target_options.xbrl.is_some()
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.xbrl.is_some())
    {
        return Err(MfdError::Unsupported(
            "XBRL boundary export is not supported; remove XBRL format options before exporting this project"
                .to_string(),
        ));
    }
    if project.source_options.external_source.is_some()
        || project.target_options.external_source.is_some()
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.external_source.is_some())
        || project
            .extra_targets
            .iter()
            .any(|target| target.options.external_source.is_some())
    {
        return Err(MfdError::Unsupported(
            "captured external-response boundaries cannot be exported to .mfd".to_string(),
        ));
    }
    if project.source_options.flextext.is_some() || project.target_options.flextext.is_some() {
        return Err(MfdError::Unsupported(
            "FlexText component export is not supported; remove FlexText format options before exporting this project"
                .to_string(),
        ));
    }
    if project.source_options.protobuf.is_some() || project.target_options.protobuf.is_some() {
        return Err(MfdError::Unsupported(
            "protobuf component export is not supported; remove protobuf format options before exporting this project"
                .to_string(),
        ));
    }
    if project.source_options.pdf.is_some() || project.target_options.pdf.is_some() {
        return Err(MfdError::Unsupported(
            "PDF component export is not supported; remove PDF format options before exporting this project"
                .to_string(),
        ));
    }
    if project.target_options.http_get.is_some() {
        return Err(MfdError::Unsupported(
            "HTTP GET transport is valid only for mapping sources".to_string(),
        ));
    }
    if has_conflicting_http_source_options(project) {
        return Err(MfdError::Unsupported(
            "HTTP GET XML sources cannot combine transport metadata with another format's options"
                .to_string(),
        ));
    }
    validate_copy_current_source(project)
}

fn has_concatenated_scope(scope: &Scope) -> bool {
    scope.concatenated().is_some()
        || scope.children.iter().any(has_concatenated_scope)
        || scope
            .dynamic_children
            .iter()
            .any(|child| has_concatenated_scope(&child.scope))
}

fn has_recursive_sequence(scope: &Scope) -> bool {
    matches!(
        scope.sequence(),
        Some(mapping::SequenceExpr::RecursiveCollect { .. })
    ) || nested_scopes(scope).any(has_recursive_sequence)
}

fn has_scalar_construction(scope: &Scope) -> bool {
    matches!(&scope.construction, ScopeConstruction::Scalar { .. })
        || nested_scopes(scope).any(has_scalar_construction)
}
fn has_recursive_filter(scope: &Scope) -> bool {
    matches!(
        &scope.construction,
        ScopeConstruction::RecursiveFilter { .. }
    ) || nested_scopes(scope).any(has_recursive_filter)
}

fn has_path_hierarchy(scope: &Scope) -> bool {
    matches!(&scope.construction, ScopeConstruction::PathHierarchy { .. })
        || nested_scopes(scope).any(has_path_hierarchy)
}

fn has_adjacency_tree(scope: &Scope) -> bool {
    matches!(&scope.construction, ScopeConstruction::AdjacencyTree { .. })
        || nested_scopes(scope).any(has_adjacency_tree)
}

fn nested_scopes(scope: &Scope) -> impl Iterator<Item = &Scope> {
    scope
        .children
        .iter()
        .chain(scope.dynamic_children.iter().map(|child| &child.scope))
        .chain(
            scope
                .concatenated()
                .into_iter()
                .flat_map(|segments| segments.iter()),
        )
}

fn has_conflicting_http_source_options(project: &Project) -> bool {
    project.source_options.http_get.is_some()
        && (project.source_options.lenient_segments
            || project.source_options.delimiter.is_some()
            || project.source_options.has_header_row.is_some()
            || project.source_options.fixed_width.is_some()
            || project.source_options.external_source.is_some()
            || project.source_options.json_lines
            || project.source_options.xlsx_sheet.is_some()
            || project.source_options.xlsx_start_row.is_some()
            || !project.source_options.xlsx_columns.is_empty()
            || !project.source_options.xlsx_rows.is_empty()
            || project.source_options.xlsx_composite.is_some()
            || project.source_options.xlsx_grid.is_some()
            || project.source_options.xlsx_hierarchical.is_some())
}

fn validate_copy_current_source(project: &Project) -> Result<(), MfdError> {
    if project.root.construction != ScopeConstruction::CopyCurrentSource {
        if has_copy(&project.root) {
            return Err(MfdError::Unsupported(
                "copy-current-source construction is exportable only at the document root"
                    .to_string(),
            ));
        }
        return Ok(());
    }
    if project.root.source().is_some()
        || project.root.sequence().is_some()
        || project.root.join().is_some()
        || project.root.filter.is_some()
        || project.root.sort_by.is_some()
        || project.root.take.is_some()
        || project.root.group_by.is_some()
        || project.root.group_starting_with.is_some()
        || project.root.group_into_blocks.is_some()
        || !project.root.bindings.is_empty()
        || !project.root.children.is_empty()
        || !project.root.dynamic_bindings.is_empty()
        || !project.root.dynamic_children.is_empty()
        || project.root.merge_dynamic_fields
    {
        return Err(MfdError::Unsupported(
            "copy-current-source export requires an uncontrolled document-root copy".to_string(),
        ));
    }
    if project.source != project.target {
        return Err(MfdError::Unsupported(
            "copy-current-source export requires identical source and target root schemas"
                .to_string(),
        ));
    }
    Ok(())
}

fn has_copy(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::CopyCurrentSource
        || scope.children.iter().any(has_copy)
        || scope
            .dynamic_children
            .iter()
            .any(|child| has_copy(&child.scope))
}

#[cfg(test)]
mod tests {
    use mapping::{AdjacencyTreePlan, Scope, ScopeConstruction};

    use super::has_adjacency_tree;

    #[test]
    fn detects_nested_adjacency_tree_construction() {
        let mut root = Scope::default();
        root.children.push(Scope {
            target_field: "tree".into(),
            construction: ScopeConstruction::AdjacencyTree {
                plan: AdjacencyTreePlan::new(
                    vec!["row".into()],
                    vec!["name".into()],
                    vec!["base".into()],
                    "name".into(),
                    "children".into(),
                    None,
                )
                .unwrap(),
            },
            ..Scope::default()
        });

        assert!(has_adjacency_tree(&root));
    }
}
