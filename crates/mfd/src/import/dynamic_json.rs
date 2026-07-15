use std::collections::BTreeSet;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{DynamicBinding, DynamicChild, IterationOutput, NodeId, Scope};

use super::graph::GraphBuilder;
use super::schema::SchemaComponent;
use super::scope::{IterationNodes, ScopeBuilder};
use super::source::SourcePath;

#[derive(Clone)]
pub(super) struct DynamicJsonTarget {
    root: Option<RootDynamicJsonTarget>,
    nested: Vec<DynamicObjectTarget>,
    claimed_sequences: BTreeSet<usize>,
}

#[derive(Clone)]
struct RootDynamicJsonTarget {
    pub(super) property_input: u32,
    pub(super) property_key_input: u32,
    pub(super) item_input: u32,
    pub(super) fields: Vec<DynamicScalarTarget>,
    value_type: ScalarType,
}

#[derive(Clone)]
struct DynamicObjectTarget {
    owner_path: Vec<String>,
    property_input: u32,
    key_input: u32,
    value_input: u32,
    value_type: ScalarType,
}

#[derive(Clone)]
pub(super) struct DynamicScalarTarget {
    pub(super) key_input: u32,
    pub(super) value_input: u32,
}

pub(super) fn prepare_target(
    component: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
) -> Option<DynamicJsonTarget> {
    component
        .dynamic_json
        .clone()
        .and_then(|mut dynamic| match dynamic.validate_frames(builder) {
            Ok(()) => {
                dynamic.claim_sequence_scopes(builder);
                Some(dynamic)
            }
            Err(reason) => {
                builder.warnings.push(format!(
                    "dynamic JSON target `{}` is unsupported: {reason}",
                    component.name
                ));
                None
            }
        })
}

pub(super) fn build_target(
    dynamic: Option<DynamicJsonTarget>,
    component: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if let Some(dynamic) = dynamic
        && let Err(reason) = dynamic.build(builder, scopes)
    {
        builder.warnings.push(format!(
            "dynamic JSON target `{}` is unsupported: {reason}",
            component.name
        ));
    }
}

impl DynamicJsonTarget {
    pub(super) fn input_count(&self) -> usize {
        self.root
            .as_ref()
            .map_or(0, RootDynamicJsonTarget::input_count)
            + self.nested.len() * 3
    }

    pub(super) fn attach_schema(&self, schema: &mut SchemaNode) -> bool {
        if self
            .root
            .as_ref()
            .is_some_and(|root| !root.attach_schema(schema))
        {
            return false;
        }
        for site in &self.nested {
            let Some(owner) = schema_node_at_mut(schema, &site.owner_path) else {
                return false;
            };
            if let Some(existing) = owner.dynamic_fields() {
                if !matches!(
                    &existing.kind,
                    SchemaKind::Scalar { ty } if *ty == site.value_type
                ) || existing.repeating
                {
                    return false;
                }
            } else if !owner.set_dynamic_fields(Some(SchemaNode::scalar("*", site.value_type))) {
                return false;
            }
        }
        true
    }

    fn frame_sources(&self, builder: &GraphBuilder<'_>) -> Result<Vec<SourcePath>, String> {
        let mut sources = Vec::new();
        if let Some(root) = &self.root {
            sources.extend(root.frame_sources(builder)?);
        }
        for site in &self.nested {
            if let Some(source) = site.iteration_source(builder)? {
                sources.push(source);
            }
        }
        Ok(sources)
    }

    fn validate_frames(&self, builder: &GraphBuilder<'_>) -> Result<(), String> {
        self.frame_sources(builder).map(|_| ())
    }

    fn claim_sequence_scopes(&mut self, builder: &mut GraphBuilder<'_>) {
        for site in &self.nested {
            let Some(feed) = builder.edge_from.get(&site.property_input).copied() else {
                continue;
            };
            if let Some(index) = builder.resolve_iteration_feed(feed).sequence_component
                && builder.sequence_scope_components.insert(index)
            {
                self.claimed_sequences.insert(index);
            }
        }
    }

    pub(super) fn build(
        &self,
        builder: &mut GraphBuilder<'_>,
        scopes: &mut ScopeBuilder,
    ) -> Result<(), String> {
        let frame_sources = self.frame_sources(builder)?;
        let previous_graph = builder.graph.clone();
        let previous_next_id = builder.next_id;
        let previous_fn_nodes = builder.fn_nodes.clone();
        let previous_sequence_items = builder.sequence_items.clone();
        let previous_sequence_scope_components = builder.sequence_scope_components.clone();
        let previous_sequence_predicate_components = builder.sequence_predicate_components.clone();
        let previous_framed = builder.framed.clone();
        let previous_source_fields = builder.source_fields.clone();
        let previous_query_scope_sources = builder.query_scope_sources.clone();
        let previous_warned_unscoped_queries = builder.warned_unscoped_queries.clone();
        let previous_udf_nodes = builder.udf_nodes.clone();
        let previous_warnings = builder.warnings.clone();
        let previous_warned_sequence_uses = builder.warned_sequence_uses.clone();
        let previous_warned_scalar_filters = builder.warned_scalar_filters.clone();
        for source in &frame_sources {
            builder.note_framed_prefixes(source);
        }
        let mut candidate = scopes.clone();
        let result = (|| {
            if let Some(root) = &self.root {
                root.build(builder, &mut candidate.root)?;
            }
            for site in &self.nested {
                site.build(builder, &mut candidate)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                *scopes = candidate;
                Ok(())
            }
            Err(reason) => {
                builder.graph = previous_graph;
                builder.next_id = previous_next_id;
                builder.fn_nodes = previous_fn_nodes;
                builder.sequence_items = previous_sequence_items;
                builder.sequence_scope_components = previous_sequence_scope_components;
                builder.sequence_predicate_components = previous_sequence_predicate_components;
                builder.framed = previous_framed;
                builder.source_fields = previous_source_fields;
                builder.query_scope_sources = previous_query_scope_sources;
                builder.warned_unscoped_queries = previous_warned_unscoped_queries;
                builder.udf_nodes = previous_udf_nodes;
                builder.warnings = previous_warnings;
                builder.warned_sequence_uses = previous_warned_sequence_uses;
                builder.warned_scalar_filters = previous_warned_scalar_filters;
                for index in &self.claimed_sequences {
                    if builder
                        .sequence_items
                        .get(index)
                        .is_some_and(|item| scope_owns_sequence_item(&scopes.root, *item))
                    {
                        continue;
                    }
                    builder.sequence_scope_components.remove(index);
                    if builder.warned_sequence_uses.insert(*index)
                        && let Some(component) = builder.fn_components.get(*index)
                    {
                        builder.warnings.push(format!(
                            "sequence function `{}` is not connected to a repeating target; scalar use is unsupported",
                            component.name
                        ));
                    }
                }
                Err(reason)
            }
        }
    }
}

impl RootDynamicJsonTarget {
    fn input_count(&self) -> usize {
        3 + self.fields.len() * 2
    }

    fn attach_schema(&self, schema: &mut SchemaNode) -> bool {
        let item = SchemaNode::group("item", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("value", self.value_type));
        item.is_some_and(|item| schema.set_dynamic_fields(Some(item.repeating())))
    }

    fn frame_sources(&self, builder: &GraphBuilder<'_>) -> Result<[SourcePath; 2], String> {
        let outer = self.iteration_source(builder, self.property_input, true)?;
        let item = self.iteration_source(builder, self.item_input, false)?;
        Ok([outer, item])
    }

    fn build(&self, builder: &mut GraphBuilder<'_>, root: &mut Scope) -> Result<(), String> {
        if !root_is_unconfigured(root) {
            return Err(
                "computed root properties cannot be combined with ordinary target ports or root iteration controls yet"
                    .to_string(),
            );
        }
        let mut candidate = root.clone();
        let outer_feed = self.connected(builder, self.property_input, "property collection")?;
        let outer_control = builder.resolve_iteration_feed(outer_feed);
        self.require_supported_controls(&outer_control, true)?;
        let outer_source = builder
            .iteration_source_path(&outer_control)
            .ok_or_else(|| "computed property collection has no source collection".to_string())?;
        let outer_source = self.normalize_source(builder, outer_source)?;
        let outer_abs = builder.context_path(&outer_source);
        candidate.set_source(Some(outer_abs.clone()));
        candidate.group_by = outer_control
            .group_key
            .and_then(|key| builder.value_node(key));
        if outer_control.group_key.is_some() && candidate.group_by.is_none() {
            return Err("computed property group key is not representable".to_string());
        }
        candidate.merge_dynamic_fields = true;

        let item_feed = self.connected(builder, self.item_input, "array item")?;
        let item_control = builder.resolve_iteration_feed(item_feed);
        self.require_supported_controls(&item_control, false)?;
        let item_source = builder
            .iteration_source_path(&item_control)
            .ok_or_else(|| "computed property array item has no source collection".to_string())?;
        let item_source = self.normalize_source(builder, item_source)?;
        let item_abs = builder.context_path(&item_source);
        if !item_abs.starts_with(&outer_abs) {
            return Err(
                "computed property values do not iterate below their property collection"
                    .to_string(),
            );
        }
        let relative_start = if outer_control.group_key.is_some() {
            outer_abs.len().saturating_sub(1)
        } else {
            outer_abs.len()
        };
        let relative = item_abs[relative_start..].to_vec();
        let mut item_scope = Scope {
            iteration: mapping::ScopeIteration::Source(relative),
            ..Scope::default()
        };
        for field in &self.fields {
            let key = self.value_from_input(builder, field.key_input, "item property name")?;
            let value = self.value_from_input(builder, field.value_input, "item property value")?;
            item_scope
                .dynamic_bindings
                .push(DynamicBinding { key, value });
        }
        let key = self.value_from_input(builder, self.property_key_input, "property name")?;
        candidate.dynamic_children.push(DynamicChild {
            key,
            scope: item_scope,
        });
        *root = candidate;
        Ok(())
    }

    fn iteration_source(
        &self,
        builder: &GraphBuilder<'_>,
        input: u32,
        allow_group: bool,
    ) -> Result<SourcePath, String> {
        let feed = self.connected(builder, input, "iteration")?;
        let control = builder.resolve_iteration_feed(feed);
        self.require_supported_controls(&control, allow_group)?;
        let source = builder
            .iteration_source_path(&control)
            .ok_or_else(|| "computed property iteration has no source collection".to_string())?;
        self.normalize_source(builder, source)
    }

    fn normalize_source(
        &self,
        builder: &GraphBuilder<'_>,
        mut source: SourcePath,
    ) -> Result<SourcePath, String> {
        let schema = &builder
            .sources
            .get(source.source)
            .ok_or_else(|| "computed property source component is missing".to_string())?
            .schema;
        source.path = super::iteration::split_at_innermost_repeating(schema, &source.path).0;
        Ok(source)
    }

    fn require_supported_controls(
        &self,
        feed: &super::iteration::IterationFeed,
        allow_group: bool,
    ) -> Result<(), String> {
        let unsupported = feed.sequence_component.is_some()
            || feed.db_where_component.is_some()
            || feed.has_filter
            || feed.group_starting_with.is_some()
            || feed.has_start_grouping
            || feed.block_size.is_some()
            || feed.has_block_grouping
            || feed.distinct_key.is_some()
            || feed.order_issue.is_some()
            || feed.sort_expr.is_some()
            || feed.take_expr.is_some()
            || feed.take_default_one
            || !allow_group && feed.group_key.is_some();
        if unsupported {
            Err(
                "computed properties currently support only plain iteration and one outer group-by"
                    .to_string(),
            )
        } else {
            Ok(())
        }
    }

    fn connected(
        &self,
        builder: &GraphBuilder<'_>,
        input: u32,
        label: &str,
    ) -> Result<u32, String> {
        builder
            .edge_from
            .get(&input)
            .copied()
            .ok_or_else(|| format!("computed {label} input is not connected"))
    }

    fn value_from_input(
        &self,
        builder: &mut GraphBuilder<'_>,
        input: u32,
        label: &str,
    ) -> Result<mapping::NodeId, String> {
        let feed = self.connected(builder, input, label)?;
        builder
            .value_node(feed)
            .ok_or_else(|| format!("computed {label} comes from an unsupported feed"))
    }
}

impl DynamicObjectTarget {
    fn iteration_source(&self, builder: &GraphBuilder<'_>) -> Result<Option<SourcePath>, String> {
        let feed = builder
            .edge_from
            .get(&self.property_input)
            .copied()
            .ok_or_else(|| {
                "computed nested property collection input is not connected".to_string()
            })?;
        let control = builder.resolve_iteration_feed(feed);
        if control.db_where_component.is_some()
            || control.has_filter
            || control.has_key_grouping
            || control.has_start_grouping
            || control.has_block_grouping
            || control.distinct_key.is_some()
            || control.order_issue.is_some()
            || control.has_sort
            || control.take_expr.is_some()
            || control.take_default_one
        {
            return Err(
                "nested computed properties currently support only plain source or generated-sequence iteration"
                    .to_string(),
            );
        }
        if control.sequence_component.is_some() {
            return Ok(None);
        }
        let mut source = builder
            .iteration_source_path(&control)
            .ok_or_else(|| "computed nested property has no source collection".to_string())?;
        let schema = &builder
            .sources
            .get(source.source)
            .ok_or_else(|| "computed nested property source component is missing".to_string())?
            .schema;
        source.path = super::iteration::split_at_innermost_repeating(schema, &source.path).0;
        Ok(Some(source))
    }

    fn build(
        &self,
        builder: &mut GraphBuilder<'_>,
        scopes: &mut ScopeBuilder,
    ) -> Result<(), String> {
        if !scope_is_unconfigured(scopes.ensure_scope(&self.owner_path)) {
            return Err(format!(
                "computed nested object `{}` cannot be combined with an existing target scope",
                self.owner_path.join("/")
            ));
        }
        let property_feed = builder
            .edge_from
            .get(&self.property_input)
            .copied()
            .ok_or_else(|| {
                "computed nested property collection input is not connected".to_string()
            })?;
        let control = builder.resolve_iteration_feed(property_feed);
        let nodes = IterationNodes {
            filter: None,
            group_by: None,
            group_starting_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_filter_order: Default::default(),
            take: None,
        };
        if let Some(index) = control.sequence_component {
            builder.sequence_scope_components.insert(index);
            let sequence = builder.sequence_expr(index).ok_or_else(|| {
                "computed nested property generated sequence is invalid".to_string()
            })?;
            if scope_owns_sequence_item(&scopes.root, sequence.item()) {
                return Err(
                    "computed nested property generated sequence already feeds another target iteration"
                        .to_string(),
                );
            }
            scopes.add_sequence(&self.owner_path, sequence, nodes, IterationOutput::Repeated);
        } else {
            let source = self
                .iteration_source(builder)?
                .ok_or_else(|| "computed nested property has no source collection".to_string())?;
            let source_abs = builder.context_path(&source);
            scopes.add_iteration(
                &self.owner_path,
                &source_abs,
                nodes,
                IterationOutput::Repeated,
            );
        }
        let key = self.value_from_input(builder, self.key_input, "nested property name")?;
        let value = self.value_from_input(builder, self.value_input, "nested property value")?;
        let scope = scopes.ensure_scope(&self.owner_path);
        scope.merge_dynamic_fields = true;
        scope.dynamic_bindings.push(DynamicBinding { key, value });
        Ok(())
    }

    fn value_from_input(
        &self,
        builder: &mut GraphBuilder<'_>,
        input: u32,
        label: &str,
    ) -> Result<mapping::NodeId, String> {
        let feed = builder
            .edge_from
            .get(&input)
            .copied()
            .ok_or_else(|| format!("computed {label} input is not connected"))?;
        builder
            .value_node(feed)
            .ok_or_else(|| format!("computed {label} comes from an unsupported feed"))
    }
}

fn schema_node_at_mut<'a>(
    schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    let mut current = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut current.kind else {
            return None;
        };
        current = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(current)
}

fn scope_is_unconfigured(scope: &Scope) -> bool {
    !scope.iterates()
        && scope.filter.is_none()
        && scope.group_by.is_none()
        && scope.group_starting_with.is_none()
        && scope.group_into_blocks.is_none()
        && scope.sort_by.is_none()
        && !scope.sort_descending
        && scope.take.is_none()
        && scope.bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn root_is_unconfigured(root: &Scope) -> bool {
    scope_is_unconfigured(root)
}

pub(super) fn read_target(
    entry: &roxmltree::Node<'_, '_>,
) -> Result<Option<DynamicJsonTarget>, String> {
    if entry
        .descendants()
        .any(|node| node.attribute("outkey").is_some())
    {
        return Ok(None);
    }
    if entry.attribute("name") != Some("root") {
        return Ok(None);
    }
    let root = read_root_target(entry)?;
    let root_property_input = root.as_ref().map(|target| target.property_input);
    let mut nested = Vec::new();
    collect_nested_targets(entry, &mut Vec::new(), root_property_input, &mut nested)?;
    if root.is_none() && nested.is_empty() {
        Ok(None)
    } else {
        Ok(Some(DynamicJsonTarget {
            root,
            nested,
            claimed_sequences: BTreeSet::new(),
        }))
    }
}

fn scope_owns_sequence_item(scope: &Scope, item: NodeId) -> bool {
    scope
        .sequence()
        .is_some_and(|sequence| sequence.item() == item)
        || scope
            .children
            .iter()
            .any(|child| scope_owns_sequence_item(child, item))
        || scope
            .dynamic_children
            .iter()
            .any(|child| scope_owns_sequence_item(&child.scope, item))
}

fn read_root_target(
    entry: &roxmltree::Node<'_, '_>,
) -> Result<Option<RootDynamicJsonTarget>, String> {
    let objects = entry
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("object"))
        .collect::<Vec<_>>();
    let [object] = objects.as_slice() else {
        return Ok(None);
    };
    let dynamic = object
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && node.attribute("type") == Some("json-property")
                && node.children().any(|child| {
                    child.has_tag_name("entry")
                        && child.attribute("type") == Some("json-propertyname")
                })
        })
        .collect::<Vec<_>>();
    let property_count = object
        .children()
        .filter(|node| {
            node.has_tag_name("entry") && node.attribute("type") == Some("json-property")
        })
        .count();
    let [property] = dynamic.as_slice() else {
        return if dynamic.is_empty() {
            Ok(None)
        } else {
            Err("exactly one computed root property collection is supported".to_string())
        };
    };
    if property_count != 1 {
        return Err(
            "computed root properties cannot be mixed with ordinary JSON properties yet"
                .to_string(),
        );
    }
    let property_input = input_key(property, "computed property collection")?;
    let property_key = direct_child(property, Some("json-propertyname"), Some("name"))?;
    let property_key_input = input_key(&property_key, "computed property name")?;
    let array = direct_child(property, None, Some("array"))?;
    let item = direct_child(&array, Some("json-item"), Some("item"))?;
    let item_input = input_key(&item, "computed property array item")?;
    let item_object = direct_child(&item, None, Some("object"))?;
    let properties = item_object
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    if properties.is_empty()
        || properties
            .iter()
            .any(|node| node.attribute("type") != Some("json-property"))
    {
        return Err(
            "computed array items must contain only computed scalar properties".to_string(),
        );
    }
    let mut value_type = None;
    let mut fields = Vec::with_capacity(properties.len());
    for property in properties {
        let name = direct_child(&property, Some("json-propertyname"), Some("name"))?;
        let key_input = input_key(&name, "computed item property name")?;
        let values = connected_value_branches(&property);
        let [value] = values.as_slice() else {
            return Err(
                "computed item properties require exactly one scalar value branch".to_string(),
            );
        };
        let ty = scalar_type(value.attribute("name").unwrap_or_default()).ok_or_else(|| {
            "computed item property values must be string, number, integer, or boolean".to_string()
        })?;
        if value_type.is_some_and(|existing| existing != ty) {
            return Err("computed item properties must share one scalar type".to_string());
        }
        value_type = Some(ty);
        fields.push(DynamicScalarTarget {
            key_input,
            value_input: input_key(value, "computed item property value")?,
        });
    }
    Ok(Some(RootDynamicJsonTarget {
        property_input,
        property_key_input,
        item_input,
        fields,
        value_type: value_type.ok_or_else(|| "computed item has no values".to_string())?,
    }))
}

fn collect_nested_targets(
    entry: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    root_property_input: Option<u32>,
    targets: &mut Vec<DynamicObjectTarget>,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        if child.attribute("type") == Some("json-property") {
            if is_computed_property(&child) {
                let property_input = input_key(&child, "computed property collection")?;
                if Some(property_input) == root_property_input {
                    continue;
                }
                targets.push(read_nested_target(&child, path, property_input)?);
                continue;
            }
            path.push(child.attribute("name").unwrap_or_default().to_string());
            collect_nested_targets(&child, path, root_property_input, targets)?;
            path.pop();
        } else if child.attribute("type") != Some("json-propertyname") {
            collect_nested_targets(&child, path, root_property_input, targets)?;
        }
    }
    Ok(())
}

fn is_computed_property(entry: &roxmltree::Node<'_, '_>) -> bool {
    if entry.attribute("type") != Some("json-property") {
        return false;
    }
    let name = entry.children().find(|child| {
        child.has_tag_name("entry") && child.attribute("type") == Some("json-propertyname")
    });
    name.is_some_and(|name| {
        entry.attribute("inpkey").is_some() || name.attribute("inpkey").is_some()
    })
}

fn read_nested_target(
    property: &roxmltree::Node<'_, '_>,
    owner_path: &[String],
    property_input: u32,
) -> Result<DynamicObjectTarget, String> {
    if owner_path.is_empty() {
        return Err("computed scalar properties require a named enclosing JSON object".to_string());
    }
    let name = direct_child(property, Some("json-propertyname"), Some("name"))?;
    let values = connected_value_branches(property);
    let [value] = values.as_slice() else {
        return Err(
            "nested computed properties require exactly one scalar value branch".to_string(),
        );
    };
    let value_type = scalar_type(value.attribute("name").unwrap_or_default()).ok_or_else(|| {
        "nested computed property values must be string, number, integer, or boolean".to_string()
    })?;
    Ok(DynamicObjectTarget {
        owner_path: owner_path.to_vec(),
        property_input,
        key_input: input_key(&name, "nested computed property name")?,
        value_input: input_key(value, "nested computed property value")?,
        value_type,
    })
}

fn connected_value_branches<'a, 'input>(
    property: &roxmltree::Node<'a, 'input>,
) -> Vec<roxmltree::Node<'a, 'input>> {
    property
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && node.attribute("type") != Some("json-propertyname")
                && (node.attribute("inpkey").is_some()
                    || node.descendants().any(|descendant| {
                        descendant.has_tag_name("entry") && descendant.attribute("inpkey").is_some()
                    }))
        })
        .collect()
}

fn direct_child<'a, 'input>(
    parent: &roxmltree::Node<'a, 'input>,
    entry_type: Option<&str>,
    name: Option<&str>,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let matches = parent
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && entry_type.is_none_or(|ty| node.attribute("type") == Some(ty))
                && name.is_none_or(|name| node.attribute("name") == Some(name))
        })
        .collect::<Vec<_>>();
    let [child] = matches.as_slice() else {
        return Err("computed JSON property has an ambiguous or missing value shape".to_string());
    };
    Ok(*child)
}

fn input_key(node: &roxmltree::Node<'_, '_>, label: &str) -> Result<u32, String> {
    super::schema::parse_u32(node.attribute("inpkey"))
        .ok_or_else(|| format!("{label} has no valid connected input port"))
}

fn scalar_type(name: &str) -> Option<ScalarType> {
    Some(match name {
        "string" => ScalarType::String,
        "number" => ScalarType::Float,
        "integer" => ScalarType::Int,
        "boolean" => ScalarType::Bool,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use mapping::Scope;

    use super::{read_target, root_is_unconfigured};

    #[test]
    fn configured_root_controls_are_not_available_for_dynamic_lowering() {
        let mut root = Scope::default();
        assert!(root_is_unconfigured(&root));
        root.set_source(Some(Vec::new()));
        assert!(!root_is_unconfigured(&root));

        let root = Scope {
            sort_descending: true,
            ..Scope::default()
        };
        assert!(!root_is_unconfigured(&root));
    }

    #[test]
    fn reports_computed_properties_below_a_non_object_root_value() {
        let document = roxmltree::Document::parse(
            r#"<entry name="root"><entry name="array"><entry name="item" type="json-item"><entry name="object"><entry name="property" type="json-property" inpkey="1"><entry name="name" type="json-propertyname" inpkey="2"/><entry name="array"><entry name="item" type="json-item" inpkey="3"><entry name="object"><entry name="property" type="json-property"><entry name="name" type="json-propertyname" inpkey="4"/><entry name="string" inpkey="5"/></entry></entry></entry></entry></entry></entry></entry></entry></entry>"#,
        )
        .unwrap();

        let Err(error) = read_target(&document.root_element()) else {
            panic!("unsupported nested computed array should be reported");
        };
        assert!(error.contains("require a named enclosing JSON object"));
    }

    #[test]
    fn ignores_object_nodes_outside_the_json_root_wrapper() {
        let document = roxmltree::Document::parse(
            r#"<entry name="document"><entry name="object"><entry name="property" type="json-property" inpkey="1"><entry name="name" type="json-propertyname" inpkey="2"/></entry></entry></entry>"#,
        )
        .unwrap();

        assert!(read_target(&document.root_element()).unwrap().is_none());
    }
}
