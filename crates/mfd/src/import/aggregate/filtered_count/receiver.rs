use super::*;
use mapping::{Scope, ScopeConstruction, ScopeIteration};

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Mode {
    CurrentItem,
    Sum(Vec<String>),
}
pub(super) struct ReceiverProof {
    pub mode: Mode,
    pub facts: Vec<Fact>,
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Fact {
    pub path: Vec<String>,
    pub source: Option<Vec<String>>,
    pub output: mapping::IterationOutput,
    pub sort_descending: bool,
    pub sort_filter_order: mapping::SortFilterOrder,
}
struct Frame {
    canonical: Vec<String>,
    label: Vec<String>,
    occurrence: usize,
}
struct Instances {
    owners: Vec<Vec<String>>,
    frames: Vec<Frame>,
}

pub(super) fn ordinary(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
        && matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Source(_)
        )
}

fn walk(
    schema: &SchemaNode,
    instances: &mut Instances,
    base: &[String],
    driver: &[String],
    scalar: bool,
    budget: &CountConstruction,
) -> Result<(), String> {
    checked_path(driver, budget)?;
    if driver.is_empty() {
        return Ok(());
    }
    let path = joined_path(base, &driver[..1], budget)?;
    if !exact_node(schema, &path, budget)?.repeating {
        return Err("singleton-before-repeat receiver/Count driver".into());
    }
    for length in 1..=driver.len() {
        let absolute = joined_path(base, &driver[..length], budget)?;
        let node = exact_node(schema, &absolute, budget)?;
        if length == driver.len()
            && (!node.repeating || !scalar && !matches!(node.kind, SchemaKind::Group { .. }))
        {
            return Err("unmodelled terminal receiver instance".into());
        }
        if node.repeating {
            if instances.owners.contains(&absolute) {
                return Err("unproved repeated owner re-entry".into());
            }
            budget.claim(2, 0, 0)?;
            let occurrence = instances.owners.len();
            instances.frames.push(Frame {
                canonical: copy_path(&absolute, budget)?,
                label: copy_path(&driver[..length], budget)?,
                occurrence,
            });
            instances.owners.push(absolute);
        }
    }
    Ok(())
}

pub(super) fn prove(
    schema: &SchemaNode,
    plan: &Plan,
    use_: &DirectUse,
    root: &Scope,
    anchors: Option<&BTreeMap<Vec<String>, Vec<String>>>,
    budget: &CountConstruction,
) -> Result<ReceiverProof, String> {
    count_visit(Counter::Receiver);
    let mut instances = Instances {
        owners: vec![Vec::new()],
        frames: Vec::new(),
    };
    let mut facts = Vec::new();
    let mut scope = Some(root);
    for length in 0..use_.path.len() {
        budget.claim(1, 0, length)?;
        let prefix = &use_.path[..length];
        let current = scope;
        let mut source = None;
        let mut output = mapping::IterationOutput::default();
        let mut sort_descending = false;
        let mut sort_filter_order = mapping::SortFilterOrder::default();
        if let Some(current) = current {
            if !ordinary(current) {
                return Err("nonordinary Count receiver control/construction".into());
            }
            output = current.iteration_output;
            sort_descending = current.sort_descending;
            sort_filter_order = current.sort_filter_order;
            if let ScopeIteration::Source(driver) = &current.iteration {
                let intended = use_
                    .drivers
                    .get(prefix)
                    .ok_or("receiver lacks a raw direct source driver")?;
                let parent = instances.owners.last().ok_or("missing receiver instance")?;
                let expected = joined_path(parent, driver, budget)?;
                if &expected != intended {
                    return Err("receiver driver does not select its intended raw owner".into());
                }
                if anchors
                    .and_then(|anchors| anchors.get(prefix))
                    .is_some_and(|anchor| anchor != intended)
                {
                    return Err("receiver anchor disagrees with proved owner".into());
                }
                let base = copy_path(parent, budget)?;
                walk(schema, &mut instances, &base, driver, false, budget)?;
                source = Some(copy_path(driver, budget)?);
            } else if use_.drivers.contains_key(prefix) {
                return Err("raw receiver driver was not installed".into());
            }
        } else if use_.drivers.contains_key(prefix) {
            return Err("missing connected receiver scope".into());
        }
        budget.claim(1, 0, length)?;
        facts.push(Fact {
            path: copy_path(prefix, budget)?,
            source,
            output,
            sort_descending,
            sort_filter_order,
        });
        if length + 1 < use_.path.len() {
            scope = if let Some(current) = current {
                budget.claim(current.children.len(), 0, 0)?;
                let mut children = current
                    .children
                    .iter()
                    .filter(|c| c.target_field == use_.path[length]);
                let found = children.next();
                if children.next().is_some() {
                    return Err("ambiguous receiver scope occurrence".into());
                }
                found
            } else {
                None
            };
        }
    }
    let intended = instances
        .owners
        .iter()
        .enumerate()
        .rev()
        .find(|(_, owner)| plan.owner.starts_with(owner.as_slice()))
        .map(|(i, _)| i)
        .ok_or("no Count base owner")?;
    let base = copy_path(&instances.owners[intended], budget)?;
    let mode = if base == plan.owner {
        Mode::CurrentItem
    } else {
        let driver = copy_path(&plan.owner[base.len()..], budget)?;
        let first = driver.first().ok_or("empty Count aggregate walk")?;
        for nearer in &instances.owners[intended + 1..] {
            let node = exact_node(schema, nearer, budget)?;
            let SchemaKind::Group { children, .. } = &node.kind else {
                return Err("non-group nearer receiver".into());
            };
            budget.claim(children.len() + 1, first.len(), 0)?;
            if children.iter().any(|n| n.name == *first) {
                return Err("nearer receiver can shadow Count walk".into());
            }
        }
        walk(
            schema,
            &mut instances,
            &base,
            &driver,
            plan.repeated_scalar,
            budget,
        )?;
        Mode::Sum(driver)
    };
    let occurrence = instances
        .owners
        .iter()
        .position(|owner| *owner == plan.owner)
        .ok_or("missing counted occurrence")?;
    budget.claim(
        instances.frames.len(),
        plan.owner.iter().map(String::len).sum(),
        0,
    )?;
    let latest = instances
        .frames
        .iter()
        .rev()
        .find(|frame| {
            frame.label == plan.owner
                || !frame.label.is_empty() && plan.owner.ends_with(&frame.label)
        })
        .ok_or("absolute Count pin has no matching frame")?;
    if latest.canonical != plan.owner || latest.occurrence != occurrence {
        return Err("latest legacy pin captures a different owner occurrence".into());
    }
    Ok(ReceiverProof { mode, facts })
}

pub(super) fn root(
    schema: &SchemaNode,
    plan: &Plan,
    budget: &CountConstruction,
) -> Result<ReceiverProof, String> {
    let use_ = DirectUse {
        candidate: 0,
        path: vec![String::new()],
        drivers: BTreeMap::new(),
    };
    prove(schema, plan, &use_, &Scope::default(), None, budget)
}
