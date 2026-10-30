//! Computed join keys execute as one scalar member per join execution,
//! evaluated in the join's receiving context. Hand-built projects reproduce
//! every admitted design of the frozen corpus under
//! `docs/design/fixtures/join-computed-keys`; MFD import lowers the designs
//! themselves separately.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use ir::SchemaNode;
use mapping::{
    Binding, ComputedJoinSourceId, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    ScopeIteration,
};

#[path = "computed_join/controls.rs"]
mod controls;
#[path = "computed_join/validation.rs"]
mod validation;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/join-computed-keys"
);
const EXPECTED: &str =
    include_str!("../../../../docs/design/fixtures/join-computed-keys/expected.json");
const INPUT: &str = include_str!("../../../../docs/design/fixtures/join-computed-keys/input.xml");
const NUMERIC_TEXT_INPUT: &str =
    include_str!("../../../../docs/design/fixtures/join-computed-keys/input-numeric-text.xml");

const JOIN: JoinId = JoinId::new(5);
const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);
/// Tuple reads and the order id every fixture project binds.
const NUMBER: NodeId = 1;
const NAME: NodeId = 2;
const ORDER_ID: NodeId = 3;

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn source_schema() -> SchemaNode {
    format_xml::xsd::import(&Path::new(FIXTURES).join("source.xsd"))
        .expect("the frozen source schema imports")
}

fn target_schema() -> SchemaNode {
    format_xml::xsd::import(&Path::new(FIXTURES).join("target.xsd"))
        .expect("the frozen target schema imports")
}

fn input(xml: &str) -> Instance {
    format_xml::from_str(xml, &source_schema()).expect("the frozen input parses")
}

/// The graph of one fixture project: the shared tuple reads plus the key's
/// own nodes.
struct Keys {
    nodes: BTreeMap<NodeId, Node>,
}

impl Keys {
    fn new() -> Self {
        let read = |field: &str| Node::JoinField {
            join: JOIN,
            collection: path(&["Customer"]),
            path: path(&[field]),
        };
        Self {
            nodes: BTreeMap::from([
                (NUMBER, read("Number")),
                (NAME, read("Name")),
                (
                    ORDER_ID,
                    Node::SourceField {
                        path: path(&["Id"]),
                        frame: None,
                    },
                ),
            ]),
        }
    }

    fn add(&mut self, node: Node) -> NodeId {
        let id = self
            .nodes
            .keys()
            .next_back()
            .map_or(10, |last| last + 1)
            .max(10);
        self.nodes.insert(id, node);
        id
    }

    fn field(&mut self, segments: &[&str]) -> NodeId {
        self.add(Node::SourceField {
            path: path(segments),
            frame: None,
        })
    }

    fn constant(&mut self, value: Value) -> NodeId {
        self.add(Node::Const { value })
    }

    fn call(&mut self, function: &str, args: Vec<NodeId>) -> NodeId {
        self.add(Node::Call {
            function: function.into(),
            args,
        })
    }

    /// `function` over `segments`, lowered as the design requires of import:
    /// `Null` when the field is absent or nil, so the join yields no tuples
    /// where the native key is empty.
    fn guarded(
        &mut self,
        segments: &[&str],
        expression: impl FnOnce(&mut Self, NodeId) -> NodeId,
    ) -> NodeId {
        let field = self.field(segments);
        let value = expression(self, field);
        let present = self.call("exists", vec![field]);
        let nil = self.call("is_xml_nil", vec![field]);
        let null = self.constant(Value::Null);
        let unless_nil = self.add(Node::If {
            condition: nil,
            then: null,
            else_: value,
        });
        self.add(Node::If {
            condition: present,
            then: unless_nil,
            else_: null,
        })
    }

    fn graph(self) -> Graph {
        Graph { nodes: self.nodes }
    }
}

/// The key of one fixture: a computed node or the existing singleton form.
#[derive(Clone)]
enum Key {
    Computed(NodeId),
    Singleton(Vec<String>),
}

fn plan(key: &Key, right_key: &str, key_first: bool) -> JoinPlan {
    let customers = JoinSource::new(path(&["Customer"]));
    let (source, left) = match key {
        Key::Computed(value) => (
            JoinSource::computed(KEY, *value),
            JoinKey::computed(KEY, path(&[right_key])),
        ),
        Key::Singleton(singleton) => (
            JoinSource::singleton(singleton.clone()),
            JoinKey::new(singleton.clone(), Vec::new(), path(&[right_key])),
        ),
    };
    if key_first {
        JoinPlan::new(source, customers, JoinConditions::new(left))
    } else {
        JoinPlan::new(
            customers,
            source,
            JoinConditions::new(JoinKey::new(
                path(&["Customer"]),
                path(&[right_key]),
                Vec::new(),
            )),
        )
    }
    .expect("a valid two-source plan")
}

fn tuple_scope(target: &str, plan: JoinPlan) -> Scope {
    Scope {
        target_field: target.into(),
        iteration: ScopeIteration::InnerJoin { id: JOIN, plan },
        bindings: vec![
            Binding {
                target_field: "Number".into(),
                node: NUMBER,
            },
            Binding {
                target_field: "Name".into(),
                node: NAME,
            },
        ],
        ..Scope::default()
    }
}

/// Where the join's tuples go: `Match` inside each target `Order`, or the
/// root `Hit` collection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    EachOrder,
    Root,
}

fn root_scope(context: Context, plan: JoinPlan) -> Scope {
    let child = match context {
        Context::EachOrder => Scope {
            target_field: "Order".into(),
            iteration: ScopeIteration::Source(path(&["Order"])),
            bindings: vec![Binding {
                target_field: "Id".into(),
                node: ORDER_ID,
            }],
            children: vec![tuple_scope("Match", plan)],
            ..Scope::default()
        },
        Context::Root => tuple_scope("Hit", plan),
    };
    Scope {
        children: vec![child],
        ..Scope::default()
    }
}

fn fixture_project(graph: Graph, root: Scope) -> Project {
    Project {
        source: source_schema(),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root,
    }
}

/// One admitted fixture design as a hand-built project.
struct Fixture {
    id: &'static str,
    input: &'static str,
    context: Context,
    right_key: &'static str,
    key: fn(&mut Keys) -> Key,
}

fn upper(keys: &mut Keys, field: NodeId) -> NodeId {
    keys.call("upper", vec![field])
}

const FIXTURE_DESIGNS: &[Fixture] = &[
    Fixture {
        id: "direct",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Fixture {
        id: "upper",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| Key::Computed(keys.guarded(&["CustomerNumber"], upper)),
    },
    Fixture {
        id: "lower",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| {
            Key::Computed(keys.guarded(&["CustomerNumber"], |keys, field| {
                keys.call("lower", vec![field])
            }))
        },
    },
    Fixture {
        id: "concat",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| {
            Key::Computed(keys.guarded(&["CustomerNumber"], |keys, field| {
                let empty = keys.constant(Value::String(String::new()));
                keys.call("concat", vec![field, empty])
            }))
        },
    },
    Fixture {
        id: "constant",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| Key::Computed(keys.constant(Value::String("C2".into()))),
    },
    Fixture {
        id: "parameter",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| {
            Key::Computed(keys.add(Node::RuntimeParameter {
                name: "Key".into(),
                ty: ScalarType::String,
                preview: Some("C2".into()),
            }))
        },
    },
    // Ferrule's numeric `add` takes a float constant; its result compares by
    // the same canonical text as the native decimal, so `7` matches "7" only.
    Fixture {
        id: "numeric",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |keys| {
            Key::Computed(keys.guarded(&["Qty"], |keys, field| {
                let zero = keys.constant(Value::Float(0.0));
                keys.call("add", vec![field, zero])
            }))
        },
    },
    Fixture {
        id: "integer-direct",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Number",
        key: |_| Key::Singleton(path(&["Qty"])),
    },
    Fixture {
        id: "rank",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Rank",
        key: |keys| Key::Computed(keys.guarded(&["CustomerNumber"], upper)),
    },
    Fixture {
        id: "rank-direct",
        input: INPUT,
        context: Context::EachOrder,
        right_key: "Rank",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Fixture {
        id: "rank-text",
        input: NUMERIC_TEXT_INPUT,
        context: Context::EachOrder,
        right_key: "Rank",
        key: |keys| Key::Computed(keys.guarded(&["CustomerNumber"], upper)),
    },
    Fixture {
        id: "rank-text-direct",
        input: NUMERIC_TEXT_INPUT,
        context: Context::EachOrder,
        right_key: "Rank",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Fixture {
        id: "root-constant",
        input: INPUT,
        context: Context::Root,
        right_key: "Number",
        key: |keys| Key::Computed(keys.constant(Value::String("C2".into()))),
    },
    Fixture {
        id: "root-header",
        input: INPUT,
        context: Context::Root,
        right_key: "Number",
        key: |keys| Key::Computed(keys.guarded(&["Header", "Code"], upper)),
    },
];

impl Fixture {
    fn project(&self, key_first: bool) -> Project {
        let mut keys = Keys::new();
        let key = (self.key)(&mut keys);
        let joined = plan(&key, self.right_key, key_first);
        fixture_project(keys.graph(), root_scope(self.context, joined))
    }
}

/// Matched `[Number, Name]` pairs per target order, or under `Hit` for the
/// root collection.
type Tuples = BTreeMap<String, Vec<(String, String)>>;

fn text(instance: &Instance) -> String {
    match instance.as_scalar() {
        Some(Value::String(text)) => text.clone(),
        other => panic!("expected text, found {other:?}"),
    }
}

fn pairs(rows: Option<&Instance>) -> Vec<(String, String)> {
    rows.and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .map(|row| {
            let field = |name| row.field(name).map(text).unwrap_or_default();
            (field("Number"), field("Name"))
        })
        .collect()
}

fn tuples(output: &Instance, context: Context) -> Tuples {
    match context {
        Context::EachOrder => {
            assert!(
                output
                    .field("Hit")
                    .is_none_or(|hit| pairs(Some(hit)).is_empty())
            );
            output
                .field("Order")
                .and_then(Instance::as_repeated)
                .expect("one target order per source order")
                .iter()
                .map(|order| {
                    (
                        order.field("Id").map(text).expect("order id"),
                        pairs(order.field("Match")),
                    )
                })
                .collect()
        }
        Context::Root => {
            assert!(
                output
                    .field("Order")
                    .is_none_or(|order| order.as_repeated().is_some_and(<[Instance]>::is_empty))
            );
            BTreeMap::from([("Hit".into(), pairs(output.field("Hit")))])
        }
    }
}

fn expected_case(id: &str) -> serde_json::Value {
    let expected: serde_json::Value = serde_json::from_str(EXPECTED).expect("expected.json parses");
    expected["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("no frozen case `{id}`"))
        .clone()
}

fn expected_tuples(id: &str) -> Tuples {
    let case = expected_case(id);
    case["expected"]
        .as_object()
        .expect("expected tuples")
        .iter()
        .map(|(owner, rows)| {
            let rows = rows
                .as_array()
                .expect("tuple rows")
                .iter()
                .map(|row| {
                    let field = |index: usize| row[index].as_str().expect("text").to_string();
                    (field(0), field(1))
                })
                .collect();
            (owner.clone(), rows)
        })
        .collect()
}

fn key_parameters() -> RuntimeParameters {
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("Key", Value::String("C2".into()))
        .expect("a valid parameter");
    parameters
}

pub(super) fn run_fixture(project: &Project, source: &Instance) -> Result<Instance, EngineError> {
    let parameters = key_parameters();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_parameters(&parameters);
    run_with_context(project, source, &execution)
}

#[test]
fn every_admitted_fixture_design_reproduces_its_frozen_tuples() {
    for fixture in FIXTURE_DESIGNS {
        let case = expected_case(fixture.id);
        assert_eq!(case["native"], "observed", "{}", fixture.id);
        let input_name = if fixture.input == INPUT {
            "input.xml"
        } else {
            "input-numeric-text.xml"
        };
        assert_eq!(case["input"], input_name, "{}", fixture.id);
        let source = input(fixture.input);
        // A single value contributes the same tuples from either side.
        for key_first in [true, false] {
            let project = fixture.project(key_first);
            assert_eq!(
                validate(&project),
                [],
                "{} key_first={key_first}",
                fixture.id
            );
            let output = run_fixture(&project, &source)
                .unwrap_or_else(|error| panic!("{}: {error}", fixture.id));
            assert_eq!(
                tuples(&output, fixture.context),
                expected_tuples(fixture.id),
                "{} key_first={key_first}",
                fixture.id
            );
        }
    }
}

/// The admitted designs are exactly the corpus rows that scopes 2 and 4
/// admit; the sequence-valued and refused rows have their own tests.
#[test]
fn the_fixture_list_covers_every_admitted_corpus_row() {
    let expected: serde_json::Value = serde_json::from_str(EXPECTED).expect("expected.json parses");
    let mut admitted = expected["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter_map(|case| case["id"].as_str())
        .filter(|id| {
            ![
                "tokens",
                "tokens-second",
                "root-repeating",
                "copy-all-computed",
                "key-read",
                "unlisted-function",
            ]
            .contains(id)
        })
        .collect::<Vec<_>>();
    admitted.sort_unstable();
    let mut covered = FIXTURE_DESIGNS
        .iter()
        .map(|fixture| fixture.id)
        .collect::<Vec<_>>();
    covered.sort_unstable();
    assert_eq!(covered, admitted);
}

/// For equal values a computed key yields exactly the singleton join's
/// tuples: an unguarded computed read of the same field matches the
/// existing form order for order, including `""`, nil and absence.
#[test]
fn a_computed_read_of_a_field_matches_the_singleton_form() {
    let source = input(INPUT);
    for right_key in ["Number", "Rank"] {
        let singleton = {
            let keys = Keys::new();
            let joined = plan(&Key::Singleton(path(&["CustomerNumber"])), right_key, true);
            fixture_project(keys.graph(), root_scope(Context::EachOrder, joined))
        };
        for key_first in [true, false] {
            let mut keys = Keys::new();
            let key = Key::Computed(keys.field(&["CustomerNumber"]));
            let joined = plan(&key, right_key, key_first);
            let computed = fixture_project(keys.graph(), root_scope(Context::EachOrder, joined));
            assert_eq!(validate(&computed), []);
            assert_eq!(
                tuples(&run(&computed, &source).unwrap(), Context::EachOrder),
                tuples(&run(&singleton, &source).unwrap(), Context::EachOrder),
                "{right_key} key_first={key_first}"
            );
        }
    }
}
