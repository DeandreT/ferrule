use ir::{Instance, Value};

use crate::{GroupField, RuntimeError};

/// Requires one computed property name to be a string.
pub fn dynamic_property_name(node: u32, value: Value) -> Result<String, RuntimeError> {
    match value {
        Value::String(name) => Ok(name),
        value => Err(RuntimeError::DynamicPropertyName {
            node,
            found: value.type_name(),
        }),
    }
}

/// Inserts one computed field without allowing fixed or prior names to collide.
pub fn insert_dynamic_field(
    fields: &mut Vec<GroupField>,
    fixed_fields: &[&str],
    name: String,
    value: Instance,
) -> Result<(), RuntimeError> {
    if fixed_fields.contains(&name.as_str()) || fields.iter().any(|(existing, _)| existing == &name)
    {
        return Err(RuntimeError::DuplicateDynamicProperty(name));
    }
    fields.push((name, value));
    Ok(())
}

/// Merges insertion-ordered object fragments, rejecting all duplicate names.
pub fn merge_dynamic_fragments(fragments: Vec<Instance>) -> Result<Instance, RuntimeError> {
    let mut merged = Vec::new();
    for fragment in fragments {
        let Instance::Group(fields) = fragment else {
            return Err(RuntimeError::InvalidDynamicPropertyFragment);
        };
        for (name, value) in fields {
            insert_dynamic_field(&mut merged, &[], name, value)?;
        }
    }
    Ok(Instance::Group(merged))
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use super::*;
    use crate::{field, group, scalar};

    #[test]
    fn keys_and_insertion_retain_typed_failures() {
        assert_eq!(
            dynamic_property_name(7, Value::Int(1)),
            Err(RuntimeError::DynamicPropertyName {
                node: 7,
                found: "int",
            })
        );
        let mut fields = vec![field("prior", scalar(Value::Null))];
        assert_eq!(
            insert_dynamic_field(&mut fields, &["fixed"], "fixed".into(), scalar(Value::Null)),
            Err(RuntimeError::DuplicateDynamicProperty("fixed".into()))
        );
        assert_eq!(
            insert_dynamic_field(&mut fields, &["fixed"], "prior".into(), scalar(Value::Null)),
            Err(RuntimeError::DuplicateDynamicProperty("prior".into()))
        );
    }

    #[test]
    fn fragment_merge_is_ordered_and_requires_unique_groups() {
        let merged = merge_dynamic_fragments(vec![
            group([field("first", scalar(Value::Int(1)))]),
            group([field("second", scalar(Value::Int(2)))]),
        ])
        .unwrap();
        let Instance::Group(fields) = merged else {
            panic!("merge returns a group")
        };
        assert_eq!(
            fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(
            merge_dynamic_fragments(vec![
                group([field("same", scalar(Value::Null))]),
                group([field("same", scalar(Value::Null))]),
            ]),
            Err(RuntimeError::DuplicateDynamicProperty("same".into()))
        );
        assert_eq!(
            merge_dynamic_fragments(vec![scalar(Value::Null)]),
            Err(RuntimeError::InvalidDynamicPropertyFragment)
        );
    }
}
