//! Owned capability admission reads a generated sequence level by level
//! through the shared level-indexed input visitor. These checks compare it
//! with the recursive admission it replaced, over every producer composed up
//! to twice, in every context.

use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    DynamicSourcePath, FilterMapCapture, FilterMapV1, FunctionId, Graph, NamedSource, Node, NodeId,
    OwnedRecursiveCollect, OwnedSourceField, OwnedSourceRef, Project, SequenceExpr, SourceOwner,
    UserFunction,
};

use super::super::{Admission, basic_item_at_producer};
use crate::OwnedCapabilityContext;

/// Sequence admission as it recursed through composed sources by hand.
fn legacy_sequence(
    admission: &mut Admission<'_>,
    sequence: &SequenceExpr,
    function: Option<FunctionId>,
    inherited: OwnedCapabilityContext,
    basic_consumer: bool,
) {
    let context = if basic_consumer
        && inherited == OwnedCapabilityContext::Basic
        && basic_item_at_producer(sequence)
    {
        OwnedCapabilityContext::Basic
    } else if matches!(sequence, SequenceExpr::RecursiveCollect { .. })
        && matches!(
            inherited,
            OwnedCapabilityContext::Basic | OwnedCapabilityContext::GeneratedItem
        )
    {
        OwnedCapabilityContext::TreeOperator
    } else if inherited == OwnedCapabilityContext::Basic {
        OwnedCapabilityContext::GeneratedItem
    } else {
        inherited
    };
    admission.push(function, context, false, sequence.inputs());
    // Producer arguments are parent scalars; the lexical declaration is
    // still an ordinary generated item, never an endpoint-bearing input.
    let item_context = if context == OwnedCapabilityContext::Basic {
        OwnedCapabilityContext::GeneratedItem
    } else {
        context
    };
    admission.push(function, item_context, false, sequence.owned_items());
    match sequence {
        SequenceExpr::RecursiveCollect { collection, .. } => admission.path(collection),
        SequenceExpr::FilterMapV1(composition) => {
            admission.function(composition.predicate);
            admission.function(composition.mapper);
            legacy_sequence(admission, &composition.source, function, context, false);
        }
        _ => {}
    }
}

const OWNED_READ: NodeId = 1;
const CONSTANT: NodeId = 2;

/// Two legacy dynamic sources, one driven by the other, a static named
/// source, an owned read every closed context refuses, and two user
/// functions whose bodies read it, so admission's verdict names whichever
/// context reads that node first.
fn project() -> Project {
    let scalar = |name: &str| SchemaNode::scalar(name, ScalarType::String);
    let rows = |name: &str| SchemaNode::group(name, vec![scalar("Value")]).repeating();
    let named = |name: &str, dynamic_path| NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: SchemaNode::group("Other", vec![rows("Tree")]),
        options: Default::default(),
        dynamic_path,
    };
    let owned_read = Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec!["Value".into()],
        },
        frame: None,
    });
    let function = |output| UserFunction {
        library: "Library".into(),
        name: format!("Function{output}"),
        description: None,
        parameters: Vec::new(),
        output_name: "Output".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: BTreeMap::from([
                (OWNED_READ, owned_read.clone()),
                (
                    CONSTANT,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
            ]),
        },
        output,
    };
    Project {
        source: SchemaNode::group("Primary", vec![scalar("Value"), rows("Tree")]),
        target: SchemaNode::group("Output", vec![scalar("Value")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named(
                "Dyn",
                Some(DynamicSourcePath::legacy(OWNED_READ, vec!["Driver".into()])),
            ),
            named(
                "Driver",
                Some(DynamicSourcePath::legacy(CONSTANT, Vec::new())),
            ),
            named("Static", None),
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([
            (FunctionId::new(1), function(OWNED_READ)),
            (FunctionId::new(2), function(CONSTANT)),
        ]),
        graph: Graph {
            nodes: BTreeMap::from([
                (OWNED_READ, owned_read),
                (
                    CONSTANT,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]),
        },
        root: mapping::Scope::default(),
    }
}

/// Producer inputs read the owned node or a constant by `read`.
fn producers(read: NodeId) -> Vec<SequenceExpr> {
    let tree = |collection: &[&str]| SequenceExpr::RecursiveCollect {
        collection: collection.iter().map(|part| (*part).into()).collect(),
        children: vec!["Children".into()],
        descent_value: vec!["Name".into()],
        values: vec!["Leaves".into()],
        value: vec!["Value".into()],
        prefix: read,
        separator: CONSTANT,
        item: 30,
    };
    vec![
        SequenceExpr::Tokenize {
            input: read,
            delimiter: CONSTANT,
            item: 30,
        },
        SequenceExpr::TokenizeByLength {
            input: CONSTANT,
            length: read,
            item: 30,
        },
        SequenceExpr::TokenizeRegex {
            input: read,
            pattern: CONSTANT,
            flags: Some(read),
            item: 30,
        },
        SequenceExpr::Generate {
            from: Some(read),
            to: CONSTANT,
            item: 30,
        },
        tree(&["Dyn", "Tree"]),
        tree(&["Static", "Tree"]),
        tree(&["Tree"]),
        SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
            collection: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Tree".into()],
            },
            children: vec!["Children".into()],
            descent_value: vec!["Name".into()],
            values: vec!["Leaves".into()],
            value: vec!["Value".into()],
            prefix: read,
            separator: CONSTANT,
            item: 30,
        }),
    ]
}

fn compose(
    source: SequenceExpr,
    item: NodeId,
    predicate: u64,
    captures: &[NodeId],
) -> SequenceExpr {
    SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(source),
        item,
        predicate: FunctionId::new(predicate),
        mapper: FunctionId::new(3 - predicate),
        output_type: ScalarType::String,
        captures: captures
            .iter()
            .map(|&node| FilterMapCapture {
                node,
                ty: ScalarType::Int,
            })
            .collect(),
    })
}

/// Every producer alone and composed once and twice, over owned and constant
/// reads; item identities reuse the read nodes too.
fn sequences() -> Vec<SequenceExpr> {
    let mut all = Vec::new();
    for read in [OWNED_READ, CONSTANT] {
        for producer in producers(read) {
            let once = compose(producer.clone(), 31, 1, &[read]);
            all.push(compose(once.clone(), OWNED_READ, 2, &[CONSTANT]));
            all.push(compose(producer.clone(), CONSTANT, 2, &[]));
            all.push(once);
            all.push(producer);
        }
    }
    all
}

#[test]
fn sequences_are_admitted_as_the_recursive_admission_did() {
    let project = project();
    let contexts = [
        OwnedCapabilityContext::Basic,
        OwnedCapabilityContext::GeneratedItem,
        OwnedCapabilityContext::TreeOperator,
        OwnedCapabilityContext::FailureRule,
        OwnedCapabilityContext::DynamicTarget,
        OwnedCapabilityContext::IsolatedFunction,
    ];
    let mut checked = 0;
    let mut refused = 0;
    for sequence in sequences() {
        for inherited in contexts {
            for basic_consumer in [false, true] {
                for function in [None, Some(FunctionId::new(1))] {
                    let mut admitted = Admission::new(&project);
                    let mut legacy = Admission::new(&project);
                    admitted.sequence(&sequence, function, inherited, basic_consumer);
                    legacy_sequence(&mut legacy, &sequence, function, inherited, basic_consumer);
                    let case =
                        format!("{sequence:?} in {inherited:?}, {basic_consumer}, {function:?}");
                    assert_eq!(admitted.pending, legacy.pending, "{case}");
                    assert_eq!(admitted.dynamic, legacy.dynamic, "{case}");
                    assert_eq!(admitted.functions, legacy.functions, "{case}");
                    let verdict = admitted.graph();
                    refused += usize::from(verdict.is_err());
                    assert_eq!(verdict, legacy.graph(), "{case}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000, "checked {checked} admissions");
    assert!(refused > 100, "refused {refused} admissions");
}
