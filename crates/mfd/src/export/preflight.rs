use ir::{SchemaKind, SchemaNode};
use mapping::{ExternalPayloadFormat, FormatOptions, Project, Scope, ScopeConstruction};

use crate::MfdError;

use super::schema::side_format;
use super::{concatenation, edi, external_source, flextext, pdf, protobuf, recursive, xbrl};

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
    validate_xml_identity(&project.source_options, "source", true)?;
    for source in &project.extra_sources {
        validate_xml_identity(&source.options, "additional source", true)?;
    }
    validate_xml_identity(&project.target_options, "target", false)?;
    for target in &project.extra_targets {
        validate_xml_identity(&target.options, "additional target", false)?;
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
    edi::validate_side(&project.source, &project.source_options, "source")?;
    for source in &project.extra_sources {
        edi::validate_side(&source.schema, &source.options, "additional source")?;
    }
    edi::validate_side(&project.target, &project.target_options, "target")?;
    for target in &project.extra_targets {
        edi::validate_side(&target.schema, &target.options, "additional target")?;
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
    pdf::validate_side(
        &project.source,
        &project.source_options,
        super::schema::Side::Source,
        "source",
    )?;
    for source in &project.extra_sources {
        pdf::validate_side(
            &source.schema,
            &source.options,
            super::schema::Side::Source,
            "additional source",
        )?;
    }
    pdf::validate_side(
        &project.target,
        &project.target_options,
        super::schema::Side::Target,
        "target",
    )?;
    for target in &project.extra_targets {
        pdf::validate_side(
            &target.schema,
            &target.options,
            super::schema::Side::Target,
            "additional target",
        )?;
    }
    if project.source_options.protobuf.is_some()
        || project
            .extra_sources
            .iter()
            .any(|source| source.options.protobuf.is_some())
    {
        return Err(MfdError::Unsupported(
            "protobuf source component export is not supported; protobuf is an output-only format"
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

fn validate_xml_identity(
    options: &FormatOptions,
    side_name: &str,
    source: bool,
) -> Result<(), MfdError> {
    if options.local_xml_file_set && (!options.xml_document || !source) {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} local XML file set is valid only on an XML source boundary"
        )));
    }
    if !options.xml_document {
        return Ok(());
    }
    let external_xml = options
        .external_source
        .as_ref()
        .is_some_and(|boundary| boundary.payload() == ExternalPayloadFormat::Xml);
    let transport_conflict = options.http_get.is_some() && options.external_source.is_some();
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.json_document
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
        || (options.local_xml_file_set
            && (options.http_get.is_some() || options.external_source.is_some()))
        || (options.external_source.is_some() && (!source || !external_xml))
        || transport_conflict
    {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} XML boundary conflicts with another format's options"
        )));
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
    protobuf::validate_target(schema, options)?;
    if additional && options.lenient_segments && options.edi_kind.is_none() {
        return Err(MfdError::Unsupported(
            "an additional EDI target cannot be exported because its dialect marker is missing"
                .to_string(),
        ));
    }
    concatenation::validate(root, schema, &project.graph, side_format(path, options))?;
    recursive::validate_target(project, schema, root)?;
    if additional && has_join(root) {
        return Err(MfdError::Unsupported(
            "inner joins owned by additional targets cannot be exported to .mfd yet".to_string(),
        ));
    }
    validate_copy_current_source(project, schema, root)
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
                    || options.local_xml_file_set
                    || options.json_document
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
    project: &Project,
    target: &SchemaNode,
    root: &Scope,
) -> Result<(), MfdError> {
    validate_copy_scope(
        project,
        root,
        Some(&project.source),
        Some(target),
        &mut Vec::new(),
    )
}

fn validate_copy_scope(
    project: &Project,
    scope: &Scope,
    parent_source: Option<&SchemaNode>,
    target: Option<&SchemaNode>,
    path: &mut Vec<String>,
) -> Result<(), MfdError> {
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            validate_copy_scope(project, segment, parent_source, target, path)?;
        }
        return Ok(());
    }

    let current_source = scope_source_schema(project, parent_source, scope);
    if scope.construction == ScopeConstruction::CopyCurrentSource {
        validate_copy_scope_shape(scope, current_source, target, path)?;
    }

    for child in &scope.children {
        path.push(child.target_field.clone());
        validate_copy_scope(
            project,
            child,
            current_source,
            target.and_then(|node| node.child(&child.target_field)),
            path,
        )?;
        path.pop();
    }
    for child in &scope.dynamic_children {
        path.push("*".to_string());
        validate_copy_scope(
            project,
            &child.scope,
            current_source,
            target.and_then(SchemaNode::dynamic_fields),
            path,
        )?;
        path.pop();
    }
    Ok(())
}

fn validate_copy_scope_shape(
    scope: &Scope,
    source: Option<&SchemaNode>,
    target: Option<&SchemaNode>,
    path: &[String],
) -> Result<(), MfdError> {
    if path.is_empty() && scope.source().is_some() {
        return Err(MfdError::Unsupported(
            "copy-current-source export requires an uncontrolled document-root copy".to_string(),
        ));
    }
    if scope.sequence().is_some()
        || scope.join().is_some()
        || scope.filter.is_some()
        || scope.sort_by.is_some()
        || scope.take.is_some()
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_into_blocks.is_some()
        || !scope.bindings.is_empty()
        || !scope.children.is_empty()
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
    {
        return Err(MfdError::Unsupported(format!(
            "copy-current-source scope `{}` requires a plain source group with no controls, bindings, or children",
            display_scope_path(path)
        )));
    }
    let (Some(source), Some(target)) = (source, target) else {
        return Err(MfdError::Unsupported(format!(
            "copy-current-source scope `{}` does not resolve to exact source and target groups",
            display_scope_path(path)
        )));
    };
    if !matches!(source.kind, SchemaKind::Group { .. })
        || !matches!(target.kind, SchemaKind::Group { .. })
        || if path.is_empty() {
            source != target
        } else {
            source.kind != target.kind
        }
    {
        return Err(MfdError::Unsupported(format!(
            "copy-current-source scope `{}` requires matching source and target group fields",
            display_scope_path(path)
        )));
    }
    Ok(())
}

fn scope_source_schema<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    scope: &Scope,
) -> Option<&'a SchemaNode> {
    let Some(path) = scope.source() else {
        return if scope.sequence().is_none() && scope.join().is_none() {
            parent
        } else {
            None
        };
    };
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
    {
        return follow_schema(&extra.schema, rest);
    }
    parent.and_then(|schema| follow_schema(schema, path))
}

fn follow_schema<'a>(mut schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for segment in path {
        schema = schema.child(segment)?;
    }
    Some(schema)
}

fn display_scope_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".to_string()
    } else {
        path.join("/")
    }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};
    use mapping::{Scope, ScopeConstruction};

    use super::validate_copy_scope_shape;

    #[test]
    fn nested_copy_rejects_mismatched_group_fields() {
        let source = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        let target = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Other", ScalarType::String)],
        );
        let scope = Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        };

        let result = validate_copy_scope_shape(
            &scope,
            Some(&source),
            Some(&target),
            &["Rows".into(), "Item".into()],
        );
        assert!(matches!(
            result,
            Err(crate::MfdError::Unsupported(message))
                if message.contains("matching source and target group fields")
        ));
    }

    #[test]
    fn nested_copy_rejects_scope_controls() {
        let item = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        let scope = Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            filter: Some(7),
            ..Scope::default()
        };

        let result = validate_copy_scope_shape(
            &scope,
            Some(&item),
            Some(&item),
            &["Rows".into(), "Item".into()],
        );
        assert!(matches!(
            result,
            Err(crate::MfdError::Unsupported(message))
                if message.contains("no controls, bindings, or children")
        ));
    }
}
