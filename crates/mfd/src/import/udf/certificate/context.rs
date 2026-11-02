use super::*;
use crate::import::udf::certificate::{ArgumentClosure, Boundary, RawOwner};

struct Frame<'a> {
    owner: RawOwner,
    source: &'a SchemaComponent,
    path: &'a [String],
}

struct Receiver {
    target: usize,
    output: u32,
    field: String,
    scopes: Vec<(Vec<String>, Option<Vec<String>>)>,
}

/// A final-context postcondition, not another scope resolver. Only plain
/// constructed contexts with the exact proved driver representation pass.
#[derive(Default)]
pub(in crate::import) struct ContextSeal {
    receivers: Vec<Receiver>,
    budget: Option<ConstructionBudget>,
}

fn copy_path(path: &[String], budget: &ConstructionBudget) -> Result<Vec<String>, String> {
    budget.claim(
        path.len().saturating_add(1),
        path.iter().map(String::len).sum(),
        path.len(),
    )?;
    Ok(path.to_vec())
}

fn frame<'a>(view: &'a MainProducerView<'a>, feed: u32) -> Result<Frame<'a>, String> {
    let Some([claim]) = view.claims.get(&feed).map(Vec::as_slice) else {
        return Err("scope driver has no unique certificate".into());
    };
    match (&claim.kind, claim.origin) {
        (Kind::Source(source, path), Origin::ActiveMain(owner)) => Ok(Frame {
            owner,
            source,
            path,
        }),
        _ => Err("scope driver has no ordinary source identity".into()),
    }
}

fn receiving<'a>(
    view: &'a MainProducerView<'a>,
    n: &Normalized<'a>,
    closure: &ArgumentClosure,
    boundary: &Boundary,
    target: &SchemaComponent,
    budget: &ConstructionBudget,
    structure: roxmltree::Node<'_, '_>,
) -> Result<Receiver, String> {
    let refuse = || {
        format!(
            "declared-library post-normalization ownership certificate refused declared output `{}`: unproved per-receiver frame or scope-hint authority",
            boundary.output
        )
    };
    let path = boundary.path.as_ref().ok_or_else(refuse)?;
    budget.claim(boundary.scopes.len().saturating_add(1), 0, path.len())?;
    let mut scopes = boundary.scopes.iter().collect::<Vec<_>>();
    scopes.sort_unstable_by_key(|(path, _, _)| path.len());
    let mut available: Vec<Frame<'_>> = Vec::new();
    let mut expected = Vec::new();
    for (target_path, _, feed) in scopes {
        let current = frame(view, *feed).map_err(|_| refuse())?;
        for length in 1..current.path.len() {
            let prefix = &current.path[..length];
            if exact_path(&current.source.schema, prefix).is_some_and(|node| node.repeating)
                && !available
                    .iter()
                    .any(|frame| frame.owner == current.owner && frame.path == prefix)
            {
                return Err(refuse());
            }
        }
        let stored = if let Some(parent) = available.last() {
            // No legacy innermost-field/root-fallback approximation: certify
            // only same-source descendant or identity chains in nested scopes.
            if parent.owner != current.owner || !current.path.starts_with(parent.path) {
                return Err(refuse());
            }
            copy_path(&current.path[parent.path.len()..], budget)?
        } else {
            let index = n
                .sources
                .iter()
                .position(|source| std::ptr::eq(*source, current.source))
                .ok_or_else(refuse)?;
            if index == 0 {
                copy_path(current.path, budget)?
            } else {
                let name = &n.source_names[index];
                if n.sources[0].schema.child(name).is_some() {
                    return Err(refuse());
                }
                budget.claim(
                    current.path.len().saturating_add(2),
                    name.len()
                        .saturating_add(current.path.iter().map(String::len).sum()),
                    current.path.len(),
                )?;
                std::iter::once(name.clone())
                    .chain(current.path.iter().cloned())
                    .collect()
            }
        };
        budget.claim(2, 0, target_path.len())?;
        expected.push((copy_path(target_path, budget)?, Some(stored)));
        available.push(current);
    }
    // Every dependency is checked in THIS receiver's ancestor chain. A frame
    // available only in a sibling boundary is not current-item authority.
    for (&key, raw) in &closure.arguments {
        if let Some(
            [
                Claim {
                    kind: Kind::Source(source, source_path),
                    ..
                },
            ],
        ) = view.claims.get(&key).map(Vec::as_slice)
        {
            for length in 1..source_path.len() {
                let prefix = &source_path[..length];
                if exact_path(&source.schema, prefix).is_some_and(|node| node.repeating)
                    && !available
                        .iter()
                        .any(|frame| frame.owner == raw.pin.owner && frame.path == prefix)
                {
                    return Err(refuse());
                }
            }
        }
    }
    // Record the whole receiving ancestor chain, including contexts expected
    // not to iterate. This seals later construction/rewriting as well as hints.
    budget.claim(path.len().saturating_add(1), 0, path.len())?;
    let mut chain = Vec::new();
    for length in 0..path.len() {
        let prefix = &path[..length];
        let value = expected
            .iter()
            .find(|(path, _)| path == prefix)
            .and_then(|(_, value)| value.as_ref());
        let value = match value {
            Some(value) => Some(copy_path(value, budget)?),
            None => None,
        };
        chain.push((copy_path(prefix, budget)?, value));
    }
    validate_hints(structure, target, path, &expected, budget).map_err(|_| refuse())?;
    let field = path.last().ok_or_else(refuse)?;
    budget.claim(1, field.len(), path.len())?;
    Ok(Receiver {
        target: std::ptr::from_ref(target) as usize,
        output: boundary.output,
        field: field.clone(),
        scopes: chain,
    })
}

fn validate_hints(
    structure: roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    receiver: &[String],
    expected: &[(Vec<String>, Option<Vec<String>>)],
    budget: &ConstructionBudget,
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for metadata in structure
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "ferrule-scope-sources")
    {
        for hint in metadata.children().filter(|node| node.is_element()) {
            budget.claim(1, 0, 0)?;
            let Some(key) = schema::parse_u32(hint.attribute("targetkey")) else {
                continue;
            };
            let Some(path) = target
                .ports
                .get(&key)
                .filter(|path| receiver.starts_with(path))
            else {
                continue;
            };
            if metadata.tag_name().namespace().is_some()
                || metadata.attribute("version") != Some("1")
                || metadata
                    .attributes()
                    .any(|a| a.namespace().is_some() || a.name() != "version")
                || !hint.has_tag_name("scope")
                || hint.tag_name().namespace().is_some()
                || hint
                    .attributes()
                    .any(|a| a.namespace().is_some() || !matches!(a.name(), "targetkey" | "source"))
                || hint.children().any(|node| node.is_element())
            {
                return Err("unproved affected scope hint".into());
            }
            budget.claim(1, 0, path.len())?;
            if !seen.insert(path.as_slice()) {
                return Err("duplicate affected scope hint before collapse".into());
            }
            let wanted = expected
                .iter()
                .find(|(scope, _)| scope == path)
                .and_then(|(_, source)| source.as_ref())
                .ok_or("hint has no proved receiving scope")?;
            let text = hint.attribute("source").ok_or("scope hint has no source")?;
            if text.len() > 4096 {
                return Err("scope hint path budget exceeded".into());
            }
            budget.claim(text.len().saturating_add(1), text.len(), 0)?;
            let actual: Vec<String> =
                serde_json::from_str(text).map_err(|_| "invalid affected scope hint")?;
            if actual.len() > 256 || &actual != wanted {
                return Err("scope hint changes proved receiver context".into());
            }
        }
    }
    Ok(())
}

pub(super) fn build<'a>(
    view: &'a MainProducerView<'a>,
    n: &Normalized<'a>,
    calls: &[Call],
    budget: &ConstructionBudget,
    structure: roxmltree::Node<'_, '_>,
) -> Result<ContextSeal, String> {
    let mut seal = ContextSeal {
        receivers: Vec::new(),
        budget: Some(budget.clone()),
    };
    for call in calls {
        let Some(closure) = &call.certificate else {
            continue;
        };
        for boundary in &closure.boundaries {
            let target = MainProducerView::boundary(n, boundary, budget)?;
            budget.claim(1, 0, 0)?;
            seal.receivers.push(receiving(
                view, n, closure, boundary, target, budget, structure,
            )?);
        }
    }
    Ok(seal)
}

impl ContextSeal {
    pub(in crate::import) fn audit_target_count(
        &self,
        expected: usize,
        actual: usize,
    ) -> Result<(), String> {
        if !self.receivers.is_empty() && expected != actual {
            return Err("declared-library post-normalization ownership certificate refused changed final target set".into());
        }
        Ok(())
    }
    pub(in crate::import) fn audit(
        &self,
        target: &SchemaComponent,
        root: &mapping::Scope,
        graph: &mapping::Graph,
        calls: &BTreeMap<u32, mapping::NodeId>,
    ) -> Result<(), String> {
        let Some(budget) = &self.budget else {
            return Ok(());
        };
        for receiver in self
            .receivers
            .iter()
            .filter(|receiver| receiver.target == std::ptr::from_ref(target) as usize)
        {
            let fail = || {
                format!(
                    "declared-library post-normalization ownership certificate refused declared output `{}`: final receiving context differs from its sealed authority",
                    receiver.output
                )
            };
            let mut leaf = root;
            for (path, source) in &receiver.scopes {
                let mut scope = root;
                for name in path {
                    budget.claim(scope.children.len().saturating_add(1), 0, path.len())?;
                    let mut children = scope
                        .children
                        .iter()
                        .filter(|child| child.target_field == *name);
                    scope = children.next().ok_or_else(fail)?;
                    if children.next().is_some() {
                        return Err(fail());
                    }
                }
                let iteration_matches = match (&scope.iteration, source) {
                    (mapping::ScopeIteration::None, None) => true,
                    (mapping::ScopeIteration::Source(actual), Some(expected)) => actual == expected,
                    _ => false,
                };
                if !iteration_matches
                    || scope.construction != mapping::ScopeConstruction::Constructed
                    || scope.filter.is_some()
                    || scope.post_group_filter.is_some()
                    || scope.has_grouping()
                    || scope.has_sort()
                    || !scope.windows.is_empty()
                    || !scope.dynamic_bindings.is_empty()
                    || !scope.dynamic_children.is_empty()
                    || scope.merge_dynamic_fields
                {
                    return Err(fail());
                }
                leaf = scope;
            }
            budget.claim(leaf.bindings.len().saturating_add(1), 0, 0)?;
            let mut bindings = leaf
                .bindings
                .iter()
                .filter(|binding| binding.target_field == receiver.field);
            let binding = bindings.next().ok_or_else(fail)?;
            let wanted = calls.get(&receiver.output).ok_or_else(fail)?;
            if bindings.next().is_some()
                || binding.node != *wanted
                || !matches!(
                    graph.nodes.get(wanted),
                    Some(mapping::Node::UserFunctionCall { .. })
                )
            {
                return Err(fail());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_context_seal_rejects_late_driver_name_construction_and_binding_changes() {
        let doc = roxmltree::Document::parse(r#"<component name="Output" library="xml" kind="14"><data><root><entry name="Output"><entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/></entry></entry></root></data></component>"#).unwrap();
        let target = schema::read_schema_component(
            &doc.root_element(),
            std::path::Path::new("scope-test.mfd"),
            &mut Vec::new(),
        )
        .unwrap();
        let root = mapping::Scope {
            children: vec![mapping::Scope {
                target_field: "Row".into(),
                iteration: mapping::ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![mapping::Binding {
                    target_field: "Result".into(),
                    node: 1,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut graph = mapping::Graph::default();
        graph.nodes.insert(
            1,
            mapping::Node::UserFunctionCall {
                function: mapping::FunctionId::new(1),
                args: Vec::new(),
            },
        );
        graph.nodes.insert(
            2,
            mapping::Node::Const {
                value: ir::Value::Null,
            },
        );
        let calls = BTreeMap::from([(21, 1)]);
        let seal = ContextSeal {
            receivers: vec![Receiver {
                target: std::ptr::from_ref(&target) as usize,
                output: 21,
                field: "Result".into(),
                scopes: vec![
                    (Vec::new(), None),
                    (vec!["Row".into()], Some(vec!["Row".into()])),
                ],
            }],
            budget: Some(ConstructionBudget::default()),
        };
        seal.audit(&target, &root, &graph, &calls).unwrap();
        for change in 0..5 {
            let mut changed = root.clone();
            match change {
                0 => {
                    changed.children[0].iteration =
                        mapping::ScopeIteration::Source(vec!["OtherRow".into()])
                }
                1 => changed.children[0].target_field = "Other".into(),
                2 => {
                    changed.children[0].construction = mapping::ScopeConstruction::CopyCurrentSource
                }
                3 => changed.children[0].filter = Some(2),
                _ => changed.children[0].bindings[0].node = 2,
            }
            assert!(seal.audit(&target, &changed, &graph, &calls).is_err());
        }
        assert!(seal.audit_target_count(1, 0).is_err());
        graph.nodes.insert(
            1,
            mapping::Node::Const {
                value: ir::Value::Null,
            },
        );
        assert!(seal.audit(&target, &root, &graph, &calls).is_err());
    }
}
