//! Export of the frozen computed join-key designs under
//! `docs/design/fixtures/join-computed-keys`: every design import admits
//! survives two export and re-import round trips with the same plan and the
//! same tuples, and every other computed key is refused before anything is
//! written.

use std::fs;
use std::path::Path;

use engine::{ExecutionContext, RuntimeParameters};
use ir::{Instance, ScalarType, Value};
use mapping::{
    AggregateOp, ComputedJoinSourceId, Graph, JoinConditions, JoinId, JoinKey, JoinPlan,
    JoinSource, Node, NodeId, Project, Scope, ScopeIteration,
};

use super::{TempDir, assert_join_input_modes};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/join-computed-keys"
);

/// Designs import refuses; export never sees them.
const REFUSED: &[&str] = &[
    "tokens",
    "tokens-second",
    "root-repeating",
    "copy-all-computed",
    "key-read",
    "unlisted-function",
];

/// Designs whose key is the existing singleton form rather than computed.
const DIRECT: &[&str] = &[
    "direct",
    "integer-direct",
    "rank-direct",
    "rank-text-direct",
];

fn expected() -> serde_json::Value {
    let text = fs::read_to_string(Path::new(FIXTURES).join("expected.json"))
        .expect("expected.json is readable");
    serde_json::from_str(&text).expect("expected.json parses")
}

fn design_text(id: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(format!("designs/{id}.mfd")))
        .expect("design is readable")
}

/// Imports a design written beside the shared schemas.
fn import_design(text: &str) -> Project {
    let dir = TempDir::new("computed-key-design");
    for file in ["source.xsd", "target.xsd"] {
        fs::copy(Path::new(FIXTURES).join(file), dir.path(file)).expect("fixture file copies");
    }
    let path = dir.path("mapping.mfd");
    fs::write(&path, text).expect("design writes");
    let imported = mfd::import(&path).expect("the design imports");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(engine::validate(&imported.project), []);
    imported.project
}

/// Exports `project` and imports it back, warning-free both ways.
fn round_trip(project: &Project, id: &str) -> (Project, String) {
    let dir = TempDir::new("computed-key-round-trip");
    let path = dir.path("mapping.mfd");
    let warnings = mfd::export(project, &path).unwrap_or_else(|error| panic!("{id}: {error}"));
    assert!(warnings.is_empty(), "{id}: {warnings:?}");
    let xml = fs::read_to_string(&path).expect("export writes the design");
    let imported = mfd::import(&path).unwrap_or_else(|error| panic!("{id}: {error}"));
    assert!(
        imported.warnings.is_empty(),
        "{id}: {:?}",
        imported.warnings
    );
    assert_eq!(engine::validate(&imported.project), [], "{id}");
    (imported.project, xml)
}

fn run(project: &Project, input: &str) -> Instance {
    let text = fs::read_to_string(Path::new(FIXTURES).join(input)).expect("input is readable");
    let source = format_xml::from_str(&text, &project.source).expect("input parses");
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("Key", Value::String("C2".into()))
        .expect("a valid parameter");
    let execution = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    engine::run_with_context(project, &source, &execution).expect("the design runs")
}

fn join_scopes<'s>(scope: &'s Scope, found: &mut Vec<&'s Scope>) {
    if scope.join().is_some() {
        found.push(scope);
    }
    for child in &scope.children {
        join_scopes(child, found);
    }
}

fn join_scope_mut(scope: &mut Scope) -> Option<&mut Scope> {
    if scope.join().is_some() {
        return Some(scope);
    }
    scope.children.iter_mut().find_map(join_scope_mut)
}

/// A node with its inputs spelled out, so plans compare without node ids.
fn describe(graph: &Graph, id: NodeId) -> String {
    let Some(node) = graph.nodes.get(&id) else {
        return format!("missing {id}");
    };
    match node {
        Node::Call { function, args } => format!(
            "{function}({})",
            args.iter()
                .map(|arg| describe(graph, *arg))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Node::If {
            condition,
            then,
            else_,
        } => format!(
            "if({}, {}, {})",
            describe(graph, *condition),
            describe(graph, *then),
            describe(graph, *else_)
        ),
        other => format!("{other:?}"),
    }
}

/// Each join's target field, sources, key expression and conditions.
fn canonical_plans(project: &Project) -> Vec<String> {
    let mut scopes = Vec::new();
    join_scopes(&project.root, &mut scopes);
    scopes
        .into_iter()
        .map(|scope| {
            let (_, plan) = scope.join().expect("a join scope");
            let sources = plan
                .sources()
                .map(|source| match source {
                    JoinSource::Collection(source) => format!("{source:?}"),
                    JoinSource::Computed(source) => format!(
                        "computed {} = {}",
                        source.id(),
                        describe(&project.graph, source.value())
                    ),
                })
                .collect::<Vec<_>>();
            let stages = plan
                .stages()
                .map(|(_, conditions)| format!("{conditions:?}"))
                .collect::<Vec<_>>();
            format!(
                "{} {:?} {sources:?} {stages:?}",
                scope.target_field, scope.iteration_output
            )
        })
        .collect()
}

/// Asserts the computed input's exported wiring: its entry name and an
/// ordinary connection for a function or constant, the parameter's name and
/// a copy-all connection for a parameter, and no guard components.
fn assert_computed_wiring(xml: &str, id: &str, computed_index: usize, parameter: bool) {
    let mut copy_all = [true, true];
    copy_all[computed_index] = parameter;
    assert_join_input_modes(xml, &copy_all);
    let name = if parameter { "Key" } else { "value" };
    assert!(
        xml.contains(&format!(
            "<entry name=\"dynamic_tree_node{computed_index}\" ns=\"1\"><entry name=\"{name}\" inpkey="
        )),
        "{id}: {xml}"
    );
    for guard in [
        "\"exists\"",
        "\"is-xsi-nil\"",
        "\"if-else\"",
        "\"set-empty\"",
    ] {
        assert!(!xml.contains(guard), "{id} writes the null guard: {xml}");
    }
}

/// Runs one admitted design through two export and re-import round trips.
fn assert_round_trips(id: &str, design: &Project, input: &str, computed_index: Option<usize>) {
    let plans = canonical_plans(design);
    let output = run(design, input);
    let (first, xml) = round_trip(design, id);
    if let Some(index) = computed_index {
        assert_computed_wiring(&xml, id, index, id == "parameter");
    }
    assert_eq!(canonical_plans(&first), plans, "{id}: first round trip");
    assert_eq!(run(&first, input), output, "{id}: first round trip");
    let (second, _) = round_trip(&first, id);
    assert_eq!(canonical_plans(&second), plans, "{id}: second round trip");
    assert_eq!(run(&second, input), output, "{id}: second round trip");
}

#[test]
fn admitted_computed_key_designs_round_trip_twice_with_their_plans_and_tuples() {
    let expected = expected();
    let mut admitted = 0;
    for case in expected["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        if REFUSED.contains(&id) {
            continue;
        }
        admitted += 1;
        let design = import_design(&design_text(id));
        let computed = !DIRECT.contains(&id);
        let mut scopes = Vec::new();
        join_scopes(&design.root, &mut scopes);
        let [scope] = scopes.as_slice() else {
            panic!("{id}: expected one join scope");
        };
        let (_, plan) = scope.join().expect("a join scope");
        assert_eq!(plan.computed_source().is_some(), computed, "{id}");
        assert_round_trips(
            id,
            &design,
            case["input"].as_str().expect("input name"),
            computed.then_some(0),
        );
    }
    assert_eq!(admitted, 14);
}

/// The `upper` design with the key as input 1, after `Customer`.
fn upper_second() -> String {
    design_text("upper")
        .replace(
            r#"<entry name="dynamic_tree_node0" ns="1" expanded="1"><entry name="result""#,
            r#"<entry name="dynamic_tree_node9" ns="1" expanded="1"><entry name="result""#,
        )
        .replace(
            r#"<entry name="dynamic_tree_node1" ns="1" expanded="1"><entry name="Customer""#,
            r#"<entry name="dynamic_tree_node0" ns="1" expanded="1"><entry name="Customer""#,
        )
        .replace("dynamic_tree_node9", "dynamic_tree_node1")
        .replace(
            r#"<first-key path-id="1" input-index="0"/><second-key path-id="2" input-index="1"/>"#,
            r#"<first-key path-id="2" input-index="0"/><second-key path-id="1" input-index="1"/>"#,
        )
}

#[test]
fn a_computed_key_as_the_second_input_round_trips_in_its_place() {
    let design = import_design(&upper_second());
    let mut scopes = Vec::new();
    join_scopes(&design.root, &mut scopes);
    let (_, plan) = scopes[0].join().expect("a join scope");
    assert!(
        plan.sources()
            .nth(1)
            .is_some_and(|source| source.as_computed().is_some())
    );
    assert_round_trips("upper-second", &design, "input.xml", Some(1));
}

/// The key node of the design's only join, and the call below its guard.
fn key_nodes(project: &Project) -> (NodeId, NodeId) {
    let mut scopes = Vec::new();
    join_scopes(&project.root, &mut scopes);
    let (_, plan) = scopes[0].join().expect("a join scope");
    let key = plan.computed_source().expect("a computed key").value();
    let Some(Node::If { then, .. }) = project.graph.nodes.get(&key) else {
        return (key, key);
    };
    let Some(Node::If { else_, .. }) = project.graph.nodes.get(then) else {
        panic!("the guard has no nil test");
    };
    (key, *else_)
}

/// The one source field the guarded call reads.
fn key_field(project: &Project) -> NodeId {
    let (_, call) = key_nodes(project);
    let Some(Node::Call { args, .. }) = project.graph.nodes.get(&call) else {
        panic!("the key is not a call");
    };
    args[0]
}

fn replace_key(project: &mut Project, value: NodeId) {
    let scope = join_scope_mut(&mut project.root).expect("a join scope");
    let ScopeIteration::InnerJoin { plan, .. } = &mut scope.iteration else {
        panic!("not an inner join");
    };
    let sources = plan.sources().cloned().collect::<Vec<_>>();
    let stages = plan
        .stages()
        .map(|(_, conditions)| conditions.clone())
        .collect::<Vec<_>>();
    let replace = |source: JoinSource| match source {
        JoinSource::Computed(source) => JoinSource::computed(source.id(), value),
        other => other,
    };
    let mut sources = sources.into_iter().map(replace);
    *plan = JoinPlan::new(
        sources.next().expect("first source"),
        sources.next().expect("second source"),
        stages[0].clone(),
    )
    .expect("the plan keeps its shape");
}

fn assert_refused(project: &Project, reason: &str) {
    let dir = TempDir::new("computed-key-refused");
    let path = dir.path("mapping.mfd");
    fs::write(&path, "sentinel").expect("sentinel writes");
    let error = mfd::export(project, &path).expect_err(reason);
    assert!(
        matches!(&error, mfd::MfdError::Unsupported(message) if message.contains(reason)),
        "{reason}: {error}"
    );
    assert_eq!(
        fs::read_to_string(&path).expect("sentinel reads"),
        "sentinel"
    );
    let written = fs::read_dir(&dir.0)
        .expect("the directory reads")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    assert_eq!(written, ["mapping.mfd"], "{reason}");
}

#[test]
fn computed_keys_import_would_not_read_back_are_refused_before_anything_is_written() {
    let upper = import_design(&design_text("upper"));

    let mut project = upper.clone();
    let (_, call) = key_nodes(&project);
    let Some(Node::Call { function, .. }) = project.graph.nodes.get_mut(&call) else {
        panic!("the key is not a call");
    };
    *function = "normalize_space".into();
    assert_refused(&project, "`normalize_space` is outside the admitted");

    let mut project = upper.clone();
    let field = key_field(&project);
    project.graph.nodes.insert(
        field,
        Node::SourceField {
            path: vec!["Qty".into()],
            frame: None,
        },
    );
    assert_refused(&project, "`upper` over an integer value can raise");

    // The bare call without the null guard import would add on re-import.
    let mut project = upper.clone();
    let (_, call) = key_nodes(&project);
    replace_key(&mut project, call);
    assert_refused(
        &project,
        "null guard covers no field but the key reads `CustomerNumber`",
    );

    // A guarded bare field would come back as the singleton form.
    let mut project = upper.clone();
    let (_, call) = key_nodes(&project);
    let field = key_field(&project);
    let read = project.graph.nodes[&field].clone();
    project.graph.nodes.insert(call, read);
    assert_refused(&project, "bare source field");

    // A guard node another expression also reads.
    let mut project = upper.clone();
    let (key, _) = key_nodes(&project);
    project.graph.nodes.insert(
        9_000,
        Node::Call {
            function: "string".into(),
            args: vec![key],
        },
    );
    let scope = join_scope_mut(&mut project.root).expect("a join scope");
    scope.filter = Some(9_000);
    assert_refused(&project, "null guard is shared");

    // A key read across repetition at the root is a sequence natively.
    let mut project = import_design(&design_text("root-header"));
    let field = key_field(&project);
    project.graph.nodes.insert(
        field,
        Node::SourceField {
            path: vec!["Order".into(), "CustomerNumber".into()],
            frame: None,
        },
    );
    assert_refused(&project, "across a repeating level");

    // An optional runtime parameter was not observed.
    let mut project = import_design(&design_text("parameter"));
    let (key, _) = key_nodes(&project);
    project.graph.nodes.insert(
        9_001,
        Node::Const {
            value: Value::String("C2".into()),
        },
    );
    project.graph.nodes.insert(
        key,
        Node::RuntimeParameterDefault {
            name: "Key".into(),
            ty: ScalarType::String,
            default: 9_001,
            preview: None,
        },
    );
    assert_refused(&project, "an optional runtime parameter");
}

#[test]
fn computed_keys_outside_a_target_iteration_or_beside_another_partner_are_refused() {
    let upper = import_design(&design_text("upper"));
    let (key, _) = key_nodes(&upper);
    let id = ComputedJoinSourceId::new(1);

    // Beside a singleton rather than a repeating collection.
    let mut project = upper.clone();
    let scope = join_scope_mut(&mut project.root).expect("a join scope");
    scope.iteration = ScopeIteration::InnerJoin {
        id: JoinId::new(77),
        plan: JoinPlan::new(
            JoinSource::computed(id, key),
            JoinSource::singleton(vec!["Header".into(), "Code".into()]),
            JoinConditions::new(JoinKey::computed(id, Vec::new())),
        )
        .expect("a valid plan"),
    };
    assert_refused(
        &project,
        "inner join 77 with a computed key cannot be exported",
    );

    // Feeding a joined aggregate.
    let mut project = upper.clone();
    let mut scopes = Vec::new();
    join_scopes(&project.root, &mut scopes);
    let (join, plan) = scopes[0].join().expect("a join scope");
    let plan = plan.clone();
    project.graph.nodes.insert(
        9_002,
        Node::JoinAggregate {
            function: AggregateOp::Count,
            join,
            plan,
            expression: None,
            arg: None,
        },
    );
    assert_refused(&project, "computed key feeding a joined aggregate");
}
