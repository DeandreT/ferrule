//! Position selector ownership and native admission audit.

use std::collections::BTreeSet;

/// Audits each reachable use against the interpreter's recorded selector paths.
/// Schema-canonical paths alone are insufficient: a nested source selector
/// `Row` records `Row`, so `Batch/Row` remains inactive and evaluates to one.
/// A known inactive use blocks native admission and disables graph rewriting.
pub(in crate::export) fn selector_context_warnings(
    project: &mapping::Project,
    sources: &super::super::source::SourceExports<'_>,
    bridge_required: bool,
) -> Vec<String> {
    use mapping::{Node, NodeId, Scope, ScopeConstruction};
    struct Audit<'a, 'b> {
        project: &'a mapping::Project,
        sources: &'b super::super::source::SourceExports<'a>,
        warnings: BTreeSet<String>,
        bridge_required: bool,
    }
    impl Audit<'_, '_> {
        fn record_source_frames(
            &self,
            path: &[String],
            anchor: &[String],
            frames: &mut Vec<Vec<String>>,
        ) -> Vec<String> {
            let absolute = if self.sources.is_named_extra_path(path) {
                path.to_vec()
            } else {
                self.sources.resolve_scope_path(anchor, path).0
            };
            let prefix_len = absolute.len().saturating_sub(path.len());
            for len in 1..=path.len() {
                if self
                    .sources
                    .schema_node_at(&absolute[..prefix_len + len])
                    .is_some_and(|node| node.repeating)
                {
                    frames.push(path[..len].to_vec());
                }
            }
            absolute
        }
        fn source_frames(
            &self,
            path: &[String],
            anchor: &[String],
            frames: &mut Vec<Vec<String>>,
        ) -> Vec<String> {
            if let Some(name) = path.first()
                && let Some(source) = self
                    .project
                    .extra_sources
                    .iter()
                    .find(|source| source.name == *name)
            {
                if let Some(dynamic) = &source.dynamic_path {
                    self.record_source_frames(&dynamic.iteration, anchor, frames);
                }
                if let Some(export) = self
                    .sources
                    .iter()
                    .skip(1)
                    .find(|source| source.name == name)
                {
                    let flat_rows = matches!(
                        export.format,
                        super::super::schema::SideFormat::Csv
                            | super::super::schema::SideFormat::FixedWidth
                    ) || export.format == super::super::schema::SideFormat::Xlsx
                        && matches!(&export.schema.kind, ir::SchemaKind::Group { children, .. } if children.iter().all(|child| child.is_scalar()));
                    if !export.schema.repeating && (flat_rows || export.options.local_xml_file_set)
                    {
                        frames.push(vec![name.clone()]);
                    }
                }
            }
            // Relative walks restart their recorded prefix. Named inputs keep
            // their explicit source-name segment, including flat-row roots.
            self.record_source_frames(path, anchor, frames)
        }
        fn expression(
            &mut self,
            id: NodeId,
            frames: &[Vec<String>],
            anchor: &[String],
            usage: &str,
            private_frames: &BTreeSet<usize>,
            active: &mut BTreeSet<NodeId>,
        ) {
            if active.len() >= 256 || !active.insert(id) {
                self.warnings.insert(format!("position selector context at {usage} is cyclic or exceeds 256 expression levels"));
                return;
            }
            let Some(node) = self.project.graph.nodes.get(&id) else {
                active.remove(&id);
                return;
            };
            let mut parent = Vec::new();
            let mut private = Vec::new();
            let mut owned = frames.to_vec();
            let mut owned_anchor = anchor.to_vec();
            let mut private_xml = false;
            match node {
                Node::Position { collection } => {
                    let owner = frames
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, frame)| collection.is_empty() || frame.ends_with(collection))
                        .map(|(index, _)| index);
                    if !collection.is_empty() && owner.is_none() {
                        self.warnings.insert(format!("position node {id} selector `{}` is inactive at {usage}; the engine returns 1, native sequence-context equivalence is not qualified for this use", collection.join("/")));
                    }
                    if self.bridge_required
                        && owner.is_some_and(|index| private_frames.contains(&index))
                    {
                        self.warnings.insert(format!("position node {id} at {usage} resolves to a private XML item context; the typed XML scope-sequence bridge has not qualified this reducer ownership"));
                    }
                }
                Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => {
                    parent.extend(args)
                }
                Node::RuntimeParameterDefault { default, .. } => parent.push(*default),
                Node::If {
                    condition,
                    then,
                    else_,
                } => parent.extend([*condition, *then, *else_]),
                Node::ValueMap { input, .. } => parent.push(*input),
                Node::Lookup { matches, .. } => parent.push(*matches),
                Node::DynamicSourceField { key, .. } => parent.push(*key),
                Node::XmlMixedContent { replacements, .. } => {
                    for replacement in replacements {
                        let mut item = frames.to_vec();
                        let item_anchor =
                            self.source_frames(&replacement.collection, anchor, &mut item);
                        let mut item_private = private_frames.clone();
                        item_private.extend(frames.len()..item.len());
                        self.expression(
                            replacement.expression,
                            &item,
                            &item_anchor,
                            usage,
                            &item_private,
                            active,
                        );
                    }
                }
                Node::CollectionFind {
                    collection,
                    predicate,
                    value,
                } => {
                    private_xml = true;
                    owned_anchor = self.source_frames(collection, anchor, &mut owned);
                    private.extend([*predicate, *value]);
                }
                Node::Aggregate {
                    collection,
                    expression,
                    arg,
                    ..
                } => {
                    private_xml = true;
                    owned_anchor = self.source_frames(collection, anchor, &mut owned);
                    private.extend(expression);
                    parent.extend(arg);
                }
                Node::JoinAggregate {
                    plan,
                    expression,
                    arg,
                    ..
                } => {
                    private_xml = true;
                    for source in plan.sources() {
                        self.source_frames(source.collection(), anchor, &mut owned);
                    }
                    private.extend(expression);
                    parent.extend(arg);
                }
                Node::SequenceExists {
                    sequence,
                    predicate,
                } => {
                    parent.extend(sequence.inputs());
                    owned.push(Vec::new());
                    private.push(*predicate);
                }
                Node::SequenceItemAt { sequence, index } => {
                    parent.extend(sequence.inputs());
                    parent.push(*index);
                }
                Node::SequenceAggregate {
                    sequence,
                    predicate,
                    expression,
                    arg,
                    ..
                } => {
                    parent.extend(sequence.inputs());
                    parent.extend(arg);
                    owned.push(Vec::new());
                    private.extend(predicate);
                    private.extend(expression);
                }
                Node::SourceField { .. }
                | Node::SourceDocumentPath
                | Node::JoinField { .. }
                | Node::JoinPosition { .. }
                | Node::Unconnected
                | Node::Const { .. }
                | Node::FunctionParameter { .. }
                | Node::RuntimeValue { .. }
                | Node::RuntimeParameter { .. }
                | Node::XmlSerialize { .. } => {}
            }
            for input in parent {
                self.expression(input, frames, anchor, usage, private_frames, active);
            }
            let mut owned_private = private_frames.clone();
            if private_xml {
                owned_private.extend(frames.len()..owned.len());
            }
            for input in private {
                self.expression(input, &owned, &owned_anchor, usage, &owned_private, active);
            }
            active.remove(&id);
        }
        fn check(
            &mut self,
            roots: impl IntoIterator<Item = NodeId>,
            frames: &[Vec<String>],
            anchor: &[String],
            usage: &str,
        ) {
            for id in roots {
                self.expression(
                    id,
                    frames,
                    anchor,
                    usage,
                    &BTreeSet::new(),
                    &mut BTreeSet::new(),
                );
            }
        }
        fn scope(
            &mut self,
            scope: &Scope,
            parent: &[Vec<String>],
            anchor: &[String],
            target: &str,
            depth: usize,
        ) {
            if depth >= 256 {
                self.warnings.insert(format!(
                    "position selector context at {target} exceeds 256 scope levels"
                ));
                return;
            }
            if let Some(segments) = scope.concatenated() {
                for (index, segment) in segments.iter().enumerate() {
                    self.scope(
                        segment,
                        parent,
                        anchor,
                        &format!("{target}/segment-{}", index + 1),
                        depth + 1,
                    );
                }
                return;
            }
            let mut frames = parent.to_vec();
            let mut current = anchor.to_vec();
            if let Some(source) = scope.source() {
                current = self.source_frames(source, anchor, &mut frames);
            }
            if let Some(sequence) = scope.sequence() {
                self.check(
                    sequence.inputs(),
                    parent,
                    anchor,
                    &format!("{target} sequence inputs"),
                );
                frames.push(Vec::new());
            }
            if let Some((_, plan)) = scope.join() {
                for source in plan.sources() {
                    self.source_frames(source.collection(), anchor, &mut frames);
                }
            }
            self.check(
                scope
                    .filter
                    .into_iter()
                    .chain(scope.post_group_filter)
                    .chain(scope.sort_keys().map(|key| key.node))
                    .chain(
                        [
                            scope.group_by,
                            scope.group_adjacent_by,
                            scope.group_starting_with,
                            scope.group_ending_with,
                        ]
                        .into_iter()
                        .flatten(),
                    ),
                &frames,
                &current,
                &format!("{target} item controls"),
            );
            self.check(
                scope
                    .windows
                    .iter()
                    .flat_map(|window| window.nodes())
                    .chain(scope.group_into_blocks),
                parent,
                anchor,
                &format!("{target} parent bounds"),
            );
            self.check(
                scope
                    .bindings
                    .iter()
                    .map(|binding| binding.node)
                    .chain(
                        scope
                            .dynamic_bindings
                            .iter()
                            .flat_map(|binding| [binding.key, binding.value]),
                    )
                    .chain(scope.output_path()),
                &frames,
                &current,
                &format!("{target} output"),
            );
            if let ScopeConstruction::Scalar { value } = scope.construction {
                self.check(
                    [value],
                    &frames,
                    &current,
                    &format!("{target} scalar output"),
                );
            }
            for child in &scope.children {
                self.scope(
                    child,
                    &frames,
                    &current,
                    &format!("{target}/{}", child.target_field),
                    depth + 1,
                );
            }
            for child in &scope.dynamic_children {
                self.check(
                    [child.key],
                    &frames,
                    &current,
                    &format!("{target} dynamic child name"),
                );
                self.scope(
                    &child.scope,
                    &frames,
                    &current,
                    &format!("{target}/dynamic-child"),
                    depth + 1,
                );
            }
        }
    }
    let mut audit = Audit {
        project,
        sources,
        warnings: BTreeSet::new(),
        bridge_required,
    };
    audit.scope(&project.root, &[], &[], "primary", 0);
    for target in &project.extra_targets {
        audit.scope(
            &target.root,
            &[],
            &[],
            &format!("target `{}`", target.name),
            0,
        );
    }
    for source in &project.extra_sources {
        if let Some(dynamic) = &source.dynamic_path {
            let mut frames = Vec::new();
            let anchor = audit.source_frames(&dynamic.iteration, &[], &mut frames);
            audit.check(
                [dynamic.node],
                &frames,
                &anchor,
                &format!("source `{}` path", source.name),
            );
        }
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let mut frames = Vec::new();
        let mut anchor = Vec::new();
        match &rule.iteration {
            mapping::FailureIteration::Source { collection } => {
                anchor = audit.source_frames(collection, &[], &mut frames)
            }
            mapping::FailureIteration::Sequence { sequence } => {
                audit.check(
                    sequence.inputs(),
                    &[],
                    &[],
                    &format!("failure {} sequence inputs", index + 1),
                );
                frames.push(Vec::new());
            }
        }
        audit.check(
            rule.selection.predicate().into_iter().chain(rule.message),
            &frames,
            &anchor,
            &format!("failure {} item", index + 1),
        );
    }
    audit.warnings.into_iter().collect()
}
