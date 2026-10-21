use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conditional-group-presence")
        .join(name)
}

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn optional(name: &str, ty: ScalarType) -> SchemaNode {
    let mut node = scalar(name, ty);
    node.xml_optional = true;
    node
}

fn rows_source() -> SchemaNode {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            scalar("Flag", ScalarType::Bool),
            scalar("Label", ScalarType::String),
            scalar("Code", ScalarType::Int),
            scalar("Note", ScalarType::String),
            scalar("Other", ScalarType::String),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(0, Some(8)).unwrap())
    .unwrap();
    SchemaNode::group("Source", vec![rows])
}

fn record(fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Record", fields).repeating()
}

fn batch_target() -> SchemaNode {
    SchemaNode::group(
        "Result",
        vec![
            SchemaNode::group(
                "Batch",
                vec![record(vec![
                    optional("Type", ScalarType::String),
                    optional("Label", ScalarType::String),
                    optional("Note", ScalarType::String),
                ])],
            )
            .repeating(),
        ],
    )
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn field(name: &str, frame: Option<&str>) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: frame.map(|frame| vec![frame.into()]),
    }
}

fn conditional(condition: u32, then: u32, else_: u32) -> Node {
    Node::If {
        condition,
        then,
        else_,
    }
}

fn call(function: &str, args: Vec<u32>) -> Node {
    Node::Call {
        function: function.into(),
        args,
    }
}

fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn present(condition: u32, bindings: Vec<Binding>) -> Scope {
    Scope {
        iteration: ScopeIteration::OptionalPresence { condition },
        bindings,
        ..Default::default()
    }
}

fn project(
    source: SchemaNode,
    target: SchemaNode,
    nodes: Vec<(u32, Node)>,
    root: Scope,
) -> Project {
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        graph: Graph {
            nodes: nodes.into_iter().collect::<BTreeMap<_, _>>(),
        },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn batch_root(records: Scope) -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Batch".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            children: vec![Scope {
                target_field: "Record".into(),
                ..records
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn concatenated(first: Scope, rest: Vec<Scope>) -> Scope {
    Scope {
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, rest)),
        ..Default::default()
    }
}

/// Each clone's presence is `exists` of its lazily selected scalar; keyless
/// branches lower to Null and the group stays in the enclosing `Rows` frame.
fn nested_expected() -> Project {
    let rows = Some("Rows");
    project(
        rows_source(),
        batch_target(),
        vec![
            (0, constant(Value::String("first".into()))),
            (1, constant(Value::String("second".into()))),
            (2, constant(Value::String("third".into()))),
            (3, constant(Value::String("zero".into()))),
            (4, constant(Value::Int(0))),
            (5, constant(Value::Int(0))),
            (6, conditional(7, 8, 9)),
            (7, field("Flag", rows)),
            (8, field("Label", rows)),
            (9, constant(Value::Null)),
            (10, call("equal", vec![11, 4])),
            (11, field("Code", rows)),
            (12, conditional(10, 3, 13)),
            (13, field("Other", rows)),
            (14, call("is_empty", vec![15])),
            (15, field("Note", rows)),
            (16, conditional(7, 5, 17)),
            (17, constant(Value::Null)),
            (18, conditional(14, 19, 16)),
            (19, constant(Value::Null)),
            (20, call("exists", vec![6])),
            (21, call("exists", vec![12])),
            (22, call("exists", vec![18])),
        ],
        batch_root(concatenated(
            present(20, vec![bind("Type", 0), bind("Note", 15)]),
            vec![
                present(21, vec![bind("Type", 1), bind("Label", 8)]),
                present(22, vec![bind("Type", 2), bind("Note", 15)]),
            ],
        )),
    )
}

/// Root clones read the document frame directly; a literal `false` is present.
fn root_expected() -> Project {
    project(
        SchemaNode::group(
            "Source",
            vec![
                scalar("Flag", ScalarType::Bool),
                scalar("Label", ScalarType::String),
                scalar("Count", ScalarType::Int),
            ],
        ),
        SchemaNode::group(
            "Result",
            vec![record(vec![
                optional("Type", ScalarType::String),
                optional("Label", ScalarType::String),
            ])],
        ),
        vec![
            (0, constant(Value::String("flagged".into()))),
            (1, constant(Value::String("literal".into()))),
            (2, constant(Value::String("counted".into()))),
            (3, constant(Value::Bool(true))),
            (4, constant(Value::Bool(false))),
            (5, constant(Value::Int(0))),
            (6, conditional(7, 8, 9)),
            (7, field("Flag", None)),
            (8, field("Label", None)),
            (9, constant(Value::Null)),
            (10, conditional(3, 4, 11)),
            (11, constant(Value::Null)),
            (12, call("equal", vec![13, 5])),
            (13, field("Count", None)),
            (14, conditional(12, 15, 13)),
            (15, constant(Value::Null)),
            (16, call("exists", vec![6])),
            (17, call("exists", vec![10])),
            (18, call("exists", vec![14])),
        ],
        Scope {
            children: vec![Scope {
                target_field: "Record".into(),
                ..concatenated(
                    present(16, vec![bind("Type", 0), bind("Label", 8)]),
                    vec![
                        present(17, vec![bind("Type", 1)]),
                        present(18, vec![bind("Type", 2), bind("Label", 8)]),
                    ],
                )
            }],
            ..Default::default()
        },
    )
}

/// One driven clone beside an unconnected declaration; the inner condition is
/// evaluated only when the outer `empty` test selects it.
fn lazy_expected() -> Project {
    let rows = Some("Rows");
    project(
        rows_source(),
        batch_target(),
        vec![
            (0, constant(Value::String("lazy".into()))),
            (1, call("is_empty", vec![2])),
            (2, field("Note", rows)),
            (3, conditional(4, 5, 6)),
            (4, field("Flag", rows)),
            (5, field("Label", rows)),
            (6, constant(Value::Null)),
            (7, conditional(1, 8, 3)),
            (8, constant(Value::Null)),
            (9, call("exists", vec![7])),
        ],
        batch_root(concatenated(
            present(9, vec![bind("Type", 0), bind("Note", 2)]),
            vec![],
        )),
    )
}

fn expected(design: &str) -> Project {
    match design {
        "nested" | "reversed" => nested_expected(),
        "root" => root_expected(),
        "lazy" => lazy_expected(),
        _ => unreachable!("unknown design {design}"),
    }
}

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "fixtures/conditional-group-presence/cases.json"
    ))
    .unwrap()
}

fn case_design(design: &str) -> &str {
    if design == "reversed" {
        "nested"
    } else {
        design
    }
}

fn assert_cases(project: &Project, design: &str) {
    let mut checked = 0;
    for case in cases()
        .iter()
        .filter(|case| case["design"] == case_design(design))
    {
        checked += 1;
        let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
        if case["reject"] == true {
            assert!(source.is_err(), "{design}: {case}");
            continue;
        }
        let result = engine::run(project, &source.unwrap());
        if let Some(error) = case.get("boolean_error") {
            assert!(
                matches!(
                    &result,
                    Err(engine::EngineError::NotABool { node, found })
                        if u64::from(*node) == error["node"].as_u64().unwrap()
                            && *found == error["found"].as_str().unwrap()
                ),
                "{design}: {case}: {result:?}"
            );
            continue;
        }
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&project.target, &result.unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{design}: {case}");
    }
    assert!(checked > 0, "{design} has no cases");
}

#[test]
fn conditional_presence_clones_preserve_complete_projects_and_values() {
    for design in ["nested", "reversed", "root", "lazy"] {
        let mapping = fixture(&format!("{design}.mfd"));
        let imported = mfd::import(&mapping).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project).unwrap(),
            mapping::project_file::encode_pretty(&expected(design)).unwrap(),
            "{design}"
        );
        let executable = mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&executable.imported.project).unwrap(),
            serde_json::to_value(&imported.project).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        assert_cases(&imported.project, design);
    }
}

#[test]
fn conditional_presence_projects_round_trip_and_refuse_mfd_export() {
    let project = expected("nested");
    let text = mapping::project_file::encode_pretty(&project).unwrap();
    assert!(text.contains("\"kind\": \"optional_presence\""), "{text}");
    let decoded = mapping::project_file::decode_str(&text).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&project).unwrap()
    );
    let directory = std::env::temp_dir().join(format!(
        "ferrule_conditional_presence_export_{}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("exported.mfd");
    let error = mfd::export(&project, &target).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("optional-presence target scopes have no qualified .mfd export"),
        "{error}"
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir_all(directory).unwrap();
}

const REFUSAL: &str =
    "declared target clones in `Batch/Record` cannot prove conditional scalar presence";

struct Control {
    label: &'static str,
    design: String,
    reason: &'static str,
}

fn control(label: &'static str, design: String, reason: &'static str) -> Control {
    Control {
        label,
        design,
        reason,
    }
}

fn refusal_controls() -> Vec<Control> {
    let nested = std::fs::read_to_string(fixture("nested.mfd")).unwrap();
    let add = |component: &str| nested.replace("</children>", &format!("{component}</children>"));
    let structural = nested.replace(
        r#"<edge from="12" to="112"/>"#,
        r#"<edge from="10" to="112"/>"#,
    );
    let udf_call = r#"<component name="lookup" library="user" kind="19"><sources><datapoint pos="0" key="171"/></sources><targets><datapoint pos="0" key="179"/></targets></component>"#;
    let aggregate = r#"<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="171"/></sources><targets><datapoint pos="0" key="179"/></targets></component>"#;
    let clock = r#"<component name="now" library="lang" kind="5"><targets><datapoint pos="0" key="179"/></targets></component>"#;
    let concat = r#"<component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="171"/><datapoint pos="1" key="172"/></sources><targets><datapoint pos="0" key="179"/></targets></component>"#;
    let operand = |component: &str| {
        add(component)
            .replace(
                r#"<edge from="12" to="112"/>"#,
                r#"<edge from="179" to="112"/>"#,
            )
            .replace(
                "</connections>",
                r#"<edge from="12" to="171"/><edge from="14" to="172"/></connections>"#,
            )
    };
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="181"/><datapoint pos="1" key="182"/></sources><targets><datapoint key="189"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint key="188"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let cyclic = nested
        .replace(
            r#"<datapoint pos="0" key="161"/><datapoint pos="1" key="162"/><datapoint pos="2"/>"#,
            r#"<datapoint pos="0" key="161"/><datapoint pos="1" key="162"/><datapoint pos="2" key="163"/>"#,
        )
        .replace("</connections>", r#"<edge from="159" to="163"/></connections>"#);
    vec![
        control(
            "structural branch",
            structural,
            "a physical operand is outside the plain enclosing frame",
        ),
        control(
            "operand cycle",
            cyclic,
            "operands form a cycle",
        ),
        control(
            "malformed conditional pins",
            nested.replace(
                r#"<datapoint pos="0" key="111"/><datapoint pos="1" key="112"/><datapoint pos="2"/>"#,
                r#"<datapoint pos="0" key="111"/><datapoint pos="2" key="112"/><datapoint pos="1"/>"#,
            ),
            "an operator has malformed raw pins, metadata, or owners",
        ),
        control(
            "keyless condition",
            nested.replace(r#"<datapoint pos="0" key="111"/>"#, "<datapoint/>"),
            "an operator has malformed raw pins, metadata, or owners",
        ),
        control(
            "priority context",
            nested.replacen(
                r#"<component name="if-else" library="core" kind="4">"#,
                r#"<component name="if-else" library="core" kind="4" prioritycontext="5">"#,
                1,
            ),
            "an operator has malformed raw pins, metadata, or owners",
        ),
        control(
            "competing raw producer",
            add(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="119"/></targets><data><constant value="x" datatype="string"/></data></component>"#),
            "a clone driver has no unique declared producer",
        ),
        control(
            "competing operand producer",
            add(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="104"/></targets><data><constant value="x" datatype="string"/></data></component>"#),
            "an operator has malformed raw pins, metadata, or owners",
        ),
        control(
            "uniterated frame",
            nested.replace(r#"<edge from="10" to="40"/>"#, ""),
            "a physical operand is outside the plain enclosing frame",
        ),
        control(
            "controlled parent",
            add(filter).replace(
                r#"<edge from="10" to="40"/>"#,
                r#"<edge from="10" to="181"/><edge from="188" to="182"/><edge from="189" to="40"/>"#,
            ),
            "a parent or existing clone scope is controlled",
        ),
        control(
            "user function operand",
            operand(udf_call),
            "an operand has no unique declared producer",
        ),
        control(
            "aggregate operand",
            operand(aggregate),
            "an operand is not a literal, physical field, or admitted operator",
        ),
        control(
            "clock operand",
            operand(clock),
            "an operand is not a literal, physical field, or admitted operator",
        ),
        control(
            "computed operand",
            operand(concat),
            "an operand is not a literal, physical field, or admitted operator",
        ),
        control(
            "non-Boolean condition",
            nested.replace(r#"<edge from="11" to="111"/>"#, r#"<edge from="14" to="111"/>"#),
            "a conditional has no connected Boolean condition",
        ),
        control(
            "empty of a non-String operand",
            nested.replace(r#"<edge from="14" to="141"/>"#, r#"<edge from="13" to="141"/>"#),
            "an empty test requires one String operand",
        ),
        control(
            "equality of incompatible operands",
            nested.replace(r#"<edge from="105" to="132"/>"#, r#"<edge from="15" to="132"/>"#),
            "an equality compares incompatible scalar types",
        ),
        control(
            "branches of different types",
            nested.replace(r#"<edge from="104" to="122"/>"#, r#"<edge from="105" to="122"/>"#),
            "conditional branches have different types",
        ),
        control(
            "binding of a different type",
            nested.replace(r#"<edge from="101" to="51"/>"#, r#"<edge from="13" to="51"/>"#),
            "a clone binding type differs from its target",
        ),
        control(
            "connected clone without a driver",
            nested.replace(r#"<edge from="129" to="60"/>"#, ""),
            "a connected clone has no presence driver",
        ),
        control(
            "physical clone driver",
            nested.replace(r#"<edge from="129" to="60"/>"#, r#"<edge from="10" to="60"/>"#),
            "a clone driver is not a conditional presence driver",
        ),
        control(
            "keyed unconnected branch",
            nested.replace(
                r#"<datapoint pos="1" key="112"/><datapoint pos="2"/>"#,
                r#"<datapoint pos="1" key="112"/><datapoint pos="2" key="113"/>"#,
            ),
            "an operator has malformed raw pins, metadata, or owners",
        ),
        control(
            "operand depth bound",
            deep_conditional(&nested, 17),
            "operand depth or count exceeds the bound",
        ),
    ]
}

/// Replaces the first clone's driver with a chain of `depth` nested
/// conditionals, each selecting the next in its keyed then branch.
fn deep_conditional(nested: &str, depth: usize) -> String {
    let mut components = String::new();
    let mut edges = String::new();
    for level in 0..depth {
        let base = 1000 + level * 10;
        components.push_str(&format!(
            r#"<component name="if-else" library="core" kind="4"><sources><datapoint pos="0" key="{}"/><datapoint pos="1" key="{}"/><datapoint/></sources><targets><datapoint pos="0" key="{}"/></targets></component>"#,
            base + 1,
            base + 2,
            base + 9
        ));
        edges.push_str(&format!(r#"<edge from="11" to="{}"/>"#, base + 1));
        let then = if level + 1 == depth { 12 } else { base + 19 };
        edges.push_str(&format!(r#"<edge from="{then}" to="{}"/>"#, base + 2));
    }
    nested
        .replace("</children>", &format!("{components}</children>"))
        .replace(
            r#"<edge from="119" to="50"/>"#,
            r#"<edge from="1009" to="50"/>"#,
        )
        .replace("</connections>", &format!("{edges}</connections>"))
}

fn with_package<T>(label: &str, design: &str, run: impl FnOnce(&Path) -> T) -> T {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule_conditional_presence_{}_{}_{}",
        std::process::id(),
        label.replace(' ', "_"),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    for file in ["source.schema.json", "target.xsd"] {
        std::fs::copy(fixture(file), directory.join(file)).unwrap();
    }
    let mapping = directory.join("mapping.mfd");
    std::fs::write(&mapping, design).unwrap();
    let result = run(&mapping);
    std::fs::remove_dir_all(directory).unwrap();
    result
}

#[test]
fn unqualified_conditional_presence_drivers_refuse_once_and_keep_the_draft() {
    let mut mismatches = Vec::new();
    for Control {
        label,
        design,
        reason,
    } in refusal_controls()
    {
        with_package(label, &design, |mapping| {
            let imported = mfd::import(mapping).unwrap();
            let expected = format!("{REFUSAL} ({reason}); presence lowering skipped");
            let refusals = imported
                .warnings
                .iter()
                .filter(|warning| warning.starts_with(REFUSAL))
                .collect::<Vec<_>>();
            if refusals != [&expected] {
                mismatches.push(format!("{label}: {:?}", imported.warnings));
            }
            let text = mapping::project_file::encode_pretty(&imported.project).unwrap();
            assert!(!text.contains("optional_presence"), "{label}: {text}");
            assert!(
                matches!(
                    mfd::import_with_profile(
                        mapping,
                        &mfd::ImportOptions::default(),
                        mfd::ImportProfile::Executable
                    ),
                    Err(mfd::MfdError::IncompatibleImport(_))
                ),
                "{label}"
            );
        });
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// A refused driver leaves the ordinary repair draft exactly as the importer
/// builds it without this recognizer: each clone becomes an unconditional
/// segment over the enclosing collection, flagged by the refusal.
#[test]
fn refused_conditional_presence_keeps_the_ordinary_repair_draft() {
    let nested = std::fs::read_to_string(fixture("nested.mfd")).unwrap();
    let design = nested.replace(
        r#"<edge from="14" to="141"/>"#,
        r#"<edge from="13" to="141"/>"#,
    );
    with_package("ordinary draft", &design, |mapping| {
        let imported = mfd::import(mapping).unwrap();
        assert_eq!(
            imported.warnings,
            [format!(
                "{REFUSAL} (an empty test requires one String operand); presence lowering skipped"
            )]
        );
        let rows = Some("Rows");
        let segment = |bindings| Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            bindings,
            ..Default::default()
        };
        let draft = project(
            rows_source(),
            batch_target(),
            vec![
                (0, constant(Value::String("first".into()))),
                (1, constant(Value::String("second".into()))),
                (2, constant(Value::String("third".into()))),
                (8, field("Label", rows)),
                (19, field("Note", rows)),
            ],
            batch_root(concatenated(
                segment(vec![bind("Type", 0), bind("Note", 19)]),
                vec![
                    segment(vec![bind("Type", 1), bind("Label", 8)]),
                    segment(vec![bind("Type", 2), bind("Note", 19)]),
                ],
            )),
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project).unwrap(),
            mapping::project_file::encode_pretty(&draft).unwrap()
        );
    });
}
