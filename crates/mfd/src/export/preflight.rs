use mapping::{Project, Scope, ScopeConstruction};

use crate::MfdError;

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
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

fn has_conflicting_http_source_options(project: &Project) -> bool {
    project.source_options.http_get.is_some()
        && (project.source_options.lenient_segments
            || project.source_options.delimiter.is_some()
            || project.source_options.has_header_row.is_some()
            || project.source_options.fixed_width.is_some()
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
