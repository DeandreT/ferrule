//! Conservative proof for global pre-target failures represented by native
//! per-item exception branches. This proves mapping-error priority only for
//! schema-conforming XML inputs; publication, I/O and resource failures remain
//! outside the assessment. Unknown cases keep their extension round-trip.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    FailureIteration, FailureSelection, FormatOptions, IterationOutput, Node, NodeId, Project,
    Scope, ScopeConstruction, ScopeIteration,
};

use super::compatibility::{ExportCompatibilityFeature, ExportCompatibilityIssue};
use super::schema::{SideFormat, side_format};

const MAX_PROOF_NODES: usize = 4096;
const MAX_PROOF_DEPTH: usize = 128;
const MAX_PROOF_WORK: usize = 100_000;

pub(super) fn issue(project: &Project) -> Option<ExportCompatibilityIssue> {
    if project.failure_rules.is_empty() {
        return None;
    }
    prove(project).err().map(|reason| ExportCompatibilityIssue {
        feature: ExportCompatibilityFeature::GlobalFailureOrdering,
        component: "defaultmap".into(),
        component_uid: Some(1),
        message: format!("global pre-target failure ordering is not proved: {reason}"),
    })
}

pub(super) fn pipeline_issue(pipeline: &mapping::Pipeline) -> Option<ExportCompatibilityIssue> {
    pipeline
        .stages
        .iter()
        .find(|stage| !stage.project.failure_rules.is_empty())
        .map(|stage| ExportCompatibilityIssue {
            feature: ExportCompatibilityFeature::GlobalFailureOrdering,
            component: format!("pipeline stage `{}`", stage.id),
            component_uid: None,
            message: "global pre-target failure ordering across connected stages is not proved"
                .into(),
        })
}

fn prove(project: &Project) -> Result<(), String> {
    let [rule] = project.failure_rules.as_slice() else {
        return Err("multiple rules are scanned rule-first, not item-first".into());
    };
    if !project.extra_sources.is_empty() || !project.extra_targets.is_empty() {
        return Err(
            "additional source or target boundaries are outside the total-target proof".into(),
        );
    }
    if !project.user_functions.is_empty()
        || project.graph.nodes.len() > MAX_PROOF_NODES
        || !bounded_schema(&project.source)
        || !bounded_schema(&project.target)
        || !bounded_scope(&project.root)
    {
        return Err("the totality proof budget or supported registry subset was exceeded".into());
    }
    if !local_xml(&project.source_path, &project.source_options)
        || !local_xml(&project.target_path, &project.target_options)
        || !plain_schema(&project.source)
        || !plain_schema(&project.target)
        || project.source.repeating
        || project.target.repeating
    {
        return Err("requires local, closed plain String/Int/Bool XML boundaries".into());
    }
    let FailureIteration::Source { collection } = &rule.iteration else {
        return Err("generated failure iterations are not proved".into());
    };
    let [name] = collection.as_slice() else {
        return Err("requires one directly repeated primary collection".into());
    };
    let item = child(&project.source, name)
        .filter(|node| node.repeating)
        .ok_or_else(|| "the failure collection must be a declared primary repeater".to_string())?;
    if !matches!(item.kind, SchemaKind::Group { .. }) {
        return Err("the failure collection must contain groups".into());
    }
    let predicate = match rule.selection {
        FailureSelection::WhenFalse { predicate } | FailureSelection::WhenTrue { predicate } => {
            predicate
        }
        FailureSelection::All => return Err("unconditional exceptions are not proved".into()),
    };
    let proof = Proof {
        project,
        collection,
        item,
        cached: RefCell::new(BTreeMap::new()),
        work: Cell::new(0),
    };
    let predicate_type = proof.expression(predicate, true)?;
    if predicate_type != Domain::required(ScalarType::Bool) {
        return Err(format!("predicate node {predicate} is not a total Boolean"));
    }
    if let Some(message) = rule.message {
        proof.expression(message, true)?;
    }
    let mut repeated = 0;
    proof.scope(
        &project.root,
        &project.target,
        false,
        predicate,
        rule.selection,
        &mut repeated,
    )?;
    if repeated != 1 {
        return Err("requires exactly one plain repeated target branch".into());
    }
    // The ordinary renderer also emits disconnected graph nodes. Do not assume
    // target reachability alone makes their runtime behavior irrelevant.
    for &node in project.graph.nodes.keys() {
        proof.expression(node, true)?;
    }
    if !engine::validate(project).is_empty() {
        return Err("the complete project must pass engine validation".into());
    }
    Ok(())
}

fn bounded_schema(root: &SchemaNode) -> bool {
    let mut pending = vec![(root, 1usize)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if count > MAX_PROOF_NODES || depth > MAX_PROOF_DEPTH {
            return false;
        }
        if let SchemaKind::Group { children, .. } = &node.kind {
            for child in children {
                if count + pending.len() >= MAX_PROOF_NODES {
                    return false;
                }
                pending.push((child, depth + 1));
            }
        }
    }
    true
}

fn bounded_scope(root: &Scope) -> bool {
    let mut pending = vec![(root, 1usize)];
    let mut count = 0;
    while let Some((scope, depth)) = pending.pop() {
        count += 1;
        if count > MAX_PROOF_NODES || depth > MAX_PROOF_DEPTH {
            return false;
        }
        for child in scope
            .children
            .iter()
            .chain(scope.dynamic_children.iter().map(|child| &child.scope))
        {
            if count + pending.len() >= MAX_PROOF_NODES {
                return false;
            }
            pending.push((child, depth + 1));
        }
        if let Some(segments) = scope.concatenated() {
            for segment in segments.iter() {
                if count + pending.len() >= MAX_PROOF_NODES {
                    return false;
                }
                pending.push((segment, depth + 1));
            }
        }
    }
    true
}

fn local_xml(path: &Option<String>, options: &FormatOptions) -> bool {
    path.as_deref().is_some_and(|path| {
        !path.is_empty() && !path.contains("://") && !path.contains('*') && !path.contains('?')
    }) && side_format(path, options) == SideFormat::Xml
        && *options
            == (FormatOptions {
                xml_document: options.xml_document,
                ..FormatOptions::default()
            })
}

// Comparing with a freshly constructed plain shape makes additional metadata
// fail closed, including future schema assertions. Only ordinary optional
// singular XML elements are included in this proof.
fn plain_schema(node: &SchemaNode) -> bool {
    let mut plain = match &node.kind {
        SchemaKind::Scalar { ty }
            if matches!(ty, ScalarType::String | ScalarType::Int | ScalarType::Bool) =>
        {
            SchemaNode::scalar(&node.name, *ty)
        }
        SchemaKind::Group { children, .. } => {
            if !children.iter().all(plain_schema) {
                return false;
            }
            let names: BTreeSet<_> = children.iter().map(|node| &node.name).collect();
            if names.len() != children.len() {
                return false;
            }
            SchemaNode::group(&node.name, children.clone())
        }
        _ => return false,
    };
    plain.repeating = node.repeating;
    plain.xml_optional = node.xml_optional;
    *node == plain
}

fn child<'a>(node: &'a SchemaNode, name: &str) -> Option<&'a SchemaNode> {
    let SchemaKind::Group { children, .. } = &node.kind else {
        return None;
    };
    children.iter().find(|child| child.name == name)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Domain {
    ty: ScalarType,
    present: bool,
}

impl Domain {
    fn required(ty: ScalarType) -> Self {
        Self { ty, present: true }
    }
}

struct Proof<'a> {
    project: &'a Project,
    collection: &'a [String],
    item: &'a SchemaNode,
    cached: RefCell<BTreeMap<(NodeId, bool), (Domain, usize)>>,
    work: Cell<usize>,
}

impl Proof<'_> {
    fn expression(&self, node: NodeId, in_item: bool) -> Result<Domain, String> {
        self.expression_inner(node, in_item, &mut BTreeSet::new())
    }

    fn expression_inner(
        &self,
        id: NodeId,
        in_item: bool,
        visiting: &mut BTreeSet<NodeId>,
    ) -> Result<Domain, String> {
        let work = self.work.get() + 1;
        self.work.set(work);
        if work > MAX_PROOF_WORK || visiting.len() >= MAX_PROOF_DEPTH {
            return Err("the total scalar proof budget was exceeded".into());
        }
        if let Some((domain, _)) = self.cached.borrow().get(&(id, in_item)).copied() {
            return Ok(domain);
        }
        if !visiting.insert(id) {
            return Err(format!("node {id} has a dependency cycle"));
        }
        let result = (|| match self.project.graph.nodes.get(&id) {
            Some(Node::Const {
                value: Value::String(value),
            }) if xml_text(value) => Ok(Domain::required(ScalarType::String)),
            Some(Node::Const {
                value: Value::Int(_),
            }) => Ok(Domain::required(ScalarType::Int)),
            Some(Node::Const {
                value: Value::Bool(_),
            }) => Ok(Domain::required(ScalarType::Bool)),
            Some(Node::SourceField {
                path,
                frame: Some(frame),
            }) if in_item && frame.as_slice() == self.collection => {
                let mut schema = self.item;
                let mut present = true;
                for name in path {
                    schema = child(schema, name)
                        .filter(|schema| !schema.repeating)
                        .ok_or_else(|| {
                            format!("node {id} does not read one declared singular item field")
                        })?;
                    present &= !schema.xml_optional;
                }
                match &schema.kind {
                    SchemaKind::Scalar { ty } => Ok(Domain { ty: *ty, present }),
                    _ => Err(format!("node {id} does not read a scalar field")),
                }
            }
            Some(Node::Position { collection })
                if in_item && collection.as_slice() == self.collection =>
            {
                Ok(Domain::required(ScalarType::Int))
            }
            Some(Node::Call { function, args }) if function == "not" && args.len() == 1 => {
                let argument = self.expression_inner(args[0], in_item, visiting)?;
                if argument == Domain::required(ScalarType::Bool) {
                    Ok(argument)
                } else {
                    Err(format!("node {id} requires a present Boolean argument"))
                }
            }
            Some(Node::Call { function, args })
                if matches!(
                    function.as_str(),
                    "equal"
                        | "not_equal"
                        | "less_than"
                        | "greater_than"
                        | "less_or_equal"
                        | "greater_or_equal"
                ) && args.len() == 2 =>
            {
                let first = self.expression_inner(args[0], in_item, visiting)?;
                let second = self.expression_inner(args[1], in_item, visiting)?;
                if first == Domain::required(ScalarType::Int) && second == first {
                    Ok(Domain::required(ScalarType::Bool))
                } else {
                    Err(format!(
                        "comparison node {id} requires two present Int arguments"
                    ))
                }
            }
            _ => Err(format!("node {id} is outside the total scalar subset")),
        })();
        visiting.remove(&id);
        if let Ok(domain) = &result {
            let height = {
                let cached = self.cached.borrow();
                self.project.graph.nodes[&id]
                    .dependencies()
                    .iter()
                    .map(|dependency| cached[&(*dependency, in_item)].1)
                    .max()
                    .unwrap_or(0)
                    + 1
            };
            if height > MAX_PROOF_DEPTH {
                return Err("the total scalar proof budget was exceeded".into());
            }
            self.cached
                .borrow_mut()
                .insert((id, in_item), (*domain, height));
        }
        result
    }

    fn scope(
        &self,
        scope: &Scope,
        target: &SchemaNode,
        in_item: bool,
        predicate: NodeId,
        selection: FailureSelection,
        repeated: &mut usize,
    ) -> Result<(), String> {
        if scope.construction != ScopeConstruction::Constructed
            || scope.post_group_filter.is_some()
            || scope.grouping_nodes().next().is_some()
            || scope.sort_by.is_some()
            || !scope.sort_then_by.is_empty()
            || !scope.windows.is_empty()
            || !scope.dynamic_bindings.is_empty()
            || !scope.dynamic_children.is_empty()
            || scope.merge_dynamic_fields
        {
            return Err(format!(
                "target scope `{}` has an unproved construction or control",
                scope.target_field
            ));
        }
        let here = match &scope.iteration {
            ScopeIteration::None
                if scope.filter.is_none()
                    && scope.iteration_output == IterationOutput::Repeated =>
            {
                in_item
            }
            ScopeIteration::Source(collection)
                if !in_item
                    && collection.as_slice() == self.collection
                    && scope.iteration_output == IterationOutput::Repeated
                    && target.repeating =>
            {
                let keeps_predicate = match selection {
                    FailureSelection::WhenFalse { .. } => scope.filter == Some(predicate),
                    FailureSelection::WhenTrue { .. } => scope.filter.is_some_and(|filter| matches!(self.project.graph.nodes.get(&filter), Some(Node::Call { function, args }) if function == "not" && args.as_slice() == [predicate])),
                    FailureSelection::All => false,
                };
                if !keeps_predicate {
                    return Err(
                        "target filter must be the exact complementary failure branch".into(),
                    );
                }
                *repeated += 1;
                true
            }
            _ => {
                return Err(format!(
                    "target scope `{}` has an unproved iteration or filter",
                    scope.target_field
                ));
            }
        };
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for binding in &scope.bindings {
            if !names.insert(binding.target_field.as_str()) {
                return Err("duplicate target bindings are not proved".into());
            }
            let leaf = child(target, &binding.target_field)
                .filter(|leaf| !leaf.repeating)
                .ok_or_else(|| {
                    format!(
                        "target binding `{}` does not own one singular leaf",
                        binding.target_field
                    )
                })?;
            let SchemaKind::Scalar { ty } = &leaf.kind else {
                return Err("target binding must be scalar".into());
            };
            let domain = self.expression(binding.node, here)?;
            if domain.ty != *ty || !domain.present && !leaf.xml_optional {
                return Err(format!(
                    "target binding `{}` requires unproved coercion or presence",
                    binding.target_field
                ));
            }
        }
        for nested in &scope.children {
            if !names.insert(nested.target_field.as_str()) {
                return Err("duplicate target children are not proved".into());
            }
            let nested_schema = child(target, &nested.target_field)
                .ok_or_else(|| "target child has no declared schema".to_string())?;
            self.scope(nested, nested_schema, here, predicate, selection, repeated)?;
        }
        let SchemaKind::Group { children, .. } = &target.kind else {
            return Err("target scopes must construct groups".into());
        };
        if children.iter().any(|child| {
            !child.repeating && !child.xml_optional && !names.contains(child.name.as_str())
        }) {
            return Err("required target fields must have proved bindings or children".into());
        }
        Ok(())
    }
}

fn xml_text(value: &str) -> bool {
    value.chars().all(|character| matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'))
}
