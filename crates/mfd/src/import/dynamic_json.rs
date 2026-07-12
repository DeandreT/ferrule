use ir::{ScalarType, SchemaNode};
use mapping::{DynamicBinding, DynamicChild, Scope};

use super::graph::GraphBuilder;
use super::schema::SchemaComponent;
use super::source::SourcePath;

#[derive(Clone)]
pub(super) struct DynamicJsonTarget {
    pub(super) property_input: u32,
    pub(super) property_key_input: u32,
    pub(super) item_input: u32,
    pub(super) fields: Vec<DynamicScalarTarget>,
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
        .and_then(|dynamic| match dynamic.prepare_frames(builder) {
            Ok(()) => Some(dynamic),
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
    root: &mut Scope,
) {
    if let Some(dynamic) = dynamic
        && let Err(reason) = dynamic.build(builder, root)
    {
        builder.warnings.push(format!(
            "dynamic JSON target `{}` is unsupported: {reason}",
            component.name
        ));
    }
}

impl DynamicJsonTarget {
    pub(super) fn input_count(&self) -> usize {
        3 + self.fields.len() * 2
    }

    pub(super) fn attach_schema(&self, schema: &mut SchemaNode) -> bool {
        let item = SchemaNode::group("item", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("value", self.value_type));
        item.is_some_and(|item| schema.set_dynamic_fields(Some(item.repeating())))
    }

    pub(super) fn prepare_frames(&self, builder: &mut GraphBuilder<'_>) -> Result<(), String> {
        let outer = self.iteration_source(builder, self.property_input, true)?;
        let item = self.iteration_source(builder, self.item_input, false)?;
        builder.note_framed_prefixes(&outer);
        builder.note_framed_prefixes(&item);
        Ok(())
    }

    pub(super) fn build(
        &self,
        builder: &mut GraphBuilder<'_>,
        root: &mut Scope,
    ) -> Result<(), String> {
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
            || feed.filter_expr.is_some()
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

fn root_is_unconfigured(root: &Scope) -> bool {
    !root.iterates()
        && root.filter.is_none()
        && root.group_by.is_none()
        && root.group_starting_with.is_none()
        && root.group_into_blocks.is_none()
        && root.sort_by.is_none()
        && !root.sort_descending
        && root.take.is_none()
        && root.bindings.is_empty()
        && root.children.is_empty()
        && root.dynamic_bindings.is_empty()
        && root.dynamic_children.is_empty()
        && !root.merge_dynamic_fields
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
        let values = property
            .children()
            .filter(|node| {
                node.has_tag_name("entry") && node.attribute("type") != Some("json-propertyname")
            })
            .collect::<Vec<_>>();
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
    Ok(Some(DynamicJsonTarget {
        property_input,
        property_key_input,
        item_input,
        fields,
        value_type: value_type.ok_or_else(|| "computed item has no values".to_string())?,
    }))
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
    fn ignores_computed_properties_below_a_non_object_root_value() {
        let document = roxmltree::Document::parse(
            r#"<entry name="root"><entry name="array"><entry name="item" type="json-item"><entry name="object"><entry name="property" type="json-property" inpkey="1"><entry name="name" type="json-propertyname" inpkey="2"/><entry name="array"><entry name="item" type="json-item" inpkey="3"><entry name="object"><entry name="property" type="json-property"><entry name="name" type="json-propertyname" inpkey="4"/><entry name="string" inpkey="5"/></entry></entry></entry></entry></entry></entry></entry></entry></entry>"#,
        )
        .unwrap();

        assert!(read_target(&document.root_element()).unwrap().is_none());
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
