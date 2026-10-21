//! Explicitly owned Lookup and CollectionFind over one exact endpoint cohort.

use super::*;

/// Annotate only owned-source failures raised by this node's own walk or
/// item reads; nested predicate, needle and value nodes keep their own nodes.
pub(super) fn owned_call(node: NodeId, target: &str, call: &str) -> String {
    format!(
        "try {{ {target}{call}; }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error_{node}) {{ error_{node}.WithOwnedSourceNode({node}U); throw; }}"
    )
}

pub(super) fn walk_call(node: NodeId, collection: &::mapping::OwnedSourceRef) -> String {
    let mut call = String::from("context.WalkOwnedSourceCohort(");
    render_owner(&collection.owner, &mut call);
    call.push_str(", ");
    mapping::basic_path(&collection.path, &mut call);
    call.push(')');
    owned_call(node, &format!("cohort_{node} = "), &call)
}

fn item_field_call(node: NodeId, lookup: &::mapping::OwnedLookup, relative: &[String]) -> String {
    let mut path = lookup.collection.path.clone();
    path.extend(relative.iter().cloned());
    let mut call = format!("item_context_{node}.ResolveOwnedField(");
    render_owner(&lookup.collection.owner, &mut call);
    call.push_str(", ");
    mapping::basic_path(&path, &mut call);
    call.push_str(", null)");
    call
}

/// Owned search walks its complete cohort before any item expression; a miss
/// is Null. Lookup keys never match when Null-like.
pub(crate) fn render_owned_search(
    node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    let (collection, needle) = match expression {
        Expression::OwnedLookup(lookup) => (&lookup.collection, Some(lookup.matches)),
        Expression::OwnedCollectionFind(find) => (&find.collection, None),
        _ => unreachable!("owned search renderer receives only owned search"),
    };
    output.push_str("\n    {\n");
    if let Some(needle) = needle {
        output.push_str(&format!(
            "        var needle_{node} = Node_{needle}(context);\n"
        ));
    }
    output.push_str(&format!(
        "        global::Ferrule.Runtime.FerruleRetainedSourceCohort cohort_{node};\n        {}\n        try\n        {{\n            foreach (var candidate_{node} in cohort_{node})\n            {{\n                var item_context_{node} = candidate_{node}.Context;\n",
        walk_call(node, collection)
    ));
    match expression {
        Expression::OwnedLookup(lookup) => {
            output.push_str(&format!(
                "                global::Ferrule.Runtime.FerruleValue key_{node};\n                {}\n                if (key_{node}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull or global::Ferrule.Runtime.FerruleValueKind.XmlNil) && key_{node} == needle_{node})\n                {{\n                    {}\n                }}\n",
                owned_call(node, &format!("key_{node} = "), &item_field_call(node, lookup, &lookup.key)),
                owned_call(node, "return ", &item_field_call(node, lookup, &lookup.value)),
            ));
        }
        Expression::OwnedCollectionFind(find) => {
            let (predicate, value) = (find.predicate, find.value);
            output.push_str(&format!(
                "                var predicate_{node} = Node_{predicate}(item_context_{node});\n                if (predicate_{node}.Kind == global::Ferrule.Runtime.FerruleValueKind.Bool)\n                {{\n                    if (predicate_{node}.BooleanValue)\n                    {{\n                        return Node_{value}(item_context_{node});\n                    }}\n                    continue;\n                }}\n                if (predicate_{node}.Kind is global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull or global::Ferrule.Runtime.FerruleValueKind.XmlNil)\n                {{\n                    continue;\n                }}\n                _ = global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(predicate_{node}, {predicate}U);\n"
            ));
        }
        _ => unreachable!("owned search renderer receives only owned search"),
    }
    output.push_str(&format!(
        "            }}\n        }}\n        finally {{ global::System.GC.KeepAlive(cohort_{node}); }}\n        return global::Ferrule.Runtime.FerruleValue.Null;\n    }}\n"
    ));
    Ok(())
}
