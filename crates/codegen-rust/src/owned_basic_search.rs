//! Explicitly owned Lookup and CollectionFind over one exact endpoint cohort.

use super::*;

/// The needle runs once in the parent; the complete owned cohort is admitted
/// before any key read. Null-like keys never match and a miss is Null.
pub(crate) fn render_owned_lookup(
    id: NodeId,
    lookup: &mapping::OwnedLookup,
    needle: &str,
) -> String {
    let owner = owner_literal(&lookup.collection.owner);
    let collection = render_string_path(&lookup.collection.path);
    let item_path = |relative: &[String]| {
        let mut path = lookup.collection.path.clone();
        path.extend(relative.iter().cloned());
        render_string_path(&path)
    };
    let (key, value) = (item_path(&lookup.key), item_path(&lookup.value));
    format!(
        "{{\n        let needle = {needle}?;\n        let cohort = context.walk_owned_source_cohort({owner}, &[{collection}]).map_err(|error| error.with_owned_source_node({id}))?;\n        for candidate in cohort.iter() {{\n            let item_context = candidate.context();\n            let key = item_context.owned_field({owner}, &[{key}], None).map_err(|error| error.with_owned_source_node({id}))?;\n            if !matches!(key, Value::Null | Value::JsonNull(_) | Value::XmlNil(_)) && key == needle {{\n                return item_context.owned_field({owner}, &[{value}], None).map_err(|error| error.with_owned_source_node({id}));\n            }}\n        }}\n        Ok(Value::Null)\n    }}"
    )
}

/// Predicate and value run in each candidate's context, in source order.
/// Null-like predicates skip an item; other non-Booleans fail at the predicate.
pub(crate) fn render_owned_collection_find(
    id: NodeId,
    find: &mapping::OwnedCollectionFind,
    predicate: &str,
    value: &str,
) -> String {
    let owner = owner_literal(&find.collection.owner);
    let collection = render_string_path(&find.collection.path);
    let predicate_node = find.predicate;
    format!(
        "{{\n        let cohort = context.walk_owned_source_cohort({owner}, &[{collection}]).map_err(|error| error.with_owned_source_node({id}))?;\n        for candidate in cohort.iter() {{\n            let item_context = candidate.context();\n            match {predicate}? {{\n                Value::Bool(true) => return {value},\n                Value::Bool(false) | Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {{}}\n                other => return Err(RuntimeError::NotABool {{ node: {predicate_node}, found: other.type_name() }}),\n            }}\n        }}\n        Ok(Value::Null)\n    }}"
    )
}
