//! Genuine public Project definitions for owned sorting. Authored source
//! only: no mapping, key, predicate or position is evaluated here.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, ScopeConstruction,
    SortFilterOrder, SortKey, SourceOwner,
};

pub const NAMED: &str = "Ref";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 16] = [
    "ascending_scalar",
    "filter_then_sort",
    "sort_then_filter",
    "stable_total_order",
    "descending_secondary",
    "named_collision",
    "raw_position_keys",
    "nested_outer_phases",
    "fts_predicate_before_key",
    "fts_predicate_before_target",
    "stf_key_before_target",
    "stf_target_before_predicate",
    "candidate_major_secondary",
    "rejected_and_empty",
    "typed_owned_key",
    "plain_identity_positions",
];

const PRIMARY: SourceOwner = SourceOwner::Primary;
const ASCENDING: bool = false;
const DESCENDING: bool = true;

fn named() -> SourceOwner {
    SourceOwner::Named { name: NAMED.into() }
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn owned_field(owner: SourceOwner, field: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(field),
        },
        frame: frame.map(path),
    })
}

fn row_field(owner: SourceOwner, field: &str) -> Node {
    owned_field(owner, &["Rows", field], Some(&["Rows"]))
}

fn owned_position(owner: SourceOwner, collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(collection)),
    })
}

fn legacy_position(collection: &[&str]) -> Node {
    Node::Position {
        collection: path(collection),
    }
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn text(value: &str) -> Node {
    constant(Value::String(value.into()))
}

fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

fn raise(message: u32) -> Node {
    Node::Raise {
        message: Some(message),
    }
}

fn when(condition: u32, then: u32, else_: u32) -> Node {
    Node::If {
        condition,
        then,
        else_,
    }
}

/// Every model declares one source shape: a root `Value`, an embedded `Ref`
/// group colliding with the named owner, nested repeated rows, an empty
/// repetition and a nonrepeating `Details` group. `row_value` types `Rows.Value`.
fn input(row_value: SchemaNode) -> SchemaNode {
    let row = |value: SchemaNode| {
        SchemaNode::group(
            "Rows",
            vec![
                value,
                bool_("Flag"),
                string("ID"),
                int("Tag"),
                SchemaNode::group("Rows", vec![int("Value"), string("ID")]).repeating(),
            ],
        )
        .repeating()
    };
    SchemaNode::group(
        "Input",
        vec![
            string("Value"),
            SchemaNode::group(NAMED, vec![string("Value"), row(int("Value"))]),
            row(row_value),
            SchemaNode::group("EmptyRows", vec![int("Value")]).repeating(),
            SchemaNode::group("Details", vec![int("Value")]),
        ],
    )
}

/// Nodes 1–8 of every model, read from `owner`: row value, owned position,
/// the position > 1 predicate, root value, legacy position, row ID and Tag.
fn base(owner: SourceOwner) -> Vec<(u32, Node)> {
    vec![
        (1, row_field(owner.clone(), "Value")),
        (2, owned_position(owner.clone(), &["Rows"])),
        (3, constant(Value::Int(1))),
        (4, call("greater_than", &[2, 3])),
        (5, owned_field(owner.clone(), &["Value"], Some(&[]))),
        (6, legacy_position(&["Rows"])),
        (7, row_field(owner.clone(), "ID")),
        (8, row_field(owner, "Tag")),
    ]
}

/// Nodes 20–29: the second row raises `predicate_message` from node 25 (true
/// otherwise); node 26 raises "earlier target" and node 28 "earlier key".
fn priority(predicate_message: &str) -> Vec<(u32, Node)> {
    let mut nodes = base(PRIMARY);
    nodes.extend([
        (20, constant(Value::Int(2))),
        (21, call("equal", &[2, 20])),
        (22, raise(23)),
        (23, text(predicate_message)),
        (24, constant(Value::Bool(true))),
        (25, when(21, 22, 24)),
        (26, raise(27)),
        (27, text("earlier target")),
        (28, raise(29)),
        (29, text("earlier key")),
    ]);
    nodes
}

fn rows_target(name: &str, fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, fields).repeating()
}

/// `Value`, `OwnedPosition` and `LegacyPosition`, then `children`.
fn positions_target(children: Vec<SchemaNode>) -> SchemaNode {
    let mut fields = vec![int("Value"), int("OwnedPosition"), int("LegacyPosition")];
    fields.extend(children);
    rows_target("Output", fields)
}

const POSITIONS: [(&str, u32); 3] = [("Value", 1), ("OwnedPosition", 2), ("LegacyPosition", 6)];
/// `POSITIONS` with the raising node 26 as the value.
const RAISING_VALUE: [(&str, u32); 3] =
    [("Value", 26), ("OwnedPosition", 2), ("LegacyPosition", 6)];

/// One sorted owned scope: `keys` are the primary key then the secondary
/// keys, each `(node, descending)`.
struct Sorted<'a> {
    target_field: &'a str,
    owner: SourceOwner,
    collection: &'a [&'a str],
    filter: Option<u32>,
    keys: &'a [(u32, bool)],
    order: SortFilterOrder,
    bindings: &'a [(&'a str, u32)],
}

impl Sorted<'_> {
    fn scope(&self, children: Vec<Scope>) -> Scope {
        let ((primary, descending), secondary) =
            self.keys.split_first().expect("authored primary key");
        Scope {
            target_field: self.target_field.into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: self.owner.clone(),
                    path: path(self.collection),
                },
                output_path: None,
            },
            filter: self.filter,
            sort_by: Some(*primary),
            sort_descending: *descending,
            sort_then_by: secondary
                .iter()
                .map(|(node, descending)| SortKey {
                    node: *node,
                    descending: *descending,
                })
                .collect(),
            sort_filter_order: self.order,
            bindings: self
                .bindings
                .iter()
                .map(|(target_field, node)| Binding {
                    target_field: (*target_field).into(),
                    node: *node,
                })
                .collect(),
            children,
            ..Scope::default()
        }
    }
}

/// A primary `Rows` root scope.
fn rows<'a>(
    filter: Option<u32>,
    keys: &'a [(u32, bool)],
    order: SortFilterOrder,
    bindings: &'a [(&'a str, u32)],
) -> Sorted<'a> {
    Sorted {
        target_field: "",
        owner: PRIMARY,
        collection: &["Rows"],
        filter,
        keys,
        order,
        bindings,
    }
}

struct Model {
    source: SchemaNode,
    named: bool,
    target: SchemaNode,
    nodes: Vec<(u32, Node)>,
    root: Scope,
    extra_targets: Vec<NamedTarget>,
}

impl Model {
    fn new(target: SchemaNode, nodes: Vec<(u32, Node)>, root: Scope) -> Self {
        Self {
            source: input(int("Value")),
            named: false,
            target,
            nodes,
            root,
            extra_targets: vec![],
        }
    }

    fn project(self) -> Project {
        let extra_sources = if self.named {
            vec![NamedSource {
                name: NAMED.into(),
                path: "unused-filter-host-input.json".into(),
                schema: self.source.clone(),
                options: Default::default(),
                dynamic_path: None,
            }]
        } else {
            vec![]
        };
        Project {
            source: self.source,
            target: self.target,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources,
            extra_targets: self.extra_targets,
            failure_rules: vec![],
            user_functions: BTreeMap::new(),
            graph: Graph {
                nodes: self.nodes.into_iter().collect(),
            },
            root: self.root,
        }
    }
}

fn extra(name: &str, schema: SchemaNode, root: Scope) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: None,
        schema,
        options: Default::default(),
        root,
    }
}

pub fn project(model: &str) -> Project {
    use SortFilterOrder::{FilterThenSort as Fts, SortThenFilter as Stf};
    let model = match model {
        // A Scalar construction sorted by ascending row value.
        "ascending_scalar" => {
            let mut root = rows(None, &[(1, ASCENDING)], Fts, &[]).scope(vec![]);
            root.construction = ScopeConstruction::Scalar { value: 1 };
            Model::new(
                SchemaNode::scalar("Output", ScalarType::Int).repeating(),
                base(PRIMARY),
                root,
            )
        }
        "filter_then_sort" => Model::new(
            positions_target(vec![]),
            base(PRIMARY),
            rows(Some(4), &[(1, ASCENDING)], Fts, &POSITIONS).scope(vec![]),
        ),
        "sort_then_filter" => Model::new(
            positions_target(vec![]),
            base(PRIMARY),
            rows(Some(4), &[(1, ASCENDING)], Stf, &POSITIONS).scope(vec![]),
        ),
        // Row values may be any scalar type.
        "stable_total_order" => {
            let union = ir::ScalarTypeSet::new([
                ScalarType::String,
                ScalarType::Int,
                ScalarType::Float,
                ScalarType::Bool,
            ])
            .expect("authored union");
            let mut model = Model::new(
                rows_target("Output", vec![string("ID"), int("Position")]),
                base(PRIMARY),
                rows(None, &[(1, ASCENDING)], Fts, &[("ID", 7), ("Position", 2)]).scope(vec![]),
            );
            model.source = input(SchemaNode::scalar_union("Value", union));
            model
        }
        "descending_secondary" => Model::new(
            rows_target(
                "Output",
                vec![int("Value"), int("Tag"), string("ID"), int("Position")],
            ),
            base(PRIMARY),
            rows(
                None,
                &[(1, DESCENDING), (8, ASCENDING)],
                Fts,
                &[("Value", 1), ("Tag", 8), ("ID", 7), ("Position", 2)],
            )
            .scope(vec![]),
        ),
        // The named owner's rows, beside the colliding primary root value.
        "named_collision" => {
            let mut nodes = base(named());
            nodes.push((9, owned_field(PRIMARY, &["Value"], Some(&[]))));
            let mut model = Model::new(
                rows_target(
                    "Output",
                    vec![
                        int("Value"),
                        int("Position"),
                        string("NamedRoot"),
                        string("PrimaryRoot"),
                    ],
                ),
                nodes,
                Sorted {
                    owner: named(),
                    ..rows(
                        None,
                        &[(1, ASCENDING)],
                        Fts,
                        &[
                            ("Value", 1),
                            ("Position", 2),
                            ("NamedRoot", 5),
                            ("PrimaryRoot", 9),
                        ],
                    )
                }
                .scope(vec![]),
            );
            model.named = true;
            model
        }
        // Filter by owned position > 2, then sort by descending owned position.
        "raw_position_keys" => {
            let mut nodes = base(PRIMARY);
            nodes.extend([
                (9, constant(Value::Int(2))),
                (10, call("greater_than", &[2, 9])),
            ]);
            Model::new(
                positions_target(vec![]),
                nodes,
                rows(Some(10), &[(2, DESCENDING)], Fts, &POSITIONS).scope(vec![]),
            )
        }
        // Sorted, filtered parents with descending sorted child rows.
        "nested_outer_phases" => {
            let mut nodes = base(PRIMARY);
            nodes.extend([
                (
                    21,
                    owned_field(PRIMARY, &["Rows", "Rows", "Value"], Some(&["Rows", "Rows"])),
                ),
                (22, owned_position(PRIMARY, &["Rows", "Rows"])),
            ]);
            let kids = Sorted {
                target_field: "Kids",
                collection: &["Rows", "Rows"],
                ..rows(
                    None,
                    &[(21, DESCENDING)],
                    Fts,
                    &[("Value", 21), ("Position", 22), ("ParentPosition", 2)],
                )
            };
            Model::new(
                positions_target(vec![rows_target(
                    "Kids",
                    vec![int("Value"), int("Position"), int("ParentPosition")],
                )]),
                nodes,
                rows(Some(4), &[(1, ASCENDING)], Stf, &POSITIONS).scope(vec![kids.scope(vec![])]),
            )
        }
        "fts_predicate_before_key" => Model::new(
            positions_target(vec![]),
            priority("later predicate"),
            rows(Some(25), &[(28, ASCENDING)], Fts, &RAISING_VALUE).scope(vec![]),
        ),
        "fts_predicate_before_target" => Model::new(
            positions_target(vec![]),
            priority("later predicate"),
            rows(Some(25), &[(1, ASCENDING)], Fts, &RAISING_VALUE).scope(vec![]),
        ),
        // Node 30 raises "later key" from the second row.
        "stf_key_before_target" => {
            let mut nodes = priority("later key");
            nodes.push((30, when(21, 22, 1)));
            Model::new(
                positions_target(vec![]),
                nodes,
                rows(None, &[(30, ASCENDING)], Stf, &RAISING_VALUE).scope(vec![]),
            )
        }
        "stf_target_before_predicate" => Model::new(
            positions_target(vec![]),
            priority("later predicate"),
            rows(Some(25), &[(1, ASCENDING)], Stf, &RAISING_VALUE).scope(vec![]),
        ),
        // Node 30 raises "second primary" from the second row; the secondary
        // key at node zero raises "first secondary" from every row.
        "candidate_major_secondary" => {
            let mut nodes = priority("second primary");
            nodes.extend([
                (30, when(21, 22, 1)),
                (0, raise(31)),
                (31, text("first secondary")),
            ]);
            Model::new(
                positions_target(vec![]),
                nodes,
                rows(None, &[(30, ASCENDING), (0, ASCENDING)], Stf, &POSITIONS).scope(vec![]),
            )
        }
        // A false predicate rejects every row before the raising key; the
        // `Empty` target sorts an empty repetition by the same key.
        "rejected_and_empty" => {
            let mut nodes = base(PRIMARY);
            nodes.extend([
                (20, constant(Value::Bool(false))),
                (21, raise(22)),
                (22, text("unreached key")),
            ]);
            let mut model = Model::new(
                positions_target(vec![]),
                nodes,
                rows(Some(20), &[(21, ASCENDING)], Fts, &POSITIONS).scope(vec![]),
            );
            let empty = Sorted {
                collection: &["EmptyRows"],
                ..rows(None, &[(21, ASCENDING)], Fts, &[("Value", 1)])
            };
            model.extra_targets = vec![extra(
                "Empty",
                rows_target("Empty", vec![int("Value")]),
                empty.scope(vec![]),
            )];
            model
        }
        // The secondary key at node u32::MAX reads the declared named source.
        "typed_owned_key" => {
            let mut nodes = base(PRIMARY);
            nodes.push((u32::MAX, owned_field(named(), &["Value"], None)));
            let mut model = Model::new(
                positions_target(vec![]),
                nodes,
                rows(
                    None,
                    &[(1, ASCENDING), (u32::MAX, ASCENDING)],
                    Stf,
                    &POSITIONS,
                )
                .scope(vec![]),
            );
            model.named = true;
            model
        }
        // A same-owner `Identity` child re-iterates the current row; the
        // `Plain` target sorts the nonrepeating primary root by `Details.Value`.
        "plain_identity_positions" => {
            let mut nodes = base(PRIMARY);
            nodes.extend([
                (9, owned_field(PRIMARY, &["Details", "Value"], Some(&[]))),
                (10, legacy_position(&[])),
            ]);
            let identity = Sorted {
                target_field: "Identity",
                ..rows(None, &[(1, ASCENDING)], Fts, &POSITIONS)
            };
            let mut model = Model::new(
                positions_target(vec![rows_target(
                    "Identity",
                    vec![int("Value"), int("OwnedPosition"), int("LegacyPosition")],
                )]),
                nodes,
                rows(None, &[(1, ASCENDING)], Fts, &POSITIONS).scope(vec![identity.scope(vec![])]),
            );
            let plain = Sorted {
                collection: &[],
                ..rows(
                    None,
                    &[(9, ASCENDING)],
                    Fts,
                    &[("Value", 9), ("LegacyPosition", 10)],
                )
            };
            model.extra_targets = vec![extra(
                "Plain",
                rows_target("Plain", vec![int("Value"), int("LegacyPosition")]),
                plain.scope(vec![]),
            )];
            model
        }
        _ => panic!("authored model"),
    };
    model.project()
}
