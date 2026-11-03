//! Complete baseline observations before semantic assertions, using public APIs.

use std::path::Path;

use mapping::{AggregateOp, Node};
use serde_json::{Value, json};

fn save(name: &str, contents: &str) {
    if let Some(root) = std::env::var_os("FERRULE_FILTERED_COUNT_BASELINE_ROOT") {
        let root = std::path::PathBuf::from(root);
        assert!(root.is_absolute());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(name), contents).unwrap();
    }
}

fn admission(label: &str, path: &Path) -> (mfd::Imported, Value) {
    let imported = mfd::import(path).unwrap();
    let strict = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    );
    let (strict_admitted, strict_category, strict_detail) = match strict {
        Ok(_) => (true, "Admitted", String::new()),
        Err(mfd::MfdError::IncompatibleImport(report)) => {
            (false, "IncompatibleImport", format!("{report:?}"))
        }
        Err(error) => (false, "UnexpectedImportError", format!("{error:?}")),
    };
    let graph = format!("{:#?}", imported.project.graph);
    let observation = json!({
        "label": label,
        "warnings": imported.warnings,
        "validation": format!("{:?}", engine::validate(&imported.project)),
        "executable": mfd::assess_import(&imported).executable,
        "strictAdmitted": strict_admitted,
        "strictCategory": strict_category,
        "strictDetail": strict_detail,
        "bareCounts": imported.project.graph.nodes.values().filter(|node| matches!(node,
            Node::Aggregate { function: AggregateOp::Count, expression: None, .. }
        )).count(),
        "conditionalSums": imported.project.graph.nodes.values().filter(|node| matches!(node,
            Node::Aggregate { function: AggregateOp::Sum, expression: Some(_), .. }
        )).count(),
        "calls": imported.project.graph.nodes.values().filter_map(|node| match node {
            Node::Call { function, .. } => Some(function),
            _ => None,
        }).collect::<Vec<_>>(),
    });
    // The original imported graph is written before parsing any data or
    // comparing arithmetic, so predicate loss is distinct from a wrong total.
    let file = label.replace(|character: char| !character.is_ascii_alphanumeric(), "-");
    save(&format!("{file}.graph.txt"), &graph);
    save(
        &format!("{file}.project.json"),
        &mapping::project_file::encode_pretty(&imported.project).unwrap(),
    );
    println!("{observation}");
    (imported, observation)
}

fn observe_output(
    label: &str,
    project: &mapping::Project,
    input: ir::Instance,
    expected: &Value,
    expected_xml: Option<&str>,
) -> Value {
    let before = input.clone();
    match engine::run(project, &input) {
        Ok(output) => {
            let serialized = format_json::to_string(&project.target, &output).unwrap();
            let actual: Value = serde_json::from_str(&serialized).unwrap();
            // Parsing the literal oracle with the target schema checks the
            // entire typed tree, including every nested item and integer type.
            let expected_typed =
                format_json::from_str(&serde_json::to_string(expected).unwrap(), &project.target)
                    .unwrap();
            let xml =
                expected_xml.map(|_| format_xml::to_string(&project.target, &output).unwrap());
            let xml_matches = expected_xml.is_none_or(|expected| {
                let expected = format_xml::from_str(expected, &project.target).unwrap();
                let actual =
                    format_xml::from_str(xml.as_deref().unwrap(), &project.target).unwrap();
                actual == expected && actual == output
            });
            let matched =
                actual == *expected && output == expected_typed && input == before && xml_matches;
            json!({"case":label,"stage":"mapping","typed":format!("{output:?}"),
                "output":actual,"expected":expected,"xml":xml,"expectedXml":expected_xml,
                "matchesWholeOracle":matched,"matchesXmlTree":xml_matches,
                "inputUnchanged":input == before})
        }
        Err(error) => json!({"case":label,"stage":"mapping","error":format!("{error:?}"),
            "expected":expected,"matchesWholeOracle":false}),
    }
}

#[test]
fn guarded_baseline_records_all_graphs_whole_outputs_input_categories_and_refusals() {
    super::preflight_all();
    let mut admissions = Vec::new();
    let mut outcomes = Vec::new();
    let mut differences = Vec::new();
    for cohort in super::cohorts() {
        super::preflight_path(cohort.design, &super::fixture(cohort.design));
        let (imported, observed) = admission(cohort.design, &super::fixture(cohort.design));
        if !observed["strictAdmitted"].as_bool().unwrap()
            || observed["bareCounts"].as_u64().unwrap() != 0
            || observed["conditionalSums"].as_u64().unwrap() == 0
        {
            differences.push(format!("{} graph/admission", cohort.name));
        }
        admissions.push(observed);
        let cases: Vec<Value> = serde_json::from_str(cohort.cases).unwrap();
        for case in cases {
            let input =
                super::parse_input(cohort, &imported.project, case["input"].as_str().unwrap())
                    .unwrap();
            let observation = observe_output(
                case["name"].as_str().unwrap(),
                &imported.project,
                input,
                &case["expected"],
                None,
            );
            if observation["matchesWholeOracle"] != true {
                differences.push(case["name"].as_str().unwrap().to_owned());
            }
            println!("{observation}");
            outcomes.push(observation);
        }
        let invalid: Vec<Value> = serde_json::from_str(cohort.rejections).unwrap();
        for case in invalid {
            let parsed =
                super::parse_input(cohort, &imported.project, case["input"].as_str().unwrap());
            let observation = match parsed {
                Err(error) => json!({"case":case["name"],"stage":"input",
                    "category":error.category(),"expectedCategory":case["nativeCategory"],
                    "error":format!("{error:?}"),"matchesCategory":error.category() == case["nativeCategory"].as_str().unwrap()}),
                Ok(input) => json!({"case":case["name"],"stage":"input",
                    "unexpectedInput":format!("{input:?}"),"matchesCategory":false}),
            };
            if observation["matchesCategory"] != true {
                differences.push(case["name"].as_str().unwrap().to_owned());
            }
            println!("{observation}");
            outcomes.push(observation);
        }
    }
    for (predicate, expected) in [
        (21, json!({"Included":2,"Excluded":0})),
        (22, json!({"Included":0,"Excluded":2})),
    ] {
        let package = super::Package::new();
        std::fs::write(
            package.0.join("refusal-source.xsd"),
            super::replace_one(
                include_str!("../fixtures/filtered-counts/refusal-source.xsd"),
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\"",
                "name=\"Text\" type=\"xs:string\" minOccurs=\"0\" nillable=\"true\"",
            ),
        )
        .unwrap();
        let label = format!("literal predicate {predicate}");
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, super::selection_design("", "", predicate)).unwrap();
        super::preflight_path(&label, &path);
        let (imported, graph) = admission(&label, &path);
        admissions.push(graph);
        let input = format_xml::from_str("<Source xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'><Record><Text xsi:nil='true'/><Other>x</Other></Record><Record><Text/><Other>x</Other></Record><Record><Other>x</Other></Record></Source>", &imported.project.source).unwrap();
        let observation = observe_output(&label, &imported.project, input, &expected, None);
        if observation["matchesWholeOracle"] != true {
            differences.push(label);
        }
        println!("{observation}");
        outcomes.push(observation);
    }
    let nil_labels: Vec<String> = super::nil_designs()
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    for (label, text) in super::all_rendered_designs() {
        if [
            "guarded-counts.mfd",
            "guarded-bool-counts.mfd",
            "guarded-edi-counts.mfd",
        ]
        .contains(&label.as_str())
            || label.starts_with("literal predicate ")
        {
            continue;
        }
        let package = super::Package::new();
        if nil_labels.contains(&label) {
            let schema = super::fixture("refusal-source.xsd");
            std::fs::write(
                package.0.join("refusal-source.xsd"),
                super::replace_one(
                    &std::fs::read_to_string(schema).unwrap(),
                    "name=\"Text\" type=\"xs:string\" minOccurs=\"0\"",
                    "name=\"Text\" type=\"xs:string\" minOccurs=\"0\" nillable=\"true\"",
                ),
            )
            .unwrap();
        }
        if label == "same-field nillable direct Bool" {
            std::fs::write(
                package.0.join("guarded-bool-source.xsd"),
                super::replace_one(
                    include_str!("../fixtures/filtered-counts/guarded-bool-source.xsd"),
                    "name=\"Enabled\" type=\"xs:boolean\" minOccurs=\"0\"",
                    "name=\"Enabled\" type=\"xs:boolean\" minOccurs=\"0\" nillable=\"true\"",
                ),
            )
            .unwrap();
        }
        if label == "repeating X12 scalar including literal predicate" {
            std::fs::write(
                package.0.join("940.Config"),
                super::replace_one(
                    include_str!("../fixtures/filtered-counts/940.Config"),
                    "name=\"Tag\" type=\"string\" minOccurs=\"0\"",
                    "name=\"Tag\" type=\"string\" minOccurs=\"0\" maxOccurs=\"unbounded\"",
                ),
            )
            .unwrap();
        }
        let path = package.0.join("selection.mfd");
        std::fs::write(&path, text).unwrap();
        super::preflight_path(&label, &path);
        // The retained pre-port receipts remain immutable. The new publication
        // boundary rejects the import itself, not an executable repair graph.
        super::assert_refused(&label, &path);
        let mut observation =
            json!({"label":label,"strictCategory":"UnsupportedImport","executable":false});
        let matched = true;
        observation["expectedRefusal"] = json!(true);
        observation["matchesRefusal"] = json!(matched);
        if !matched {
            differences.push(format!("refusal: {label}"));
        }
        admissions.push(observation);
    }
    save(
        "guarded-admission.json",
        &serde_json::to_string_pretty(&admissions).unwrap(),
    );
    save(
        "guarded-outcomes.json",
        &serde_json::to_string_pretty(&outcomes).unwrap(),
    );
    save(
        "guarded-differences.json",
        &serde_json::to_string_pretty(&differences).unwrap(),
    );
    assert!(
        differences.is_empty(),
        "complete baseline differences: {differences:?}"
    );
}

#[test]
fn fresh_counted_field_xml_baseline_keeps_whole_nested_xml_and_json_oracles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filtered-count-baseline-xml/counts.mfd");
    let (imported, graph) = admission("fresh-counted-field-xml", &path);
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-baseline-xml/cases.json"
    ))
    .unwrap();
    let mut observations = Vec::new();
    for case in cases {
        let input = format_xml::from_str(case["input"].as_str().unwrap(), &imported.project.source)
            .unwrap();
        let observation = observe_output(
            case["name"].as_str().unwrap(),
            &imported.project,
            input,
            &case["expected"],
            Some(case["expectedXml"].as_str().unwrap()),
        );
        println!("{observation}");
        observations.push(observation);
    }
    save(
        "fresh-xml-admission.json",
        &serde_json::to_string_pretty(&graph).unwrap(),
    );
    save(
        "fresh-xml-outcomes.json",
        &serde_json::to_string_pretty(&observations).unwrap(),
    );
    assert!(
        observations
            .iter()
            .all(|observation| observation["matchesWholeOracle"] == true)
    );
    assert_eq!(graph["bareCounts"], 0);
}
