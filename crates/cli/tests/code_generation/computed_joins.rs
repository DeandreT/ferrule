//! #1241 computed join keys: the engine's computed-key fixture projects,
//! generated as Rust and C#, match `engine::run` on the frozen inputs. The
//! successful variants share one project, each under its own target group
//! with its own join ids; each key failure is a project of its own, because
//! it fails the whole run.
use super::basic_owned_sequence_item_at::codec;
use super::*;
use mapping::{
    AggregateOp, ComputedJoinSourceId, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    NodeId, SequenceWindow,
};
use serde_json::{Value as Json, json};
use std::fs;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/join-computed-keys"
);
const EXPECTED: &str =
    include_str!("../../../../docs/design/fixtures/join-computed-keys/expected.json");
const INPUT: &str = include_str!("../../../../docs/design/fixtures/join-computed-keys/input.xml");
const NUMERIC_TEXT_INPUT: &str =
    include_str!("../../../../docs/design/fixtures/join-computed-keys/input-numeric-text.xml");
const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);
const TAG: &str = "computedjoin1241";

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn source_schema() -> TestResult<SchemaNode> {
    Ok(format_xml::xsd::import(
        &Path::new(FIXTURES).join("source.xsd"),
    )?)
}

/// The fixture `Report` fields, `Order` and `Hit`, that each variant repeats.
fn report_fields() -> TestResult<Vec<SchemaNode>> {
    let report = format_xml::xsd::import(&Path::new(FIXTURES).join("target.xsd"))?;
    match report.kind {
        ir::SchemaKind::Group { children, .. } => Ok(children),
        _ => Err("the fixture target is a group".into()),
    }
}

/// `instance` without XML type origins, which no fixture join reads.
fn plain(instance: Instance) -> Instance {
    match instance {
        Instance::Group(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), plain(value.clone())))
                .collect::<Vec<_>>()
                .into(),
        ),
        Instance::Repeated(items) => Instance::Repeated(items.into_iter().map(plain).collect()),
        other => other,
    }
}

/// The frozen inputs, plus the first without its customers. Each passes
/// through the host recipe codec unchanged.
fn inputs() -> TestResult<Vec<(&'static str, Instance)>> {
    let schema = source_schema()?;
    let without_customers = INPUT
        .lines()
        .filter(|line| !line.starts_with("<Customer>"))
        .collect::<Vec<_>>()
        .join("\n");
    let inputs = vec![
        ("input", plain(format_xml::from_str(INPUT, &schema)?)),
        (
            "numeric-text",
            plain(format_xml::from_str(NUMERIC_TEXT_INPUT, &schema)?),
        ),
        (
            "without-customers",
            plain(format_xml::from_str(&without_customers, &schema)?),
        ),
    ];
    for (name, input) in &inputs {
        if codec::decode(&codec::encode(input)) != *input {
            return Err(format!("{name} does not round-trip through the host codec").into());
        }
    }
    Ok(inputs)
}

/// The key of one variant: a computed node or the existing singleton form.
enum Key {
    Computed(NodeId),
    Singleton(Vec<String>),
}

fn plan(key: &Key, right_key: &str, key_first: bool) -> TestResult<JoinPlan> {
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
    Ok(if key_first {
        JoinPlan::new(source, customers, JoinConditions::new(left))?
    } else {
        JoinPlan::new(
            customers,
            source,
            JoinConditions::new(JoinKey::new(
                path(&["Customer"]),
                path(&[right_key]),
                Vec::new(),
            )),
        )?
    })
}

/// Where a variant's tuples go: `Match` inside each target `Order`, or the
/// variant's `Hit` collection.
#[derive(Clone, Copy, PartialEq)]
enum Context {
    EachOrder,
    Root,
}

/// One generated project under construction: a shared graph and one target
/// group per variant, `V1`, `V2`, ….
struct Cohort {
    nodes: BTreeMap<NodeId, Node>,
    groups: Vec<SchemaNode>,
    scopes: Vec<Scope>,
    joins: u64,
}

impl Cohort {
    fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            groups: Vec::new(),
            scopes: Vec::new(),
            joins: 0,
        }
    }

    fn add(&mut self, node: Node) -> NodeId {
        let id = self.nodes.len() as NodeId + 1;
        self.nodes.insert(id, node);
        id
    }

    fn join(&mut self) -> JoinId {
        self.joins += 1;
        JoinId::new(self.joins)
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

    fn read(&mut self, join: JoinId, collection: &str, field: &str) -> NodeId {
        self.add(Node::JoinField {
            join,
            collection: path(&[collection]),
            path: path(&[field]),
        })
    }

    /// `function` over `segments`, `Null` when the field is absent or nil, as
    /// the engine fixtures lower the native empty key.
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

    fn upper(&mut self, segments: &[&str]) -> NodeId {
        self.guarded(segments, |cohort, field| cohort.call("upper", vec![field]))
    }

    /// Adds one variant group holding `fields`, built by `scope`; returns its
    /// target field.
    fn variant(&mut self, fields: Vec<SchemaNode>, mut scope: Scope) -> String {
        let name = format!("V{}", self.groups.len() + 1);
        self.groups.push(SchemaNode::group(&name, fields));
        scope.target_field = name.clone();
        self.scopes.push(scope);
        name
    }

    /// A variant whose tuples drive `Match` in each `Order` or `Hit`; the
    /// tuple scope reads `number` and `name`, and `controls` may adjust it.
    fn tuples(
        &mut self,
        report: &[SchemaNode],
        context: Context,
        join: JoinId,
        plan: JoinPlan,
        (number, name): (NodeId, NodeId),
        controls: impl FnOnce(&mut Self, &mut Scope),
    ) -> String {
        let mut tuples = Scope {
            target_field: if context == Context::Root {
                "Hit"
            } else {
                "Match"
            }
            .into(),
            iteration: ScopeIteration::InnerJoin { id: join, plan },
            bindings: vec![
                Binding {
                    target_field: "Number".into(),
                    node: number,
                },
                Binding {
                    target_field: "Name".into(),
                    node: name,
                },
            ],
            ..Scope::default()
        };
        controls(self, &mut tuples);
        let child = match context {
            Context::Root => tuples,
            Context::EachOrder => {
                let id = self.field(&["Id"]);
                Scope {
                    target_field: "Order".into(),
                    iteration: ScopeIteration::Source(path(&["Order"])),
                    bindings: vec![Binding {
                        target_field: "Id".into(),
                        node: id,
                    }],
                    children: vec![tuples],
                    ..Scope::default()
                }
            }
        };
        self.variant(
            report.to_vec(),
            Scope {
                children: vec![child],
                ..Scope::default()
            },
        )
    }

    /// A variant whose `Names` and `Count` reduce the computed-key tuples per
    /// `Order` or once at the root.
    fn aggregates(&mut self, key: NodeId, key_first: bool, each_order: bool) -> TestResult<()> {
        let mut bindings = Vec::new();
        for function in [AggregateOp::Join, AggregateOp::Count] {
            let join = self.join();
            let (expression, arg, field) = if function == AggregateOp::Join {
                let name = self.read(join, "Customer", "Name");
                let separator = self.constant(Value::String(",".into()));
                (Some(name), Some(separator), "Names")
            } else {
                (None, None, "Count")
            };
            let node = self.add(Node::JoinAggregate {
                function,
                join,
                plan: plan(&Key::Computed(key), "Number", key_first)?,
                expression,
                arg,
            });
            bindings.push(Binding {
                target_field: field.into(),
                node,
            });
        }
        let summary = vec![
            SchemaNode::scalar("Names", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ];
        if each_order {
            let id = self.field(&["Id"]);
            bindings.insert(
                0,
                Binding {
                    target_field: "Id".into(),
                    node: id,
                },
            );
            let mut order = vec![SchemaNode::scalar("Id", ScalarType::String)];
            order.extend(summary);
            self.variant(
                vec![SchemaNode::group("Order", order).repeating()],
                Scope {
                    children: vec![Scope {
                        target_field: "Order".into(),
                        iteration: ScopeIteration::Source(path(&["Order"])),
                        bindings,
                        ..Scope::default()
                    }],
                    ..Scope::default()
                },
            );
        } else {
            self.variant(
                summary,
                Scope {
                    bindings,
                    ..Scope::default()
                },
            );
        }
        Ok(())
    }

    fn project(self) -> TestResult<Project> {
        Ok(Project {
            source: source_schema()?,
            target: SchemaNode::group("Cohort", self.groups),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph { nodes: self.nodes },
            root: Scope {
                children: self.scopes,
                ..Scope::default()
            },
        })
    }
}

/// One admitted fixture design, as the engine scope builds it.
struct Design {
    id: &'static str,
    input: &'static str,
    context: Context,
    right_key: &'static str,
    key: fn(&mut Cohort) -> Key,
}

const DESIGNS: &[Design] = &[
    Design {
        id: "direct",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Design {
        id: "upper",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| Key::Computed(cohort.upper(&["CustomerNumber"])),
    },
    Design {
        id: "lower",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| {
            Key::Computed(cohort.guarded(&["CustomerNumber"], |cohort, field| {
                cohort.call("lower", vec![field])
            }))
        },
    },
    Design {
        id: "concat",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| {
            Key::Computed(cohort.guarded(&["CustomerNumber"], |cohort, field| {
                let empty = cohort.constant(Value::String(String::new()));
                cohort.call("concat", vec![field, empty])
            }))
        },
    },
    Design {
        id: "constant",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| Key::Computed(cohort.constant(Value::String("C2".into()))),
    },
    Design {
        id: "parameter",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| {
            Key::Computed(cohort.add(Node::RuntimeParameter {
                name: "Key".into(),
                ty: ScalarType::String,
                preview: Some("C2".into()),
            }))
        },
    },
    Design {
        id: "numeric",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |cohort| {
            Key::Computed(cohort.guarded(&["Qty"], |cohort, field| {
                let zero = cohort.constant(Value::Float(0.0));
                cohort.call("add", vec![field, zero])
            }))
        },
    },
    Design {
        id: "integer-direct",
        input: "input",
        context: Context::EachOrder,
        right_key: "Number",
        key: |_| Key::Singleton(path(&["Qty"])),
    },
    Design {
        id: "rank",
        input: "input",
        context: Context::EachOrder,
        right_key: "Rank",
        key: |cohort| Key::Computed(cohort.upper(&["CustomerNumber"])),
    },
    Design {
        id: "rank-direct",
        input: "input",
        context: Context::EachOrder,
        right_key: "Rank",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Design {
        id: "rank-text",
        input: "numeric-text",
        context: Context::EachOrder,
        right_key: "Rank",
        key: |cohort| Key::Computed(cohort.upper(&["CustomerNumber"])),
    },
    Design {
        id: "rank-text-direct",
        input: "numeric-text",
        context: Context::EachOrder,
        right_key: "Rank",
        key: |_| Key::Singleton(path(&["CustomerNumber"])),
    },
    Design {
        id: "root-constant",
        input: "input",
        context: Context::Root,
        right_key: "Number",
        key: |cohort| Key::Computed(cohort.constant(Value::String("C2".into()))),
    },
    Design {
        id: "root-header",
        input: "input",
        context: Context::Root,
        right_key: "Number",
        key: |cohort| Key::Computed(cohort.upper(&["Header", "Code"])),
    },
];

/// Tuples of one variant join over `Customer`, read as `Number` and `Name`.
fn customer_tuples(cohort: &mut Cohort) -> (JoinId, (NodeId, NodeId)) {
    let join = cohort.join();
    let number = cohort.read(join, "Customer", "Number");
    let name = cohort.read(join, "Customer", "Name");
    (join, (number, name))
}

/// The successful variants: every admitted design from either side, scope
/// controls, longer plans, Null-like keys and joined aggregates. Also returns
/// each design variant's group, design and key side.
fn tuples_project() -> TestResult<(Project, Vec<(String, &'static Design)>)> {
    let report = report_fields()?;
    let mut cohort = Cohort::new();
    let mut designs = Vec::new();
    for design in DESIGNS {
        for key_first in [true, false] {
            let (join, reads) = customer_tuples(&mut cohort);
            let key = (design.key)(&mut cohort);
            let plan = plan(&key, design.right_key, key_first)?;
            let group = cohort.tuples(&report, design.context, join, plan, reads, |_, _| {});
            designs.push((group, design));
        }
    }

    type Controls = fn(&mut Cohort, &mut Scope, (NodeId, NodeId));
    let controls: [Controls; 6] = [
        |_, _, _| {},
        |_, scope, (_, name)| {
            scope.sort_by = Some(name);
            scope.sort_descending = true;
        },
        |cohort, scope, (_, name)| {
            let hopper = cohort.constant(Value::String("Hopper".into()));
            scope.filter = Some(cohort.call("equal", vec![name, hopper]));
        },
        |cohort, scope, _| {
            let (join, _) = scope.join().expect("control variants iterate a join");
            let position = cohort.add(Node::JoinPosition { join });
            let two = cohort.constant(Value::Int(2));
            scope.filter = Some(cohort.call("equal", vec![position, two]));
        },
        |cohort, scope, _| {
            let count = cohort.constant(Value::Int(1));
            scope.windows = vec![SequenceWindow::First { count }];
        },
        |cohort, scope, _| {
            let count = cohort.constant(Value::Int(1));
            scope.windows = vec![SequenceWindow::Last { count }];
        },
    ];
    for control in controls {
        for key_first in [true, false] {
            let (join, reads) = customer_tuples(&mut cohort);
            let key = cohort.constant(Value::String("C2".into()));
            let plan = plan(&Key::Computed(key), "Number", key_first)?;
            cohort.tuples(
                &report,
                Context::Root,
                join,
                plan,
                reads,
                |cohort, scope| control(cohort, scope, reads),
            );
        }
    }

    for key_text in ["C2", "C1"] {
        for key_middle in [true, false] {
            let join = cohort.join();
            let reads = (
                cohort.read(join, "Order", "Id"),
                cohort.read(join, "Customer", "Name"),
            );
            let key = cohort.constant(Value::String(key_text.into()));
            let customers = || JoinSource::new(path(&["Customer"]));
            let orders = || JoinSource::new(path(&["Order"]));
            let plan = if key_middle {
                JoinPlan::new(
                    customers(),
                    JoinSource::computed(KEY, key),
                    JoinConditions::new(JoinKey::new(
                        path(&["Customer"]),
                        path(&["Number"]),
                        Vec::new(),
                    )),
                )?
                .then(
                    orders(),
                    JoinConditions::new(JoinKey::new(
                        path(&["Customer"]),
                        path(&["Number"]),
                        path(&["CustomerNumber"]),
                    )),
                )?
            } else {
                JoinPlan::new(
                    JoinSource::computed(KEY, key),
                    customers(),
                    JoinConditions::new(JoinKey::computed(KEY, path(&["Number"]))),
                )?
                .then(
                    orders(),
                    JoinConditions::new(JoinKey::computed(KEY, path(&["CustomerNumber"]))),
                )?
            };
            cohort.tuples(&report, Context::Root, join, plan, reads, |_, _| {});
        }
    }

    for value in [Value::Null, Value::json_null(), Value::xml_nil()] {
        for key_first in [true, false] {
            let (join, reads) = customer_tuples(&mut cohort);
            let key = cohort.constant(value.clone());
            let plan = plan(&Key::Computed(key), "Number", key_first)?;
            cohort.tuples(&report, Context::Root, join, plan, reads, |_, _| {});
        }
    }

    for key_first in [true, false] {
        let key = cohort.upper(&["CustomerNumber"]);
        cohort.aggregates(key, key_first, true)?;
        let key = cohort.upper(&["Header", "Code"]);
        cohort.aggregates(key, key_first, false)?;
    }
    Ok((cohort.project()?, designs))
}

/// A key node failure fails the scope or aggregate that owns the key, even
/// when the other source is empty: an unguarded `upper` of the absent
/// `CustomerNumber` of order O3.
fn key_error_project(aggregate: bool) -> TestResult<Project> {
    let mut cohort = Cohort::new();
    let field = cohort.field(&["CustomerNumber"]);
    let key = cohort.call("upper", vec![field]);
    if aggregate {
        cohort.aggregates(key, false, true)?;
    } else {
        let (join, reads) = customer_tuples(&mut cohort);
        let plan = plan(&Key::Computed(key), "Number", true)?;
        cohort.tuples(
            &report_fields()?,
            Context::EachOrder,
            join,
            plan,
            reads,
            |_, _| {},
        );
    }
    cohort.project()
}

/// A decimal key is evaluated, then refused by `equal` at its first pairing.
fn decimal_key_project() -> TestResult<Project> {
    let report = report_fields()?;
    let mut cohort = Cohort::new();
    let decimal = Value::decimal(ir::Decimal96::parse_exact("7")?);
    for key_first in [true, false] {
        let (join, reads) = customer_tuples(&mut cohort);
        let key = cohort.constant(decimal.clone());
        let plan = plan(&Key::Computed(key), "Number", key_first)?;
        cohort.tuples(&report, Context::Root, join, plan, reads, |_, _| {});
    }
    cohort.project()
}

fn models() -> TestResult<Vec<(&'static str, Project)>> {
    Ok(vec![
        ("tuples", tuples_project()?.0),
        ("scope-key-error", key_error_project(false)?),
        ("aggregate-key-error", key_error_project(true)?),
        ("decimal-key", decimal_key_project()?),
    ])
}

/// The engine run with the `parameter` design's host value, `Key` = `C2`.
fn engine_run(project: &Project, source: &Instance) -> Result<Instance, engine::EngineError> {
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert("Key", Value::String("C2".into()))
        .expect("the fixture parameter is valid");
    let execution =
        engine::ExecutionContext::new(Path::new("mapping.json")).with_parameters(&parameters);
    engine::run_with_context(project, source, &execution)
}

/// Every model on every input, with the engine's outcome as its expectation.
/// A failure must be a function error, which both hosts compare by kind and
/// function, and the Rust host also by its text.
fn cases(models: &[(&str, Project)]) -> TestResult<Json> {
    let inputs = inputs()?;
    let mut cases = Vec::new();
    for (index, (name, project)) in models.iter().enumerate() {
        for (input, source) in &inputs {
            let expected = match engine_run(project, source) {
                Ok(output) => json!({"Ok": codec::encode(&output)}),
                Err(engine::EngineError::Function(error)) => {
                    let (kind, function) = match &error {
                        codegen_runtime::FunctionError::TypeMismatch { function, .. } => {
                            ("FunctionType", *function)
                        }
                        codegen_runtime::FunctionError::InvalidArgument { function, .. } => {
                            ("FunctionInvalidArgument", *function)
                        }
                        other => return Err(format!("{name}/{input}: {other}").into()),
                    };
                    json!({"Err": {"kind": kind, "function": function, "display": error.to_string()}})
                }
                Err(error) => return Err(format!("{name}/{input}: {error}").into()),
            };
            cases.push(json!({
                "label": format!("{name}/{input}"),
                "model": index,
                "source": codec::encode(source),
                "expected": expected,
            }));
        }
    }
    Ok(Json::Array(cases))
}

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

/// Matched pairs per target order, or under `Hit` at the root.
fn tuples(variant: &Instance, context: Context) -> BTreeMap<String, Vec<(String, String)>> {
    match context {
        Context::EachOrder => variant
            .field("Order")
            .and_then(Instance::as_repeated)
            .unwrap_or_default()
            .iter()
            .map(|order| {
                (
                    order.field("Id").map(text).unwrap_or_default(),
                    pairs(order.field("Match")),
                )
            })
            .collect(),
        Context::Root => BTreeMap::from([("Hit".into(), pairs(variant.field("Hit")))]),
    }
}

fn expected_tuples(id: &str) -> TestResult<BTreeMap<String, Vec<(String, String)>>> {
    let expected: Json = serde_json::from_str(EXPECTED)?;
    let case = expected["cases"]
        .as_array()
        .ok_or("cases")?
        .iter()
        .find(|case| case["id"] == id)
        .ok_or_else(|| format!("no frozen case `{id}`"))?;
    let mut tuples = BTreeMap::new();
    for (owner, rows) in case["expected"].as_object().ok_or("expected tuples")? {
        let rows = rows
            .as_array()
            .ok_or("tuple rows")?
            .iter()
            .map(|row| {
                let field = |index: usize| row[index].as_str().unwrap_or_default().to_string();
                (field(0), field(1))
            })
            .collect();
        tuples.insert(owner.clone(), rows);
    }
    Ok(tuples)
}

/// The cohort's own projects reproduce the frozen tuples of every admitted
/// design, so the generated hosts are compared on the fixture semantics.
#[test]
fn computed_join_cohort_reproduces_the_frozen_fixture_tuples() -> TestResult<()> {
    let (project, designs) = tuples_project()?;
    assert_eq!(engine::validate(&project), []);
    let inputs = inputs()?;
    for (input_name, source) in &inputs {
        let output = engine_run(&project, source)?;
        for (group, design) in &designs {
            if design.input != *input_name {
                continue;
            }
            let variant = output.field(group).ok_or("variant output")?;
            assert_eq!(
                tuples(variant, design.context),
                expected_tuples(design.id)?,
                "{} ({group})",
                design.id
            );
        }
    }
    for (name, project) in models()? {
        assert_eq!(engine::validate(&project), [], "{name}");
    }
    Ok(())
}

#[test]
fn computed_join_cohort_lowers_and_emits_in_both_languages() -> TestResult<()> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    for (name, project) in models()? {
        let program = codegen::lower(&project).map_err(|error| format!("{name}: {error}"))?;
        let options = codegen_rust::Options {
            package_name: format!("{TAG}-{name}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        codegen_rust::emit(&program, &options).map_err(|error| format!("{name}: {error}"))?;
        codegen_csharp::emit(&program).map_err(|error| format!("{name}: {error}"))?;
    }
    Ok(())
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        let destination = directory.join(file.path.as_str());
        fs::create_dir_all(destination.parent().ok_or("artifact parent")?)?;
        fs::write(destination, &file.contents)?;
    }
    Ok(())
}

fn require_success(stage: &str, output: &Output) -> TestResult<()> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{stage}: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .into())
}

/// Builds one generated Rust host over every model and one C# host per
/// model; requires cargo and the .NET 10 SDK.
#[test]
fn computed_join_cohort_generated_rust_and_csharp_match_the_engine() -> TestResult<()> {
    let models = models()?;
    let cases = cases(&models)?;
    let directory = TempDir::new(TAG)?;
    let root = directory.0.canonicalize()?;
    let cases_path = root.join("cases.json");
    fs::write(&cases_path, serde_json::to_vec_pretty(&cases)?)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;

    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, (name, project)) in models.iter().enumerate() {
        let program = codegen::lower(project).map_err(|error| format!("{name}: {error}"))?;
        let options = codegen_rust::Options {
            package_name: format!("{TAG}-model{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit(&program)?,
        )?;
        dependencies.push_str(&format!(
            "model{index} = {{ package = \"{TAG}-model{index}\", path = \"../rust-model{index}\" }}\n"
        ));
        dispatch.push_str(&format!(
            "        {index} => model{index}::execute_with_context(source, context),\n"
        ));
    }

    let host = root.join("rust-host");
    fs::create_dir_all(host.join("src"))?;
    fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{TAG}-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\ncodegen-runtime = {{ path = {} }}\nserde_json = \"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        ),
    )?;
    fs::write(
        host.join("src/main.rs"),
        include_str!("fixtures/computed_joins_rust.rs.txt").replace("@DISPATCH@", &dispatch),
    )?;
    fs::write(
        host.join("src/codec.rs"),
        include_str!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(host.join("Cargo.toml"))
        .arg("--")
        .arg(&cases_path)
        .env("CARGO_BUILD_BUILD_DIR", root.join("cargo-target"))
        .env("CARGO_TARGET_DIR", root.join("cargo-target"))
        .isolated_output()?;
    require_success("generated Rust hosts", &rust)?;

    for (index, (name, _)) in models.iter().enumerate() {
        let host = root.join(format!("csharp-host{index}"));
        fs::create_dir_all(&host)?;
        fs::write(
            host.join("Host.csproj"),
            format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
            ),
        )?;
        fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/computed_joins_csharp.cs.txt"),
        )?;
        let csharp = dotnet_command(&root)
            .args(["run", "--project"])
            .arg(host.join("Host.csproj"))
            .args(["--configuration", "Release", "--"])
            .arg(&cases_path)
            .arg(index.to_string())
            .current_dir(&root)
            .isolated_output()?;
        require_success(&format!("generated C# host for {name}"), &csharp)?;
    }
    Ok(())
}
