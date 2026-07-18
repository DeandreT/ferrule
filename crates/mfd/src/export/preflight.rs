use ir::SchemaNode;
use mapping::{FormatOptions, Project, Scope, ScopeConstruction};

use crate::MfdError;

use super::schema::side_format;
use super::{concatenation, external_source, flextext, xbrl};

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    if project.extra_sources.len() > 256 {
        return Err(MfdError::Unsupported(
            "projects with more than 256 additional sources cannot be exported to .mfd".to_string(),
        ));
    }
    if project.extra_targets.len() > 256 {
        return Err(MfdError::Unsupported(
            "projects with more than 256 additional targets cannot be exported to .mfd".to_string(),
        ));
    }
    validate_target(
        project,
        &project.target,
        &project.target_path,
        &project.target_options,
        &project.root,
        false,
    )?;
    for target in &project.extra_targets {
        validate_target(
            project,
            &target.schema,
            &target.path,
            &target.options,
            &target.root,
            true,
        )?;
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
    xbrl::validate_side(
        &project.source,
        &project.source_options,
        mapping::XbrlBoundaryMode::ExternalSource,
        "source",
    )?;
    for source in &project.extra_sources {
        xbrl::validate_side(
            &source.schema,
            &source.options,
            mapping::XbrlBoundaryMode::ExternalSource,
            "additional source",
        )?;
    }
    xbrl::validate_side(
        &project.target,
        &project.target_options,
        mapping::XbrlBoundaryMode::ExternalTarget,
        "target",
    )?;
    for target in &project.extra_targets {
        xbrl::validate_side(
            &target.schema,
            &target.options,
            mapping::XbrlBoundaryMode::ExternalTarget,
            "additional target",
        )?;
    }
    external_source::validate(project)?;
    flextext::validate_side(&project.source, &project.source_options, "source")?;
    for source in &project.extra_sources {
        flextext::validate_side(&source.schema, &source.options, "additional source")?;
    }
    flextext::validate_side(&project.target, &project.target_options, "target")?;
    for target in &project.extra_targets {
        flextext::validate_side(&target.schema, &target.options, "additional target")?;
    }
    if project.source_options.protobuf.is_some()
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.protobuf.is_some())
        || project.target_options.protobuf.is_some()
        || project
            .extra_targets
            .iter()
            .any(|target| target.options.protobuf.is_some())
    {
        return Err(MfdError::Unsupported(
            "protobuf component export is not supported; remove protobuf format options before exporting this project"
                .to_string(),
        ));
    }
    if project.source_options.pdf.is_some()
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.pdf.is_some())
        || project.target_options.pdf.is_some()
        || project
            .extra_targets
            .iter()
            .any(|target| target.options.pdf.is_some())
    {
        return Err(MfdError::Unsupported(
            "PDF component export is not supported; remove PDF format options before exporting this project"
                .to_string(),
        ));
    }
    if project.target_options.http_get.is_some()
        || project
            .extra_targets
            .iter()
            .any(|target| target.options.http_get.is_some())
    {
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
    for source in &project.extra_sources {
        if source.dynamic_path.is_some() {
            let path = (!source.path.is_empty()).then_some(source.path.clone());
            if side_format(&path, &source.options) != super::schema::SideFormat::Xml
                || source.options.http_get.is_some()
                || source.options.external_source.is_some()
                || source.options.xbrl.is_some()
            {
                return Err(MfdError::Unsupported(format!(
                    "dynamic extra source `{}` is exportable only as a local XML document component",
                    source.name
                )));
            }
            let node = source.dynamic_path.as_ref().map(|dynamic| dynamic.node);
            if node.is_none_or(|node| !project.graph.nodes.contains_key(&node)) {
                return Err(MfdError::Unsupported(format!(
                    "dynamic extra source `{}` references a missing path node",
                    source.name
                )));
            }
        }
    }
    Ok(())
}

fn validate_target(
    project: &Project,
    schema: &SchemaNode,
    path: &Option<String>,
    options: &FormatOptions,
    root: &Scope,
    additional: bool,
) -> Result<(), MfdError> {
    if additional && options.lenient_segments {
        return Err(MfdError::Unsupported(
            "an additional EDI target cannot be exported because its configuration and dialect are not retained in the project"
                .to_string(),
        ));
    }
    concatenation::validate(root, schema, side_format(path, options))?;
    for (present, message) in [
        (
            has_recursive_sequence(root),
            "recursive scalar sequence export is not supported",
        ),
        (
            has_scalar_construction(root),
            "scalar scope construction export is not supported",
        ),
        (
            has_recursive_filter(root),
            "recursive-filter scope construction export is not supported",
        ),
        (
            has_path_hierarchy(root),
            "path-hierarchy scope construction export is not supported",
        ),
        (
            has_adjacency_tree(root),
            "adjacency-tree scope construction export is not supported",
        ),
    ] {
        if present {
            return Err(MfdError::Unsupported(message.to_string()));
        }
    }
    if additional && has_join(root) {
        return Err(MfdError::Unsupported(
            "inner joins owned by additional targets cannot be exported to .mfd yet".to_string(),
        ));
    }
    validate_copy_current_source(&project.source, schema, root)
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

fn has_join(scope: &Scope) -> bool {
    scope.join().is_some() || nested_scopes(scope).any(has_join)
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
    std::iter::once(&project.source_options)
        .chain(project.extra_sources.iter().map(|source| &source.options))
        .any(|options| {
            options.http_get.is_some()
                && (options.lenient_segments
                    || options.delimiter.is_some()
                    || options.has_header_row.is_some()
                    || options.fixed_width.is_some()
                    || options.external_source.is_some()
                    || options.json_lines
                    || options.xlsx_sheet.is_some()
                    || options.xlsx_start_row.is_some()
                    || !options.xlsx_columns.is_empty()
                    || !options.xlsx_rows.is_empty()
                    || options.xlsx_composite.is_some()
                    || options.xlsx_grid.is_some()
                    || options.xlsx_hierarchical.is_some())
        })
}

fn validate_copy_current_source(
    source: &SchemaNode,
    target: &SchemaNode,
    root: &Scope,
) -> Result<(), MfdError> {
    if root.construction != ScopeConstruction::CopyCurrentSource {
        if has_copy(root) {
            return Err(MfdError::Unsupported(
                "copy-current-source construction is exportable only at the document root"
                    .to_string(),
            ));
        }
        return Ok(());
    }
    if root.source().is_some()
        || root.sequence().is_some()
        || root.join().is_some()
        || root.filter.is_some()
        || root.sort_by.is_some()
        || root.take.is_some()
        || root.group_by.is_some()
        || root.group_starting_with.is_some()
        || root.group_into_blocks.is_some()
        || !root.bindings.is_empty()
        || !root.children.is_empty()
        || !root.dynamic_bindings.is_empty()
        || !root.dynamic_children.is_empty()
        || root.merge_dynamic_fields
    {
        return Err(MfdError::Unsupported(
            "copy-current-source export requires an uncontrolled document-root copy".to_string(),
        ));
    }
    if source != target {
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
        || scope
            .concatenated()
            .is_some_and(|segments| segments.iter().any(has_copy))
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
