use ir::{SchemaKind, SchemaNode};
use mapping::{Node, Project, ScopeIteration, SourceOwner};

use super::{
    ValidationIssue, ValidationOwner, ValidationSchemaLocation, ValidationSchemaStep, own_issues,
};

/// Leading text of the issue reported for a malformed nested-array wrapper frame.
pub(super) const WRAPPER_FRAME_ISSUE: &str = "JSON nested-array wrapper metadata";
/// Leading text of the issue reported for a malformed or misplaced tuple frame.
pub(super) const TUPLE_FRAME_ISSUE: &str = "JSON tuple frame metadata";

pub(super) fn validate_schema(
    root: &str,
    schema: &SchemaNode,
    owner: &ValidationSchemaLocation,
    path: &mut Vec<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let start = issues.len();
    let suffix = if path.is_empty() {
        String::new()
    } else {
        format!(" at `{}`", path.join("/"))
    };
    if path.is_empty() && !schema.json_pattern_budget_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            "JSON pattern metadata, compiled programs, or fixed-value assertions exceed the schema-wide validation budget",
        ));
    }
    if !schema.fixed_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("fixed-value metadata{suffix} requires one concrete scalar type"),
        ));
    }
    if !schema.numeric_range_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "numeric-range metadata{suffix} requires one matching concrete numeric scalar and must contain its fixed value"
            ),
        ));
    }
    if !schema.item_count_range_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("item-count metadata{suffix} requires a repeating schema node"),
        ));
    }
    if !schema.json_contains_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON contains metadata{suffix} requires a repeating schema node with canonical bounded predicate schemas and exact arbitrary-precision number handling"
            ),
        ));
    }
    if !schema.json_dependent_schemas_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON dependent-schema metadata{suffix} requires an object schema with canonical bounded predicate schemas and exact arbitrary-precision number handling"
            ),
        ));
    }
    if !schema.property_count_range_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("property-count metadata{suffix} requires a feasible object schema"),
        ));
    }
    if !schema.json_property_dependencies_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("JSON property-dependency metadata{suffix} requires a feasible object schema"),
        ));
    }
    if !schema.json_property_names_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON property-name metadata{suffix} requires a feasible object schema whose required names satisfy every assertion"
            ),
        ));
    }
    if !schema.string_length_range_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "string-length metadata{suffix} requires a non-arbitrary string-capable scalar domain and must contain its fixed value"
            ),
        ));
    }
    if !schema.json_patterns_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON pattern metadata{suffix} requires a non-arbitrary string-capable scalar domain"
            ),
        ));
    }
    if !schema.json_formats_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON format metadata{suffix} requires a non-arbitrary string or string-containing scalar domain"
            ),
        ));
    }
    if !schema.recursive_ref_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("recursive-reference metadata{suffix} requires an empty structural group"),
        ));
    }
    if !schema.value_generation_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "generated-value metadata{suffix} requires one non-repeating scalar without a fixed or default value"
            ),
        ));
    }
    if !schema.default_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "default-value metadata{suffix} requires one non-repeating scalar without a fixed or generated value"
            ),
        ));
    }
    if !schema.alternatives_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "group alternative metadata{suffix} has duplicate or unknown names, members, or required fields"
            ),
        ));
    }
    if !schema.required_fields_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "required-field metadata{suffix} has empty, duplicate, or undeclared closed-object names"
            ),
        ));
    }
    if !schema.alternative_mode_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("alternative-mode metadata{suffix} requires group alternatives"),
        ));
    }
    if !schema.xml_alternative_kind_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "XML alternative-kind metadata{suffix} requires exclusive structural alternatives"
            ),
        ));
    }
    if !schema.xml_optional_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("XML optional-occurrence metadata{suffix} requires a named singular element"),
        ));
    }
    if !schema.xml_attribute_required_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("required XML attribute-use metadata{suffix} requires one named singular scalar attribute"),
        ));
    }
    if !schema.xml_default_type_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("XML default-type metadata{suffix} requires a retained concrete declared base"),
        ));
    }
    if !schema.xml_name_alternatives_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "XML expanded-name metadata{suffix} requires one ordinary exact-name element with unique alternative namespaces"
            ),
        ));
    }
    if !schema.xml_wildcard_process_contents_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "XML wildcard process metadata{suffix} requires one namespace-aware generic element or attribute group"
            ),
        ));
    }
    if !schema.xml_wildcard_excluded_names_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "XML wildcard excluded names{suffix} require one namespace-aware generic element or attribute group whose namespace constraint allows every name"
            ),
        ));
    }
    if !schema.xml_repeating_sequences_are_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "XML repeating-sequence metadata{suffix} has duplicate, unknown, or non-repeating members"
            ),
        ));
    }
    if !schema.database_relation_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "database-relation metadata{suffix} requires a repeating table group with valid join columns"
            ),
        ));
    }
    if !schema.nullable_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("scalar nullability metadata{suffix} requires a scalar schema"),
        ));
    }
    if !schema.container_nullable_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "container-nullability metadata{suffix} requires an object or repeating schema"
            ),
        ));
    }
    if !schema.json_object_item_nullable_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "JSON object-item nullability metadata{suffix} requires a repeating object group"
            ),
        ));
    }
    if !schema.json_any_is_valid() {
        issues.push(ValidationIssue::new(
            root,
            format!("arbitrary-JSON metadata{suffix} requires one non-repeating string scalar"),
        ));
    }
    if let Err(error) = schema.check_json_array_wrapper_frame() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "{WRAPPER_FRAME_ISSUE}{suffix} requires a repeating group frame whose only child is a repeating `{}` array: {error}",
                ir::JSON_ARRAY_ITEM_FIELD
            ),
        ));
    }
    if let Err(error) = schema.check_json_tuple_frame() {
        issues.push(ValidationIssue::new(
            root,
            format!(
                "{TUPLE_FRAME_ISSUE}{suffix} requires a canonical group of `{}1`..`{}N` members and an optional repeating `{}` tail, none of them a JSON array: {error}",
                ir::JSON_TUPLE_MEMBER_PREFIX,
                ir::JSON_TUPLE_MEMBER_PREFIX,
                ir::JSON_TUPLE_REST_FIELD
            ),
        ));
    }
    own_issues(
        &mut issues[start..],
        ValidationOwner::SchemaNode(owner.clone()),
    );
    if let Some(constraints) = &schema.json_contains {
        for (index, constraint) in constraints.as_slice().iter().enumerate() {
            if let Some(predicate) = constraint.predicate().as_schema() {
                path.push(format!("<contains:{}>", index + 1));
                validate_schema(
                    root,
                    predicate,
                    &owner.descendant(ValidationSchemaStep::Contains(index)),
                    path,
                    issues,
                );
                path.pop();
            }
        }
    }
    if let Some(constraints) = &schema.json_dependent_schemas {
        for (index, constraint) in constraints.as_slice().iter().enumerate() {
            if let Some(predicate) = constraint.predicate().as_schema() {
                path.push(format!(
                    "<dependent-schema:{}:{}>",
                    index + 1,
                    constraint.trigger()
                ));
                validate_schema(
                    root,
                    predicate,
                    &owner.descendant(ValidationSchemaStep::DependentSchema(index)),
                    path,
                    issues,
                );
                path.pop();
            }
        }
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    for (index, child) in children.iter().enumerate() {
        path.push(child.name.clone());
        validate_schema(
            root,
            child,
            &owner.descendant(ValidationSchemaStep::Child(index)),
            path,
            issues,
        );
        path.pop();
    }
    if let Some(dynamic) = schema.dynamic_fields() {
        path.push("*".to_string());
        validate_schema(
            root,
            dynamic,
            &owner.descendant(ValidationSchemaStep::DynamicField),
            path,
            issues,
        );
        path.pop();
    }
}

pub(super) fn validate_owned_source_node(
    project: &Project,
    node: &Node,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let (owner, scalar_path, collection) = match node {
        Node::OwnedSourceField(field) => (
            &field.source.owner,
            Some(field.source.path.as_slice()),
            field.frame.as_deref(),
        ),
        Node::OwnedPosition(position) => (&position.owner, None, position.collection.as_deref()),
        Node::OwnedRowDocumentPath(path) => (&path.owner, None, path.collection.as_deref()),
        Node::OwnedSourceDocumentPath(path) => (&path.owner, None, None),
        Node::OwnedXmlSerialize(serializer) => {
            // Document-level checks are independent of the owner's schema.
            if serializer.schema.repeating {
                issues.push(ValidationIssue::new(
                    location,
                    "XML serializer schema must describe one document element",
                ));
            }
            if serializer.namespace.as_ref().is_some_and(String::is_empty) {
                issues.push(ValidationIssue::new(
                    location,
                    "XML serializer namespace cannot be empty",
                ));
            }
            (&serializer.source.owner, None, serializer.frame.as_deref())
        }
        _ => return,
    };
    let Some(root) = owned_source_root(project, owner, location, issues) else {
        return;
    };
    if let Node::OwnedXmlSerialize(serializer) = node {
        // Only the exact owner's schema may supply the serialized element.
        let expected_group = matches!(serializer.schema.kind, SchemaKind::Group { .. });
        if follow_schema_resolved(root, &serializer.source.path).is_none_or(|source| {
            source.name != serializer.schema.name
                || matches!(source.kind, SchemaKind::Group { .. }) != expected_group
        }) {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned XML serializer source `{}` does not match its `{}` schema in its owner",
                    serializer.source.path.join("/"),
                    serializer.schema.name
                ),
            ));
        }
    }
    if let Some(path) = scalar_path
        && follow_schema_resolved(root, path).is_none_or(|field| !field.is_scalar())
    {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "owned source path `{}` matches no scalar in its owner",
                path.join("/")
            ),
        ));
    }
    if let Some(collection) = collection {
        // Explicit root [] preserves flat CSV/XLSX group-root conventions.
        // Nonempty selectors identify a real collection, including repeated scalars.
        match follow_schema(root, collection) {
            None => issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned collection `{}` matches no path in its owner",
                    collection.join("/")
                ),
            )),
            Some(field) if !collection.is_empty() && !field.repeating => {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "owned collection `{}` is not a repeating path in its owner",
                        collection.join("/")
                    ),
                ))
            }
            _ => {}
        }
    }
}

/// The object must be an open scalar object in its declared owner; an explicit
/// frame must prefix it and be the endpoint root or a repeating owner path.
pub(super) fn validate_owned_dynamic_field(
    project: &Project,
    field: &mapping::OwnedDynamicSourceField,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(root) = owned_source_root(project, &field.object.owner, location, issues) else {
        return;
    };
    let path = &field.object.path;
    if follow_schema_resolved(root, path).is_none_or(|object| {
        object
            .dynamic_fields()
            .is_none_or(|dynamic| dynamic.repeating || !dynamic.is_scalar())
    }) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "owned dynamic object `{}` matches no open scalar object in its owner",
                path.join("/")
            ),
        ));
    }
    let Some(frame) = &field.frame else {
        return;
    };
    if !path.starts_with(frame) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "owned frame `{}` does not prefix dynamic object `{}`",
                frame.join("/"),
                path.join("/")
            ),
        ));
    } else if !frame.is_empty() && follow_schema(root, frame).is_none_or(|node| !node.repeating) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "owned collection `{}` is not a repeating path in its owner",
                frame.join("/")
            ),
        ));
    }
}

pub(super) fn current_source_schema<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    iteration: &ScopeIteration,
) -> Option<&'a SchemaNode> {
    match iteration {
        // Optional presence adds no frame; its group stays in the parent item.
        ScopeIteration::None | ScopeIteration::OptionalPresence { .. } => parent,
        ScopeIteration::OwnedSource { source, .. } => {
            let root =
                owned_source_root(project, &source.owner, "owned collection", &mut Vec::new())?;
            follow_schema_resolved(root, &source.path)
        }
        ScopeIteration::Source(path) | ScopeIteration::DynamicDocuments { source: path, .. } => {
            source_schema_at(project, parent, path)
        }
        ScopeIteration::Sequence(_)
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::OwnedInnerJoin { .. }
        | ScopeIteration::Concatenate(_) => None,
    }
}

pub(super) fn source_schema_at<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    path: &[String],
) -> Option<&'a SchemaNode> {
    if let Some(node) = parent.and_then(|schema| follow_schema(schema, path)) {
        return Some(node);
    }
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && let Some(node) = follow_schema(&extra.schema, rest)
    {
        return Some(node);
    }
    find_schema_path(&project.source, path).or_else(|| {
        project
            .extra_sources
            .iter()
            .find_map(|source| find_schema_path(&source.schema, path))
    })
}

fn find_schema_path<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    find_schema_path_from(schema, schema, path)
}

fn find_schema_path_from<'a>(
    root: &'a SchemaNode,
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    follow_schema_from(root, schema, path).or_else(|| match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .find_map(|child| find_schema_path_from(root, child, path)),
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
    })
}

pub(super) fn source_path_matches(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema(&extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }

    any_schema_path(&project.source, path, predicate)
        || project
            .extra_sources
            .iter()
            .any(|source| any_schema_path(&source.schema, path, predicate))
}

/// Like [`source_path_matches`], but only matches whose intermediate path
/// segments are non-repeating. A path crossing repetition selects one terminal
/// per repeated item rather than one value.
pub(super) fn source_path_matches_direct(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema_direct_from(&extra.schema, &extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }
    any_schema_path_direct_from(&project.source, &project.source, path, predicate)
        || project.extra_sources.iter().any(|source| {
            any_schema_path_direct_from(&source.schema, &source.schema, path, predicate)
        })
}

/// Whether `path` reaches a repeating node, its terminal included, from any
/// group it fully resolves from, or from a named source it names. An unframed
/// read resolves from whichever context item holds its first segment and reads
/// the first item where it crosses repetition, so a path that is direct from
/// one anchor can still cross from the anchor the engine uses.
pub(super) fn source_path_crosses_repetition(project: &Project, path: &[String]) -> bool {
    fn crosses_from(root: &SchemaNode, schema: &SchemaNode, path: &[String]) -> bool {
        let mut current = schema;
        let mut crossed = false;
        for segment in path {
            if let Some(anchor) = &current.recursive_ref {
                let Some(group) = find_concrete_schema_group(root, anchor) else {
                    return false;
                };
                current = group;
            }
            let Some(child) = current.child(segment) else {
                return false;
            };
            current = child;
            crossed |= current.repeating;
        }
        crossed
    }
    fn any_anchor(root: &SchemaNode, schema: &SchemaNode, path: &[String]) -> bool {
        crosses_from(root, schema, path)
            || match &schema.kind {
                SchemaKind::Group { children, .. } => {
                    children.iter().any(|child| any_anchor(root, child, path))
                }
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
            }
    }
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && crosses_from(&extra.schema, &extra.schema, rest)
    {
        return true;
    }
    any_anchor(&project.source, &project.source, path)
        || project
            .extra_sources
            .iter()
            .any(|source| any_anchor(&source.schema, &source.schema, path))
}

/// Resolves a root-context path without crossing repetition. The primary root
/// owns paths whose first field it declares, matching runtime frame selection;
/// other paths need an explicit named-source prefix.
pub(super) fn root_source_path_direct<'a>(
    project: &'a Project,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let primary = &project.source;
    if path.is_empty() || follow_schema_direct_from(primary, primary, &path[..1]).is_some() {
        return follow_schema_direct_from(primary, primary, path);
    }
    let (name, rest) = path.split_first()?;
    let extra = project
        .extra_sources
        .iter()
        .find(|source| source.name == *name)?;
    follow_schema_direct_from(&extra.schema, &extra.schema, rest)
}

fn any_schema_path_direct_from(
    root: &SchemaNode,
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if follow_schema_direct_from(root, schema, path).is_some_and(predicate) {
        return true;
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .any(|child| any_schema_path_direct_from(root, child, path, predicate)),
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
    }
}

fn follow_schema_direct_from<'a>(
    root: &'a SchemaNode,
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = schema;
    for (index, segment) in path.iter().enumerate() {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_schema_group(root, anchor)?;
        }
        current = current.child(segment)?;
        if current.repeating && index + 1 != path.len() {
            return None;
        }
    }
    Some(current)
}

pub(super) fn source_path_matches_resolved(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema_resolved(&extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }
    any_schema_path_resolved(&project.source, path, predicate)
        || project
            .extra_sources
            .iter()
            .any(|source| any_schema_path_resolved(&source.schema, path, predicate))
}

fn any_schema_path_resolved(
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    fn visit(
        root: &SchemaNode,
        schema: &SchemaNode,
        path: &[String],
        predicate: impl Fn(&SchemaNode) -> bool + Copy,
    ) -> bool {
        if follow_schema_from(root, schema, path)
            .and_then(|node| {
                node.recursive_ref
                    .as_deref()
                    .and_then(|anchor| find_concrete_schema_group(root, anchor))
                    .or(Some(node))
            })
            .is_some_and(predicate)
        {
            return true;
        }
        match &schema.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .any(|child| visit(root, child, path, predicate)),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
        }
    }
    visit(schema, schema, path, predicate)
}

pub(super) fn follow_schema_resolved<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let node = follow_schema(schema, path)?;
    node.recursive_ref
        .as_deref()
        .and_then(|anchor| find_concrete_schema_group(schema, anchor))
        .or(Some(node))
}

/// SourceField paths are relative to the current scope frame, so a valid
/// path may start at any group in the source tree rather than only its root.
fn any_schema_path(
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    any_schema_path_from(schema, schema, path, predicate)
}

fn any_schema_path_from(
    root: &SchemaNode,
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if follow_schema_from(root, schema, path).is_some_and(predicate) {
        return true;
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .any(|child| any_schema_path_from(root, child, path, predicate)),
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
    }
}

pub(super) fn follow_schema<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    follow_schema_from(schema, schema, path)
}

fn follow_schema_from<'a>(
    root: &'a SchemaNode,
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = schema;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_schema_group(root, anchor)?;
        }
        current = current.child(segment)?;
    }
    Some(current)
}

fn find_concrete_schema_group<'a>(schema: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if schema.recursive_ref.is_none()
        && schema.name == anchor
        && matches!(schema.kind, SchemaKind::Group { .. })
    {
        return Some(schema);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

pub(super) fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
    }
}

pub(super) fn owned_source_root<'a>(
    project: &'a Project,
    owner: &SourceOwner,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<&'a SchemaNode> {
    let root = match owner {
        SourceOwner::Primary => &project.source,
        SourceOwner::Named { name } => {
            if name.is_empty() {
                issues.push(ValidationIssue::new(
                    location,
                    "named source owner cannot be empty",
                ));
                return None;
            }
            let mut matches = project
                .extra_sources
                .iter()
                .filter(|source| source.name == *name);
            let Some(source) = matches.next() else {
                issues.push(ValidationIssue::new(
                    location,
                    format!("unknown source owner `{name}`"),
                ));
                return None;
            };
            if matches.next().is_some() {
                issues.push(ValidationIssue::new(
                    location,
                    format!("source owner `{name}` is ambiguous"),
                ));
                return None;
            }
            &source.schema
        }
    };
    Some(root)
}

/// An owned lookup key or value names one scalar inside each selected item.
/// It never crosses a repetition below the item, so the owned read cannot
/// meet an uniterated collection at run time.
pub(super) fn validate_owned_item_scalar(
    project: &Project,
    collection: &mapping::OwnedSourceRef,
    path: &[String],
    label: &str,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(root) = owned_source_root(project, &collection.owner, location, &mut Vec::new())
    else {
        return;
    };
    let Some(mut current) = follow_schema(root, &collection.path) else {
        return;
    };
    let mut valid = true;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            match find_concrete_schema_group(root, anchor) {
                Some(group) => current = group,
                None => {
                    valid = false;
                    break;
                }
            }
        }
        match current.child(segment) {
            Some(child) if !child.repeating => current = child,
            _ => {
                valid = false;
                break;
            }
        }
    }
    if !valid || !current.is_scalar() {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "{label} `{}` is not a nonrepeating scalar under owned collection `{}`",
                path.join("/"),
                collection.path.join("/")
            ),
        ));
    }
}

/// An owned aggregate value path names one scalar inside each selected item.
/// Count never reads it, so its empty path also counts group items.
pub(super) fn validate_owned_aggregate_value(
    project: &Project,
    aggregate: &mapping::OwnedAggregate,
    value: &[String],
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if aggregate.function == mapping::AggregateOp::Count && value.is_empty() {
        return;
    }
    validate_owned_item_scalar(
        project,
        &aggregate.collection,
        value,
        "owned aggregate value",
        location,
        issues,
    );
}

pub(super) fn validate_owned_collection(
    project: &Project,
    source: &mapping::OwnedSourceRef,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(root) = owned_source_root(project, &source.owner, location, issues) else {
        return;
    };
    match follow_schema(root, &source.path) {
        None => issues.push(ValidationIssue::new(
            location,
            format!(
                "owned collection `{}` matches no path in its owner",
                source.path.join("/")
            ),
        )),
        Some(field) if !source.path.is_empty() && !field.repeating => {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned collection `{}` is not a repeating path in its owner",
                    source.path.join("/")
                ),
            ));
        }
        _ => {}
    }
}
