//! Computed join keys (docs/design/join-computed-keys.md, scope 5): a join
//! input fed by the output of the key's own constant, runtime parameter or
//! call chain instead of a source port.
//!
//! Export admits exactly the keys MFD import produces. Import lowers a key
//! that reads source fields under a null guard, one per field, because
//! ferrule's `upper` refuses `Null` and its `concat` treats it as `""` while
//! the native functions return an empty sequence. The native design needs no
//! guard, so export writes only the call chain below it, and import adds the
//! same guard again on re-import. Every other key is refused, so the whole
//! export fails before anything is written.
//!
//! A sequence-valued key is written as the `tokenize` component import
//! admits, its input guarded and written the same way, its delimiter a
//! non-empty string constant.

use std::collections::BTreeSet;

use ir::{ScalarType, SchemaKind, Value};
use mapping::{
    CollectionJoinSource, ComputedJoinValue, Graph, JoinPlan, JoinSourceCardinality, Node, NodeId,
    Project, Scope, ScopeRoot, ScopeRootSlot, SequenceExpr,
};

use super::super::schema::PortMatch;
use super::super::source::SourceExports;

use crate::MfdError;

/// The bound import puts on the components one key may evaluate.
const MAX_KEY_COMPONENTS: usize = 64;

/// How one admitted computed key is written.
pub(super) struct ComputedKey {
    /// The node whose output feeds the join input: the key without its
    /// null guard, or the item of a sequence-valued key, whose output is the
    /// written `tokenize` component's.
    pub(super) value: NodeId,
    /// The `tokenize` component a sequence-valued key writes.
    pub(super) tokenize: Option<TokenizeKey>,
    /// The guard's own nodes, which are never written.
    pub(super) guard: BTreeSet<NodeId>,
    /// The input entry name: the parameter's name for a parameter, as
    /// observed natively, and `value` otherwise.
    pub(super) input_name: String,
    /// A parameter feeds the input through a copy-all connection; a function
    /// or constant through an ordinary one, as the native validation requires.
    pub(super) copy_all: bool,
}

/// The nodes feeding a sequence-valued key's `tokenize` component.
#[derive(Clone, Copy)]
pub(super) struct TokenizeKey {
    /// The string input, without its null guard.
    pub(super) input: NodeId,
    pub(super) delimiter: NodeId,
}

/// Refuses every computed join key outside an ordinary target iteration of
/// the primary target: MFD import lowers a key only there, so no other place
/// would re-import the same plan.
pub(in crate::export) fn validate_computed_placement(project: &Project) -> Result<(), MfdError> {
    let refusal = |place: &str| {
        Err(MfdError::Unsupported(format!(
            "a join with a computed key {place} cannot be exported to .mfd; import admits a computed key only for a join that drives an iteration of the primary target"
        )))
    };
    if project.graph.nodes.values().any(
        |node| matches!(node, Node::JoinAggregate { plan, .. } if plan.computed_source().is_some()),
    ) {
        return refusal("feeding a joined aggregate");
    }
    if project.user_functions.values().any(|function| {
        function.body.nodes.values().any(|node| {
            matches!(node, Node::JoinAggregate { plan, .. } if plan.computed_source().is_some())
        })
    }) {
        return refusal("in a user-defined function");
    }
    if project
        .extra_targets
        .iter()
        .any(|target| scope_has_computed_join(&target.root))
    {
        return refusal("in an additional target");
    }
    if outside_child_chain(&project.root) {
        return refusal("in a concatenated or dynamic scope");
    }
    Ok(())
}

fn scope_has_computed_join(scope: &Scope) -> bool {
    scope
        .join()
        .is_some_and(|(_, plan)| plan.computed_source().is_some())
        || scope
            .nested_scopes()
            .any(|(_, nested)| scope_has_computed_join(nested))
}

/// Whether a computed join sits where the join export does not look: below
/// a concatenated segment or a dynamic child.
fn outside_child_chain(scope: &Scope) -> bool {
    scope
        .concatenated()
        .is_some_and(|segments| segments.iter().any(scope_has_computed_join))
        || scope
            .dynamic_children
            .iter()
            .any(|child| scope_has_computed_join(&child.scope))
        || scope.children.iter().any(outside_child_chain)
}

/// Admits the computed key of `plan`, evaluated in the receiving context of
/// a source iteration over `anchor` (empty at the root), or gives the reason
/// the join is refused.
pub(super) fn admit(
    project: &Project,
    plan: &JoinPlan,
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<ComputedKey, String> {
    let computed = plan
        .computed_source()
        .ok_or("the join has no computed key")?;
    let collections = plan.collection_sources().collect::<Vec<_>>();
    let ([collection], 2) = (collections.as_slice(), plan.sources().count()) else {
        return Err(
            "a computed key is exported only in a join of two inputs beside one source collection"
                .to_string(),
        );
    };
    admit_collection(collection, sources)?;
    match computed.value() {
        ComputedJoinValue::Scalar(key) => admit_scalar(project, *key, anchor, sources),
        ComputedJoinValue::Sequence(sequence) => admit_sequence(project, sequence, anchor, sources),
    }
}

fn admit_scalar(
    project: &Project,
    key: NodeId,
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<ComputedKey, String> {
    let graph = &project.graph;
    let (value, guarded, guard) = peel_guard(graph, key);
    let mut walk = Walk {
        graph,
        anchor,
        sources,
        visited: 0,
        reads: Vec::new(),
    };
    let key_node = node(graph, value)?;
    if matches!(key_node, Node::SourceField { .. }) {
        return Err(
            "a key that is a bare source field would be imported as a singleton source".to_string(),
        );
    }
    walk.expression(value)?;
    if guarded != walk.reads {
        return Err(format!(
            "the key's null guard covers {} but the key reads {}; import guards exactly the fields a key reads",
            describe(&guarded),
            describe(&walk.reads)
        ));
    }
    ensure_guard_is_private(project, &guard, key)?;

    let (input_name, copy_all) = match key_node {
        Node::RuntimeParameter { name, .. } => (name.clone(), true),
        _ => ("value".to_string(), false),
    };
    Ok(ComputedKey {
        value,
        tokenize: None,
        guard,
        input_name,
        copy_all,
    })
}

/// Admits a sequence-valued key as import admits a `tokenize` feed: a
/// string input that a scalar key could read, guarded the same way, and a
/// non-empty string constant delimiter. Other sequences were not observed.
fn admit_sequence(
    project: &Project,
    sequence: &SequenceExpr,
    anchor: &[String],
    sources: &SourceExports<'_>,
) -> Result<ComputedKey, String> {
    let SequenceExpr::Tokenize {
        input,
        delimiter,
        item,
    } = sequence
    else {
        return Err(format!(
            "a sequence-valued key is exported only as tokenize, not `{}`",
            sequence.kind_name()
        ));
    };
    let graph = &project.graph;
    match node(graph, *delimiter)? {
        Node::Const {
            value: Value::String(text),
        } if !text.is_empty() => {}
        _ => {
            return Err(
                "the key's tokenize delimiter must be a non-empty string constant".to_string(),
            );
        }
    }
    let (value, guarded, guard) = peel_guard(graph, *input);
    let mut walk = Walk {
        graph,
        anchor,
        sources,
        visited: 0,
        reads: Vec::new(),
    };
    let ty = walk.expression(value)?;
    if ty != ScalarType::String {
        return Err("the key's tokenize input is not a string".to_string());
    }
    if guarded != walk.reads {
        return Err(format!(
            "the key's null guard covers {} but the key reads {}; import guards exactly the fields a key reads",
            describe(&guarded),
            describe(&walk.reads)
        ));
    }
    ensure_guard_is_private(project, &guard, *input)?;
    Ok(ComputedKey {
        value: *item,
        tokenize: Some(TokenizeKey {
            input: value,
            delimiter: *delimiter,
        }),
        guard,
        input_name: "value".to_string(),
        copy_all: false,
    })
}

/// The other input must be a repeating collection of the primary source:
/// import admits no other partner, and named collections beside a computed
/// key were not observed natively.
fn admit_collection(
    collection: &CollectionJoinSource,
    sources: &SourceExports<'_>,
) -> Result<(), String> {
    if collection.cardinality() != JoinSourceCardinality::Repeating {
        return Err(
            "the input beside a computed key must be a repeating source collection".to_string(),
        );
    }
    if sources.is_named_extra_path(collection.collection()) {
        return Err(format!(
            "the collection `{}` beside a computed key belongs to a named source; only primary-source collections were observed",
            collection.collection().join("/")
        ));
    }
    Ok(())
}

fn node(graph: &Graph, id: NodeId) -> Result<&Node, String> {
    graph
        .nodes
        .get(&id)
        .ok_or_else(|| format!("the key references missing node {id}"))
}

fn describe(reads: &[Vec<String>]) -> String {
    if reads.is_empty() {
        return "no field".to_string();
    }
    reads
        .iter()
        .map(|path| format!("`{}`", path.join("/")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_null(graph: &Graph, id: NodeId) -> bool {
    matches!(
        graph.nodes.get(&id),
        Some(Node::Const { value: Value::Null })
    )
}

/// The unframed source field `call` tests with `function`, if it is one.
fn guard_test<'g>(
    graph: &'g Graph,
    call: NodeId,
    function: &str,
) -> Option<(NodeId, &'g [String])> {
    let Some(Node::Call {
        function: name,
        args,
    }) = graph.nodes.get(&call)
    else {
        return None;
    };
    let [field] = args.as_slice() else {
        return None;
    };
    match graph.nodes.get(field) {
        Some(Node::SourceField { path, frame: None }) if name == function => {
            Some((*field, path.as_slice()))
        }
        _ => None,
    }
}

/// Strips the guards import wraps around a key, outermost first:
/// `if(exists(f), if(is_xml_nil(f), null, key), null)` per field `f`.
/// Returns the key below them, the guarded field paths, and the guard's own
/// nodes.
fn peel_guard(graph: &Graph, mut value: NodeId) -> (NodeId, Vec<Vec<String>>, BTreeSet<NodeId>) {
    let mut fields = Vec::new();
    let mut nodes = BTreeSet::new();
    while let Some(Node::If {
        condition,
        then,
        else_,
    }) = graph.nodes.get(&value)
    {
        let Some((field, path)) = guard_test(graph, *condition, "exists") else {
            break;
        };
        let Some(Node::If {
            condition: nil,
            then: nil_value,
            else_: inner,
        }) = graph.nodes.get(then)
        else {
            break;
        };
        if !is_null(graph, *else_)
            || !is_null(graph, *nil_value)
            || guard_test(graph, *nil, "is_xml_nil").map(|(nil_field, _)| nil_field) != Some(field)
        {
            break;
        }
        fields.push(path.to_vec());
        nodes.extend([value, *condition, *then, *else_, *nil, *nil_value]);
        value = *inner;
    }
    (value, fields, nodes)
}

/// Refuses a guard whose nodes other expressions also read: they would lose
/// their producers when the guard is left out.
fn ensure_guard_is_private(
    project: &Project,
    guard: &BTreeSet<NodeId>,
    key: NodeId,
) -> Result<(), String> {
    if guard.is_empty() {
        return Ok(());
    }
    let shared_by_node = project
        .graph
        .nodes
        .iter()
        .filter(|(id, _)| !guard.contains(id))
        .any(|(_, node)| {
            node.evaluated_inputs()
                .iter()
                .any(|input| guard.contains(input))
        });
    let mut shared_by_root = false;
    let mut visit = |root: ScopeRoot| {
        let key_root = root.slot == ScopeRootSlot::ComputedJoinKey && root.node == key;
        shared_by_root |= !key_root && guard.contains(&root.node);
    };
    project.root.visit_tree_roots(&mut visit);
    for target in &project.extra_targets {
        target.root.visit_tree_roots(&mut visit);
    }
    for rule in &project.failure_rules {
        rule.visit_roots(&mut visit);
    }
    if shared_by_node || shared_by_root {
        return Err("the key's null guard is shared with other expressions".to_string());
    }
    Ok(())
}

struct Walk<'a, 's> {
    graph: &'a Graph,
    anchor: &'a [String],
    sources: &'a SourceExports<'s>,
    visited: usize,
    /// Field paths in the order import first lowers them.
    reads: Vec<Vec<String>>,
}

impl Walk<'_, '_> {
    /// Checks one key node as import's admission would check its component,
    /// and gives its scalar type.
    fn expression(&mut self, id: NodeId) -> Result<ScalarType, String> {
        self.visited += 1;
        if self.visited > MAX_KEY_COMPONENTS {
            return Err(format!(
                "the key computation exceeds {MAX_KEY_COMPONENTS} components"
            ));
        }
        match node(self.graph, id)? {
            Node::Const { value } => match value {
                Value::String(_) => Ok(ScalarType::String),
                Value::Int(_) => Ok(ScalarType::Int),
                Value::Float(value) if value.is_finite() => Ok(ScalarType::Float),
                Value::Bool(_) => Ok(ScalarType::Bool),
                _ => Err(format!(
                    "the key constant at node {id} is not a string, integer, finite decimal or boolean"
                )),
            },
            Node::RuntimeParameter { ty, .. } => Ok(*ty),
            Node::SourceField { path, frame } => self.field(id, path, frame.as_deref()),
            Node::Call { function, args } => {
                let types = args
                    .iter()
                    .map(|arg| self.expression(*arg))
                    .collect::<Result<Vec<_>, _>>()?;
                call_type(function, &types)
                    .map_err(|reason| format!("the key function `{function}` {reason}"))
            }
            other => Err(format!(
                "the key node {id} is {}; a key is a constant, a required runtime parameter, or uppercase, lowercase, concat and add over those and source fields",
                kind_name(other)
            )),
        }
    }

    fn field(
        &mut self,
        id: NodeId,
        path: &[String],
        frame: Option<&[String]>,
    ) -> Result<ScalarType, String> {
        let display = path.join("/");
        if frame.is_some() || path.is_empty() {
            return Err(format!(
                "the key field `{display}` at node {id} is framed; import reads key fields unframed"
            ));
        }
        let (absolute, _) = self.sources.resolve_scope_path(self.anchor, path);
        if self.sources.is_named_extra_path(&absolute) {
            return Err(format!(
                "the key reads named source field `{display}`; computed keys over named or dynamic sources are not supported"
            ));
        }
        // Import reads a key field relative to the receiving context it
        // strips from the port's absolute path.
        let relative = absolute.strip_prefix(self.anchor).unwrap_or(&absolute);
        if relative != path {
            return Err(format!(
                "the key field `{display}` would be read as `{}` on re-import",
                relative.join("/")
            ));
        }
        let ty = match self
            .sources
            .schema_node_at(&absolute)
            .map(|node| (node.repeating, &node.kind))
        {
            Some((false, SchemaKind::Scalar { ty })) => *ty,
            _ => {
                return Err(format!(
                    "the key reads `{}`, which is not a non-repeating typed scalar",
                    absolute.join("/")
                ));
            }
        };
        let base = &absolute[..absolute.len() - relative.len()];
        let mut prefix = base.to_vec();
        for segment in relative {
            prefix.push(segment.clone());
            if self
                .sources
                .schema_node_at(&prefix)
                .is_none_or(|node| node.repeating)
            {
                return Err(format!(
                    "the key reads `{}` across a repeating level outside its iteration; the native key is a sequence",
                    absolute.join("/")
                ));
            }
        }
        // The exported field node takes the port its unframed path matches,
        // which must be the port import reads back.
        let port = self.sources.key_for_abs(&absolute);
        match self.sources.match_field(path, false) {
            PortMatch::Unique(matched) if Some(matched) == port => {}
            _ => {
                return Err(format!(
                    "the key field `{display}` has no unique source port"
                ));
            }
        }
        if !self.reads.iter().any(|read| read == path) {
            self.reads.push(path.to_vec());
        }
        Ok(ty)
    }
}

fn kind_name(node: &Node) -> &'static str {
    match node {
        Node::If { .. } => "a conditional",
        Node::ValueMap { .. } => "a value map",
        Node::RuntimeParameterDefault { .. } => "an optional runtime parameter",
        Node::RuntimeValue { .. } => "a runtime value",
        Node::Aggregate { .. } | Node::JoinAggregate { .. } | Node::SequenceAggregate { .. } => {
            "an aggregate"
        }
        Node::UserFunctionCall { .. } => "a user-defined function call",
        Node::JoinField { .. } | Node::JoinPosition { .. } => "a joined tuple read",
        _ => "not an admitted key node",
    }
}

/// The result type of an admitted call, or why import refuses it: each
/// admitted call is total over the types it accepts, so a key never raises.
/// This mirrors import's admission rule.
fn call_type(function: &str, args: &[ScalarType]) -> Result<ScalarType, String> {
    let name = |ty: &ScalarType| match ty {
        ScalarType::String => "a string",
        ScalarType::Int => "an integer",
        ScalarType::Float => "a decimal",
        ScalarType::Bool => "a boolean",
    };
    match function {
        "upper" | "lower" => match args {
            [ScalarType::String] => Ok(ScalarType::String),
            [other] => Err(format!("over {} value can raise", name(other))),
            _ => Err(format!("takes one input, not {}", args.len())),
        },
        "concat" => {
            if args.len() < 2 {
                return Err(format!("takes at least two inputs, not {}", args.len()));
            }
            match args.iter().find(|ty| **ty == ScalarType::Float) {
                Some(ty) => Err(format!(
                    "over {} value has an unobserved text form",
                    name(ty)
                )),
                None => Ok(ScalarType::String),
            }
        }
        "add" => {
            if args.len() < 2 {
                return Err(format!("takes at least two inputs, not {}", args.len()));
            }
            if let Some(ty) = args
                .iter()
                .find(|ty| !matches!(ty, ScalarType::Int | ScalarType::Float))
            {
                return Err(format!("over {} value can raise", name(ty)));
            }
            if args.contains(&ScalarType::Float) {
                Ok(ScalarType::Float)
            } else {
                Err("over integers alone can overflow".to_string())
            }
        }
        _ => Err("is outside the admitted uppercase, lowercase, concat and add".to_string()),
    }
}
