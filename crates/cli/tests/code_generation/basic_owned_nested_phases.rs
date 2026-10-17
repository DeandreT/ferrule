//! Finite, self-authored Project qualification, not a general parity claim.
//! The full typed fixtures below are fixed independently of all three executions.
//! No existing conformance corpus or production source is used as an oracle.
use super::*;
use mapping::{
    IterationOutput, NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceWindow,
    SortFilterOrder, SortKey, SourceOwner,
};

#[path = "basic_owned_nested_phases/hosts.rs"]
mod hosts;

const FLAGS: [&str; 7] = [
    "ParentPredicateFail",
    "KeyFail",
    "PredicateFail",
    "BoundFail",
    "ParentBindingFail",
    "RowBindingFail",
    "ChildFail",
];
const ROW_BOUND: u32 = 908;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn group(fields: impl IntoIterator<Item = (&'static str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn integer(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
fn bindings(fields: &[(&str, u32)]) -> Vec<Binding> {
    fields
        .iter()
        .map(|(name, node)| Binding {
            target_field: (*name).into(),
            node: *node,
        })
        .collect()
}

#[derive(Default)]
struct Nodes {
    graph: Graph,
    next: u32,
}
impl Nodes {
    fn add(&mut self, node: Node) -> u32 {
        self.next += 1;
        assert!(self.next < 900, "fixture IDs and error IDs stay disjoint");
        self.graph.nodes.insert(self.next, node);
        self.next
    }
    fn constant(&mut self, value: Value) -> u32 {
        self.add(Node::Const { value })
    }
    fn call(&mut self, function: &str, args: Vec<u32>) -> u32 {
        self.add(Node::Call {
            function: function.into(),
            args,
        })
    }
    fn choose(&mut self, condition: u32, then: u32, else_: u32) -> u32 {
        self.add(Node::If {
            condition,
            then,
            else_,
        })
    }
    fn field(&mut self, owner: SourceOwner, parts: &[&str], pin: &[&str]) -> u32 {
        self.add(Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(parts),
            },
            frame: Some(path(pin)),
        }))
    }
    fn position(&mut self, owner: SourceOwner, parts: &[&str]) -> u32 {
        self.add(Node::OwnedPosition(OwnedPosition {
            owner,
            collection: Some(path(parts)),
        }))
    }
}

fn source_parent_schema() -> SchemaNode {
    SchemaNode::group(
        "Parents",
        vec![
            string("Label"),
            int("Score"),
            bool_("Keep"),
            string("Bound"),
            SchemaNode::group(
                "Rows",
                vec![string("Label"), int("Score"), string("Tie"), bool_("Keep")],
            )
            .repeating(),
        ],
    )
    .repeating()
}
fn legacy_schema() -> SchemaNode {
    SchemaNode::group("Legacy", vec![string("Label"), int("Position")])
}
fn target_row_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Label"),
            string("Parent"),
            int("ParentPosition"),
            int("Position"),
            string("Root"),
            string("Outer"),
            int("OuterPosition"),
            legacy_schema(),
        ],
    )
    .repeating()
}
fn owned_iteration(owner: SourceOwner, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        output_path: None,
    }
}
fn legacy_scope(label: u32, position: u32) -> Scope {
    Scope {
        target_field: "Legacy".into(),
        iteration: ScopeIteration::Source(vec![]),
        iteration_output: IterationOutput::First,
        bindings: bindings(&[("Label", label), ("Position", position)]),
        ..Default::default()
    }
}

fn project_model() -> Project {
    let primary = SourceOwner::Primary;
    let mut n = Nodes::default();
    let one = n.constant(Value::Int(1));
    let two = n.constant(Value::Int(2));
    let four = n.constant(Value::Int(4));
    let five = n.constant(Value::Int(5));
    let eight = n.constant(Value::Int(8));
    let flags = FLAGS.map(|flag| n.field(primary.clone(), &[flag], &[]));
    for (id, message) in [
        (901, "parent-predicate"),
        (902, "later-raw-key"),
        (903, "later-sorted-predicate"),
        (904, "parent-window-bound"),
        (905, "parent-binding"),
        (906, "row-binding"),
        (907, "legacy-grandchild"),
    ] {
        let message = n.constant(Value::String(message.into()));
        n.graph.nodes.insert(
            id,
            Node::Raise {
                message: Some(message),
            },
        );
    }
    let parent_label = n.field(primary.clone(), &["Parents", "Label"], &["Parents"]);
    let parent_position = n.position(primary.clone(), &["Parents"]);
    let parent_score = n.field(primary.clone(), &["Parents", "Score"], &["Parents"]);
    let parent_keep = n.field(primary.clone(), &["Parents", "Keep"], &["Parents"]);
    let parent_bound = n.field(primary.clone(), &["Parents", "Bound"], &["Parents"]);
    let at_second = n.call("equal", vec![parent_position, two]);
    let fail_parent_predicate = n.call("and", vec![flags[0], at_second]);
    let parent_predicate = n.choose(fail_parent_predicate, 901, parent_keep);
    let bound_parent_label = n.choose(flags[4], 905, parent_label);
    n.graph.nodes.insert(
        ROW_BOUND,
        Node::If {
            condition: flags[3],
            then: 904,
            else_: parent_bound,
        },
    );
    let legacy_label = n.add(Node::SourceField {
        path: path(&["Label"]),
        frame: None,
    });
    let legacy_position = n.add(Node::Position { collection: vec![] });
    // Even the failing grandchild has no owned expression: its root flag is a
    // legacy outward read. Owned ancestry must come from the iteration, not it.
    let legacy_child_flag = n.add(Node::SourceField {
        path: path(&["ChildFail"]),
        frame: None,
    });
    let legacy_child_label = n.choose(legacy_child_flag, 907, legacy_label);
    let contrast = |name: &str, iteration| Scope {
        target_field: name.into(),
        iteration,
        bindings: bindings(&[("Parent", parent_label), ("Position", parent_position)]),
        children: vec![legacy_scope(legacy_label, legacy_position)],
        ..Default::default()
    };
    let mut row_scopes = |owner: SourceOwner, include_raw: bool| {
        let label = n.field(
            owner.clone(),
            &["Parents", "Rows", "Label"],
            &["Parents", "Rows"],
        );
        let score = n.field(
            owner.clone(),
            &["Parents", "Rows", "Score"],
            &["Parents", "Rows"],
        );
        let tie = n.field(
            owner.clone(),
            &["Parents", "Rows", "Tie"],
            &["Parents", "Rows"],
        );
        let keep = n.field(
            owner.clone(),
            &["Parents", "Rows", "Keep"],
            &["Parents", "Rows"],
        );
        let position = n.position(owner.clone(), &["Parents", "Rows"]);
        let parent = n.field(owner.clone(), &["Parents", "Label"], &["Parents"]);
        let outer_position = n.position(owner.clone(), &["Parents"]);
        let root = n.field(owner.clone(), &["Label"], &[]);
        let at_eighth = n.call("equal", vec![position, eight]);
        let fail_key = n.call("and", vec![flags[1], at_eighth]);
        let key = n.choose(fail_key, 902, score);
        let in_first_four = n.call("less_or_equal", vec![position, four]);
        let keep = n.call("and", vec![keep, in_first_four]);
        let at_fifth = n.call("equal", vec![position, five]);
        let fail_predicate = n.call("and", vec![flags[2], at_fifth]);
        let predicate = n.choose(fail_predicate, 903, keep);
        let label = n.choose(flags[5], 906, label);
        let bound = if owner == primary {
            ROW_BOUND
        } else {
            let value = n.field(owner.clone(), &["Parents", "Bound"], &["Parents"]);
            n.choose(flags[3], 904, value)
        };
        let fields = bindings(&[
            ("Label", label),
            ("Parent", parent),
            ("ParentPosition", outer_position),
            ("Position", position),
            ("Root", root),
            ("Outer", parent_label),
            ("OuterPosition", parent_position),
        ]);
        let mut scopes = [
            ("SortFirst", SortFilterOrder::SortThenFilter),
            ("FilterFirst", SortFilterOrder::FilterThenSort),
        ]
        .map(|(name, order)| Scope {
            target_field: name.into(),
            iteration: owned_iteration(owner.clone(), &["Parents", "Rows"]),
            sort_by: Some(key),
            sort_descending: true,
            sort_then_by: vec![SortKey {
                node: tie,
                descending: false,
            }],
            sort_filter_order: order,
            filter: Some(predicate),
            windows: vec![
                SequenceWindow::SkipFirst { count: one },
                SequenceWindow::First { count: bound },
            ],
            bindings: fields.clone(),
            children: vec![legacy_scope(legacy_child_label, legacy_position)],
            ..Default::default()
        })
        .to_vec();
        if include_raw {
            scopes.push(Scope {
                target_field: "RawKeys".into(),
                iteration: owned_iteration(owner, &["Parents", "Rows"]),
                sort_by: Some(position),
                sort_descending: true,
                windows: vec![SequenceWindow::First { count: two }],
                bindings: fields,
                children: vec![legacy_scope(legacy_child_label, legacy_position)],
                ..Default::default()
            });
        }
        scopes
    };
    let mut children = row_scopes(primary.clone(), true);
    let named_children = row_scopes(named(), false);
    let named_parent_label = n.field(named(), &["Parents", "Label"], &["Parents"]);
    let named_parent_position = n.position(named(), &["Parents"]);
    let named_parent_score = n.field(named(), &["Parents", "Score"], &["Parents"]);
    children.push(Scope {
        target_field: "Reference".into(),
        iteration: owned_iteration(named(), &["Parents"]),
        sort_by: Some(named_parent_score),
        sort_descending: true,
        windows: vec![
            SequenceWindow::SkipFirst { count: one },
            SequenceWindow::First { count: one },
        ],
        bindings: bindings(&[
            ("Parent", named_parent_label),
            ("Position", named_parent_position),
            ("Outer", parent_label),
            ("OuterPosition", parent_position),
        ]),
        children: named_children,
        ..Default::default()
    });
    let primary_decoy = n.field(primary.clone(), &["Ref", "Label"], &[]);
    let named_root = n.field(named(), &["Label"], &[]);
    let contrast_schema = |name| {
        SchemaNode::group(
            name,
            vec![string("Parent"), int("Position"), legacy_schema()],
        )
        .repeating()
    };
    let reference = SchemaNode::group(
        "Reference",
        vec![
            string("Parent"),
            int("Position"),
            string("Outer"),
            int("OuterPosition"),
            target_row_schema("SortFirst"),
            target_row_schema("FilterFirst"),
        ],
    )
    .repeating();
    let mut source_children = vec![
        string("Label"),
        SchemaNode::group("Ref", vec![string("Label")]),
        source_parent_schema(),
    ];
    source_children.extend(FLAGS.map(bool_));
    Project {
        source: SchemaNode::group("Input", source_children),
        target: SchemaNode::group(
            "Output",
            vec![
                string("PrimaryDecoy"),
                string("NamedRoot"),
                contrast_schema("Plain"),
                contrast_schema("Owned"),
                SchemaNode::group(
                    "Nested",
                    vec![
                        string("Parent"),
                        int("Position"),
                        target_row_schema("SortFirst"),
                        target_row_schema("FilterFirst"),
                        target_row_schema("RawKeys"),
                        reference,
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Ref".into(),
            path: "unused-ref.json".into(),
            schema: SchemaNode::group("Input", vec![string("Label"), source_parent_schema()]),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: n.graph,
        root: Scope {
            bindings: bindings(&[("PrimaryDecoy", primary_decoy), ("NamedRoot", named_root)]),
            children: vec![
                contrast("Plain", ScopeIteration::Source(path(&["Parents"]))),
                contrast("Owned", owned_iteration(primary.clone(), &["Parents"])),
                Scope {
                    target_field: "Nested".into(),
                    iteration: owned_iteration(primary, &["Parents"]),
                    filter: Some(parent_predicate),
                    sort_by: Some(parent_score),
                    sort_descending: true,
                    windows: vec![
                        SequenceWindow::SkipFirst { count: one },
                        SequenceWindow::Last { count: two },
                    ],
                    bindings: bindings(&[
                        ("Parent", bound_parent_label),
                        ("Position", parent_position),
                    ]),
                    children,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    }
}

fn source_rows(parent: &str) -> Instance {
    // Complete raw order. C/D are equal on both keys; F is filtered, H is late
    // raw but first sorted. The <=4 predicate distinguishes the two orders.
    Instance::Repeated(
        [
            ("A", 1, "b", true),
            ("B", 3, "z", true),
            ("C", 3, "a", true),
            ("D", 3, "a", true),
            ("E", 2, "x", true),
            ("F", 4, "m", false),
            ("G", 0, "q", true),
            ("H", 5, "r", true),
        ]
        .into_iter()
        .map(|(label, score, tie, keep)| {
            group([
                ("Label", text(&format!("{parent}.{label}"))),
                ("Score", integer(score)),
                ("Tie", text(tie)),
                ("Keep", Instance::Scalar(Value::Bool(keep))),
            ])
        })
        .collect(),
    )
}
fn source_parent(label: &str, score: i64, keep: bool, rows: Instance, bound: Instance) -> Instance {
    group([
        ("Label", text(label)),
        ("Score", integer(score)),
        ("Keep", Instance::Scalar(Value::Bool(keep))),
        ("Bound", bound),
        ("Rows", rows),
    ])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Full,
    EmptyPrimary,
    EmptyChild,
    EmptyNamed,
    InvalidBound,
}
fn inputs(shape: Shape, enabled: &[usize]) -> (Instance, Instance) {
    let parents = if shape == Shape::EmptyPrimary {
        vec![]
    } else {
        [
            ("P.A", 3, true),
            ("P.B", 1, true),
            ("P.C", 3, true),
            ("P.D", 9, false),
        ]
        .into_iter()
        .map(|(label, score, keep)| {
            source_parent(
                label,
                score,
                keep,
                if shape == Shape::EmptyChild && label == "P.C" {
                    Instance::Repeated(vec![])
                } else {
                    source_rows(label)
                },
                if shape == Shape::InvalidBound && label == "P.C" {
                    Instance::Scalar(Value::Bool(true))
                } else {
                    text("2")
                },
            )
        })
        .collect()
    };
    let mut fields = vec![
        ("Label", text("P.root")),
        ("Ref", group([("Label", text("P.Ref.decoy"))])),
        ("Parents", Instance::Repeated(parents)),
    ];
    fields.extend(FLAGS.into_iter().enumerate().map(|(index, flag)| {
        (
            flag,
            Instance::Scalar(Value::Bool(enabled.contains(&index))),
        )
    }));
    let primary = group(fields);
    let named_parents = if shape == Shape::EmptyNamed {
        vec![]
    } else {
        [("N.A", 7), ("N.B", 7), ("N.C", 1)]
            .into_iter()
            .map(|(label, score)| source_parent(label, score, true, source_rows(label), text("2")))
            .collect()
    };
    (
        primary,
        group([
            ("Label", text("N.root")),
            ("Parents", Instance::Repeated(named_parents)),
        ]),
    )
}

fn expected_legacy(label: &str, position: i64) -> Instance {
    group([("Label", text(label)), ("Position", integer(position))])
}
fn expected_rows(
    parent: &str,
    parent_position: i64,
    root: &str,
    outer: &str,
    outer_position: i64,
    labels: &[&str],
) -> Instance {
    Instance::Repeated(
        labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let label = format!("{parent}.{label}");
                let position = index as i64 + 1;
                group([
                    ("Label", text(&label)),
                    ("Parent", text(parent)),
                    ("ParentPosition", integer(parent_position)),
                    ("Position", integer(position)),
                    ("Root", text(root)),
                    ("Outer", text(outer)),
                    ("OuterPosition", integer(outer_position)),
                    ("Legacy", expected_legacy(&label, position)),
                ])
            })
            .collect(),
    )
}
fn expected_tree(shape: Shape) -> Instance {
    // Derivation: parent sort D,A,C,B -> filter A,C,B -> skip A -> C,B.
    // Rows sort H,F,C,D,B,E,A,G -> filter H,C,D -> skip H -> C,D.
    // Raw filter A,B,C,D -> stable sort C,D,B,A -> skip C -> D,B.
    // A raw-position descending key -> H,G,F,E,D,C,B,A -> first H,G.
    // Named parent stable sort A,B,C -> skip A/first1 -> B. No primary
    // position may be overwritten by that named Parents cursor or its Rows.
    let empty_primary = shape == Shape::EmptyPrimary;
    let contrast = |owned: bool| {
        Instance::Repeated(if empty_primary {
            vec![]
        } else {
            [("P.A", 1), ("P.B", 2), ("P.C", 3), ("P.D", 4)]
                .into_iter()
                .map(|(label, position)| {
                    group([
                        ("Parent", text(label)),
                        ("Position", integer(position)),
                        (
                            "Legacy",
                            expected_legacy(label, if owned { position } else { 1 }),
                        ),
                    ])
                })
                .collect()
        })
    };
    let nested = if empty_primary {
        vec![]
    } else {
        [("P.C", 1), ("P.B", 2)]
            .into_iter()
            .map(|(parent, position)| {
                let labels = |full: &'static [&'static str]| {
                    if shape == Shape::EmptyChild && parent == "P.C" {
                        &[][..]
                    } else {
                        full
                    }
                };
                let reference = if shape == Shape::EmptyNamed {
                    vec![]
                } else {
                    vec![group([
                        ("Parent", text("N.B")),
                        ("Position", integer(1)),
                        ("Outer", text(parent)),
                        ("OuterPosition", integer(position)),
                        (
                            "SortFirst",
                            expected_rows("N.B", 1, "N.root", parent, position, &["C", "D"]),
                        ),
                        (
                            "FilterFirst",
                            expected_rows("N.B", 1, "N.root", parent, position, &["D", "B"]),
                        ),
                    ])]
                };
                group([
                    ("Parent", text(parent)),
                    ("Position", integer(position)),
                    (
                        "SortFirst",
                        expected_rows(
                            parent,
                            position,
                            "P.root",
                            parent,
                            position,
                            labels(&["C", "D"]),
                        ),
                    ),
                    (
                        "FilterFirst",
                        expected_rows(
                            parent,
                            position,
                            "P.root",
                            parent,
                            position,
                            labels(&["D", "B"]),
                        ),
                    ),
                    (
                        "RawKeys",
                        expected_rows(
                            parent,
                            position,
                            "P.root",
                            parent,
                            position,
                            labels(&["H", "G"]),
                        ),
                    ),
                    ("Reference", Instance::Repeated(reference)),
                ])
            })
            .collect()
    };
    group([
        ("PrimaryDecoy", text("P.Ref.decoy")),
        ("NamedRoot", text("N.root")),
        ("Plain", contrast(false)),
        ("Owned", contrast(true)),
        ("Nested", Instance::Repeated(nested)),
    ])
}

#[derive(Clone, Debug)]
enum Expected {
    Tree(Instance),
    Raise { node: u32, message: &'static str },
    ItemCount { node: u32 },
}
impl Expected {
    fn native(&self) -> Result<Instance, engine::EngineError> {
        match self {
            Self::Tree(tree) => Ok(tree.clone()),
            Self::Raise { node, message } => Err(engine::EngineError::MappingException {
                node: *node,
                message: Some((*message).into()),
            }),
            Self::ItemCount { node } => Err(engine::EngineError::NotAnItemCount {
                node: *node,
                found: "bool",
            }),
        }
    }
}
#[derive(Debug)]
struct Case {
    id: &'static str,
    primary: Instance,
    named: Instance,
    expected: Expected,
}
fn cases() -> Vec<Case> {
    let mut cases = [
        ("full", Shape::Full),
        ("empty-primary", Shape::EmptyPrimary),
        ("empty-child", Shape::EmptyChild),
        ("empty-named", Shape::EmptyNamed),
    ]
    .into_iter()
    .map(|(id, shape)| {
        let (primary, named) = inputs(shape, &[]);
        Case {
            id,
            primary,
            named,
            expected: Expected::Tree(expected_tree(shape)),
        }
    })
    .collect::<Vec<_>>();
    for (id, flags, node, message, shape) in [
        (
            "parent-predicate-before-parent-binding",
            &[0, 4][..],
            901,
            "parent-predicate",
            Shape::Full,
        ),
        (
            "parent-binding-before-child-key",
            &[4, 1][..],
            905,
            "parent-binding",
            Shape::Full,
        ),
        (
            "late-key-before-row-binding",
            &[1, 5][..],
            902,
            "later-raw-key",
            Shape::Full,
        ),
        (
            "late-predicate-before-row-binding",
            &[2, 5][..],
            903,
            "later-sorted-predicate",
            Shape::Full,
        ),
        (
            "bound-before-row-binding",
            &[3, 5][..],
            904,
            "parent-window-bound",
            Shape::Full,
        ),
        (
            "bound-before-post-sort-predicate",
            &[3, 2][..],
            904,
            "parent-window-bound",
            Shape::Full,
        ),
        (
            "row-binding-before-grandchild",
            &[5, 6][..],
            906,
            "row-binding",
            Shape::Full,
        ),
        (
            "legacy-grandchild",
            &[6][..],
            907,
            "legacy-grandchild",
            Shape::Full,
        ),
        (
            "empty-child-still-evaluates-bound",
            &[3][..],
            904,
            "parent-window-bound",
            Shape::EmptyChild,
        ),
    ] {
        let (primary, named) = inputs(shape, flags);
        cases.push(Case {
            id,
            primary,
            named,
            expected: Expected::Raise { node, message },
        });
    }
    let (primary, named) = inputs(Shape::InvalidBound, &[]);
    cases.push(Case {
        id: "typed-bool-invalid-parent-count",
        primary,
        named,
        expected: Expected::ItemCount { node: ROW_BOUND },
    });
    assert_eq!(cases.len(), 14);
    cases
}

struct Directory {
    path: PathBuf,
    complete: bool,
}
impl Directory {
    fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_basic_nested_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        host_policy::preflight_environment(&path)?;
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            complete: false,
        })
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if self.complete && !host_policy::keep_artifacts() {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "retained Basic nested-phase qualification: {}",
                self.path.display()
            );
        }
    }
}
fn freeze(directory: &Path, project: &Project, cases: &[Case]) -> TestResult<()> {
    std::fs::write(
        directory.join("project.json"),
        serde_json::to_vec_pretty(project)?,
    )?;
    std::fs::write(
        directory.join("frozen-typed-cases.txt"),
        format!("{cases:#?}\n"),
    )?;
    // Public native execution does not expose caller-selected limits. Its fixed
    // five-limit policy is recorded, not silently presented as configurable.
    std::fs::write(
        directory.join("policy.txt"),
        "native: public engine APIs, fixed internal limits (roots4096/depth256/path256/metadata16777216/walk1000000)\ngenerated: explicit caller limits (4096,256,256,16777216,1000000)\n14 cases, all-output and selected-primary calls; no boundary codecs, dynamic sources, joins, grouping, generated sequences, failure rules, or resource-limit qualification\n",
    )?;
    Ok(())
}

#[test]
fn native_basic_owned_nested_phases_match_frozen_typed_contract() -> TestResult<()> {
    let project = project_model();
    let cases = cases();
    let mut directory = Directory::new("native")?;
    freeze(&directory.path, &project, &cases)?;
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let context = engine::ExecutionContext::new(Path::new("self-authored/nested.json"));
    let mut failures = Vec::new();
    for case in &cases {
        let extras = || vec![("Ref".into(), case.named.clone())];
        let all = engine::run_outputs_with_sources_and_context(
            &project,
            &case.primary,
            extras(),
            &context,
        );
        std::fs::write(
            directory.path.join(format!("{}-all.txt", case.id)),
            format!("{all:#?}\n"),
        )?;
        let all = all.map(|output| {
            assert!(output.extras.is_empty());
            output.primary
        });
        let selected = engine::run_selected_target_with_sources_and_context(
            &project,
            &case.primary,
            extras(),
            &context,
            engine::TargetSelection::Primary,
        );
        std::fs::write(
            directory.path.join(format!("{}-selected.txt", case.id)),
            format!("{selected:#?}\n"),
        )?;
        let selected = selected.map(|output| match output {
            engine::SelectedTargetOutput::Primary(tree) => tree,
            _ => panic!("selected primary output kind"),
        });
        for (method, actual) in [("all", all), ("selected", selected)] {
            if actual != case.expected.native() {
                failures.push(format!("{} {method}: {actual:#?}", case.id));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "native mismatches (originals retained):\n{}",
        failures.join("\n")
    );
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_basic_owned_nested_phases_match_frozen_typed_contract() -> TestResult<()> {
    hosts::exercise("rust")
}
#[test]
fn generated_csharp_basic_owned_nested_phases_match_frozen_typed_contract() -> TestResult<()> {
    hosts::exercise("csharp")
}
