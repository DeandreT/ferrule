//! The frozen computed join-key designs under
//! `docs/design/fixtures/join-computed-keys`: each admitted design imports
//! warning-free and reproduces its expected tuples, and each refused design
//! keeps the join and its driven scopes out with its own reason.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use engine::{ExecutionContext, RuntimeParameters};
use ir::{Instance, Value};
use mapping::{JoinPlan, Project, Scope, ScopeIteration};

use super::TempDir;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/join-computed-keys"
);

/// Designs the import still refuses, with the reason each warning names.
const REFUSED: &[(&str, &str)] = &[
    (
        "root-repeating",
        "the key reads `Order/CustomerNumber` across a repeating level outside its iteration",
    ),
    (
        "copy-all-computed",
        "input 0 takes a function or constant over a copy-all connection",
    ),
    (
        "key-read",
        "input 0 is a computed key with a connected output",
    ),
    (
        "unlisted-function",
        "input 0 key uses `normalize-space`, outside the admitted",
    ),
];

/// Designs whose key is sequence-valued.
const SEQUENCE: &[&str] = &["tokens", "tokens-second"];

/// Designs that keep today's singleton form rather than a computed key.
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

/// Copies one design beside the shared schemas and inputs it names.
fn stage(dir: &TempDir, design: &str) -> PathBuf {
    let fixtures = Path::new(FIXTURES);
    for file in [
        "source.xsd",
        "target.xsd",
        "input.xml",
        "input-numeric-text.xml",
    ] {
        fs::copy(fixtures.join(file), dir.0.join(file)).expect("fixture file copies");
    }
    let path = dir.0.join("mapping.mfd");
    fs::copy(fixtures.join(design), &path).expect("design copies");
    path
}

fn import_text(dir: &TempDir, text: &str) -> mfd::Imported {
    stage(dir, "designs/upper.mfd");
    let path = dir.0.join("mapping.mfd");
    fs::write(&path, text).expect("design writes");
    mfd::import(&path).expect("the design imports")
}

fn design_text(id: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(format!("designs/{id}.mfd")))
        .expect("design is readable")
}

fn join_plans(scope: &Scope, plans: &mut Vec<JoinPlan>) {
    if let ScopeIteration::InnerJoin { plan, .. } = &scope.iteration {
        plans.push(plan.clone());
    }
    for child in &scope.children {
        join_plans(child, plans);
    }
}

fn plans(project: &Project) -> Vec<JoinPlan> {
    let mut plans = Vec::new();
    join_plans(&project.root, &mut plans);
    plans
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

/// Matches per target order, or under `Hit` for the root collection.
fn tuples(output: &Instance, root: bool) -> Tuples {
    if root {
        return BTreeMap::from([("Hit".to_string(), pairs(output.field("Hit")))]);
    }
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

fn expected_tuples(case: &serde_json::Value) -> Tuples {
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

#[test]
fn admitted_computed_key_designs_reproduce_their_frozen_tuples() {
    let expected = expected();
    let mut admitted = 0;
    for case in expected["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        if REFUSED.iter().any(|(refused, _)| *refused == id) {
            continue;
        }
        admitted += 1;
        let dir = TempDir::new();
        let imported = mfd::import(&stage(&dir, case["design"].as_str().expect("design")))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        assert!(
            imported.warnings.is_empty(),
            "{id}: {:?}",
            imported.warnings
        );
        assert_eq!(engine::validate(&imported.project), [], "{id}");
        let [plan] = plans(&imported.project)
            .try_into()
            .unwrap_or_else(|plans: Vec<_>| {
                panic!("{id}: expected one join plan, found {}", plans.len())
            });
        assert_eq!(
            plan.computed_source().is_some(),
            !DIRECT.contains(&id),
            "{id}: {plan:?}"
        );
        assert_eq!(
            plan.computed_source()
                .and_then(|computed| computed.as_sequence())
                .is_some(),
            SEQUENCE.contains(&id),
            "{id}: {plan:?}"
        );
        let output = run(
            &imported.project,
            case["input"].as_str().expect("input name"),
        );
        let root = case["expected"].get("Hit").is_some();
        assert_eq!(tuples(&output, root), expected_tuples(case), "{id}");
    }
    assert_eq!(admitted, 16);
}

/// A single-valued key gives the same tuples as input 1, after `Customer`,
/// as the `tokens-second` wiring shows for the input order.
#[test]
fn a_computed_key_as_the_second_input_keeps_its_tuples() {
    let text = design_text("upper")
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
        );
    let dir = TempDir::new();
    let imported = import_text(&dir, &text);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(engine::validate(&imported.project), []);
    let [plan] = plans(&imported.project).try_into().expect("one join plan");
    let (right, _) = plan.stages().next().expect("one stage");
    assert!(right.as_computed().is_some(), "{plan:?}");
    let upper = expected()["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == "upper")
        .expect("the upper case")
        .clone();
    assert_eq!(
        tuples(&run(&imported.project, "input.xml"), false),
        expected_tuples(&upper)
    );
}

#[test]
fn refused_computed_key_designs_keep_the_join_out_with_their_reason() {
    for (id, reason) in REFUSED {
        let dir = TempDir::new();
        let imported = mfd::import(&stage(&dir, &format!("designs/{id}.mfd")))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains(reason)),
            "{id}: {:?}",
            imported.warnings
        );
        assert_eq!(plans(&imported.project), [], "{id}");
    }
}

/// A design variant that the observed set or the bounds refuse.
fn assert_refused(text: &str, reason: &str) {
    let dir = TempDir::new();
    let imported = import_text(&dir, text);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains(reason)),
        "{reason}: {:?}",
        imported.warnings
    );
    assert_eq!(plans(&imported.project), [], "{reason}");
}

#[test]
fn key_computations_that_could_raise_stay_refused() {
    assert_refused(
        &design_text("numeric").replace(r#"datatype="decimal""#, r#"datatype="integer""#),
        "input 0 key function `add` over integers alone can overflow",
    );
    assert_refused(
        &design_text("numeric").replace(
            r#"<vertex vertexkey="14"><edges><edge vertexkey="200"/>"#,
            r#"<vertex vertexkey="12"><edges><edge vertexkey="200"/>"#,
        ),
        "input 0 key function `add` over a string value can raise",
    );
    assert_refused(
        &design_text("upper").replace(
            r#"<vertex vertexkey="12"><edges><edge vertexkey="200"/>"#,
            r#"<vertex vertexkey="14"><edges><edge vertexkey="200"/>"#,
        ),
        "input 0 key function `uppercase` over an integer value can raise",
    );
    assert_refused(
        &design_text("concat").replace(
            r#"<constant value="" datatype="string"/>"#,
            r#"<constant value="0" datatype="decimal"/>"#,
        ),
        "input 0 key function `concat` over a decimal value has an unobserved text form",
    );
}

#[test]
fn unobserved_join_shapes_with_a_computed_key_stay_refused() {
    // Both inputs fed by the same constant.
    assert_refused(
        &design_text("constant")
            .replace(
                r#"<vertex vertexkey="20"><edges><edge vertexkey="61" edgekey="961"/></edges></vertex>"#,
                "",
            )
            .replace(
                r#"<edge vertexkey="60"/>"#,
                r#"<edge vertexkey="60"/><edge vertexkey="61"/>"#,
            ),
        "inputs 0 and 1 are both computed keys",
    );
    // A third input joins `Customer` again.
    assert_refused(
        &design_text("upper")
            .replace(
                "\n</entry></entry></root>\n<join>",
                "\n<entry name=\"dynamic_tree_node2\" ns=\"1\"><entry name=\"Customer\" inpkey=\"70\"/></entry>\n</entry></entry></root>\n<join>",
            )
            .replace(
                "</keypair></joinkeys>",
                r#"</keypair><keypair><first-key path-id="2" input-index="1"/><second-key path-id="2" input-index="2"/></keypair></joinkeys>"#,
            )
            .replace(
                r#"<edge vertexkey="61" edgekey="961"/>"#,
                r#"<edge vertexkey="61" edgekey="961"/><edge vertexkey="70"/>"#,
            ),
        "input 0 is a computed key in a join of 3 inputs",
    );
    // The joined tuples feed a count per order instead of a target iteration.
    assert_refused(
        &design_text("root-constant")
            .replace(
                "</children>",
                r#"<component name="count" library="core" uid="9" kind="5"><sources><datapoint/><datapoint pos="1" key="300"/></sources><targets><datapoint pos="0" key="301"/></targets></component>
</children>"#,
            )
            .replace(
                r#"<vertex vertexkey="62"><edges><edge vertexkey="50"/></edges></vertex>
<vertex vertexkey="63"><edges><edge vertexkey="51"/></edges></vertex>
<vertex vertexkey="64"><edges><edge vertexkey="52"/></edges></vertex>"#,
                r#"<vertex vertexkey="10"><edges><edge vertexkey="40"/></edges></vertex>
<vertex vertexkey="65"><edges><edge vertexkey="300"/></edges></vertex>
<vertex vertexkey="301"><edges><edge vertexkey="41"/></edges></vertex>"#,
            ),
        "a computed join key feeding a joined aggregate is not supported",
    );
    // The key beside a singleton scalar rather than a collection.
    assert_refused(
        &design_text("upper").replace(
            r#"<vertex vertexkey="20"><edges><edge vertexkey="61" edgekey="961"/></edges></vertex>"#,
            r#"<vertex vertexkey="30"><edges><edge vertexkey="61" edgekey="961"/></edges></vertex>"#,
        ),
        "input 1 beside a computed key must be a repeating source collection",
    );
}

#[test]
fn keys_over_named_sources_stay_refused() {
    let text = design_text("upper");
    let start = text
        .find(r#"<component name="Source""#)
        .expect("source component");
    let end = start + text[start..].find("</component>").expect("component end") + 12;
    let mut other = text[start..end]
        .replace(r#"name="Source" library"#, r#"name="Other" library"#)
        .replace(r#"uid="2""#, r#"uid="7""#);
    for key in [30, 10, 11, 12, 13, 14, 20, 21, 22, 23] {
        other = other.replace(
            &format!(r#"outkey="{key}""#),
            &format!(r#"outkey="9{key}""#),
        );
    }
    let text = text
        .replacen("</children>", &format!("{other}\n</children>"), 1)
        .replace(
            r#"<vertex vertexkey="12"><edges><edge vertexkey="200"/>"#,
            r#"<vertex vertexkey="912"><edges><edge vertexkey="200"/>"#,
        );
    assert_refused(
        &text,
        "input 0 key reads named source `Other`; computed keys over named or dynamic sources are not supported",
    );
}

/// Sequence-valued keys outside the observed `tokenize` form keep the join
/// out with their own reason.
#[test]
fn unobserved_sequence_keys_stay_refused() {
    let tokens = design_text("tokens");
    for (name, library) in [("tokenize-by-length", "core"), ("tokenize-regexp", "core")] {
        assert_refused(
            &tokens.replace(
                r#"<component name="tokenize" library="core""#,
                &format!(r#"<component name="{name}" library="{library}""#),
            ),
            &format!("input 0 key is a `{name}` sequence; only tokenize keys were observed"),
        );
    }
    // The delimiter is a field rather than a constant, or an empty constant.
    assert_refused(
        &tokens
            .replace(
                r#"<vertex vertexkey="211"><edges><edge vertexkey="202"/>"#,
                r#"<vertex vertexkey="11"><edges><edge vertexkey="41"/><edge vertexkey="202"/>"#,
            )
            .replace(
                r#"<vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex>"#,
                "",
            ),
        "input 0 key tokenize delimiter is not a non-empty string constant",
    );
    assert_refused(
        &tokens.replace(r#"<constant value=" ""#, r#"<constant value="""#),
        "input 0 key tokenize delimiter is not a non-empty string constant",
    );
    // Tokens of an integer were not observed.
    assert_refused(
        &tokens.replace(
            r#"<vertex vertexkey="13"><edges><edge vertexkey="200"/>"#,
            r#"<vertex vertexkey="14"><edges><edge vertexkey="200"/>"#,
        ),
        "input 0 key tokenizes a non-string value",
    );
    // A sequence inside a scalar computation is not a key of its own.
    assert_refused(
        &tokens
            .replace(
                r#"<vertex vertexkey="201"><edges><edge vertexkey="60"/>"#,
                r#"<vertex vertexkey="201"><edges><edge vertexkey="220"/>"#,
            )
            .replace(
                "</children>",
                r#"<component name="uppercase" library="lang" uid="7" kind="5"><sources><datapoint pos="0" key="220"/></sources><targets><datapoint pos="0" key="221"/></targets></component>
</children>"#,
            )
            .replace(
                "</vertices>",
                r#"<vertex vertexkey="221"><edges><edge vertexkey="60"/></edges></vertex>
</vertices>"#,
            ),
        "input 0 key reads sequence `tokenize` inside its computation",
    );
    // At the root, the codes of every order would be a sequence of sequences.
    assert_refused(
        &tokens
            .replace(
                r#"<vertex vertexkey="10"><edges><edge vertexkey="40"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex>
"#,
                "",
            )
            .replace(r#"<edge vertexkey="42"/>"#, r#"<edge vertexkey="50"/>"#)
            .replace(r#"<edge vertexkey="43"/>"#, r#"<edge vertexkey="51"/>"#)
            .replace(r#"<edge vertexkey="44"/>"#, r#"<edge vertexkey="52"/>"#),
        "the key reads `Order/Codes` across a repeating level outside its iteration",
    );
}
