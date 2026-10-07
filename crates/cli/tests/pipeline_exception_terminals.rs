use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use engine::{EngineError, PipelineError};
use ir::{Instance, Value};
use mapping::{FailureIteration, FailureSelection, Pipeline, PipelineInput};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_pipeline_exception_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn child(&self, label: &str) -> PathBuf {
        let path = self.0.join(label);
        std::fs::create_dir(&path).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!(
                "retained pipeline-exception originals: {}",
                self.0.display()
            );
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn retain(directory: &Path, label: &str, value: &impl Debug) {
    std::fs::write(
        directory.join(format!("{label}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn design() -> String {
    r#"<mapping version="31"><component name="mapping"><structure><children>
<component name="Input" library="xml" kind="14"><data><root><entry name="Input"><entry name="Item" outkey="10"><entry name="Value" outkey="11"/><entry name="Allowed" outkey="12"/><entry name="Message" outkey="13"/></entry></entry></root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
<component name="Bridge" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Bridge"><entry name="Item" inpkey="20" outkey="30"><entry name="Value" inpkey="21" outkey="31"/><entry name="Allowed" inpkey="22" outkey="32"/><entry name="Message" inpkey="23" outkey="33"/></entry></entry></root><document schema="bridge.xsd" instanceroot="{}Bridge"/></data></component>
<component name="Output" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Row" inpkey="40"><entry name="Result" inpkey="41"/></entry></entry></root><document schema="output.xsd" outputinstance="output.xml" instanceroot="{}Output"/></data></component>
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="50"/><datapoint pos="1" key="51"/></sources><targets><datapoint pos="0" key="52"/><datapoint pos="1" key="53"/></targets></component>
<component name="reject-row" library="core" kind="18"><sources><datapoint pos="0" key="60"/><datapoint pos="1" key="61"/></sources><data><exception/></data></component>
</children></structure><connections>
<edge from="10" to="20"/><edge from="11" to="21"/><edge from="12" to="22"/><edge from="13" to="23"/>
<edge from="30" to="50"/><edge from="32" to="51"/><edge from="52" to="40"/><edge from="31" to="41"/>
<edge from="53" to="60"/><edge from="33" to="61"/>
</connections></component></mapping>"#.into()
}

fn fixture(directory: &Path, text: &str) -> PathBuf {
    for (file, root) in [("input.xsd", "Input"), ("bridge.xsd", "Bridge")] {
        std::fs::write(directory.join(file), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="Item" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:long"/><xs:element name="Allowed" type="xs:boolean"/><xs:element name="Message" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
    }
    std::fs::write(directory.join("output.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:long"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    let path = directory.join("design.mfd");
    std::fs::write(&path, text).unwrap();
    path
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn input(first: bool, second: bool) -> Instance {
    group(vec![(
        "Item",
        Instance::Repeated(
            [
                (10, first, "earlier-unselected"),
                (20, second, "late-selected"),
            ]
            .into_iter()
            .map(|(value, allowed, message)| {
                group(vec![
                    ("Value", Instance::Scalar(Value::Int(value))),
                    ("Allowed", Instance::Scalar(Value::Bool(allowed))),
                    ("Message", Instance::Scalar(Value::String(message.into()))),
                ])
            })
            .collect(),
        ),
    )])
}

fn expected() -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            [10, 20]
                .into_iter()
                .map(|value| group(vec![("Result", Instance::Scalar(Value::Int(value)))]))
                .collect(),
        ),
    )])
}

fn import(directory: &Path, path: &Path) -> Pipeline {
    let result = mfd::import_pipeline_with_options(
        path,
        &mfd::ImportOptions::default().with_package_root(directory),
    );
    match &result {
        Ok(outcome) => {
            retain(directory, "import-warnings", &outcome.warnings);
            retain(directory, "import-pipeline", &outcome.pipeline);
        }
        Err(error) => retain(directory, "import-error", error),
    }
    let outcome = result.unwrap();
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    assert_eq!(outcome.pipeline.stages.len(), 2);
    let validation = engine::validate_pipeline(&outcome.pipeline);
    retain(directory, "pipeline-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    outcome.pipeline
}

fn host_name(pipeline: &Pipeline) -> String {
    let PipelineInput::Host { name } = &pipeline.stages[0].source else {
        panic!("first stage needs a host");
    };
    let hosts = pipeline
        .stages
        .iter()
        .flat_map(|stage| {
            std::iter::once(&stage.source)
                .chain(stage.extra_sources.iter().map(|input| &input.from))
        })
        .filter_map(|input| match input {
            PipelineInput::Host { name } => Some(name.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(hosts, BTreeSet::from([name.clone()]));
    name.clone()
}

fn execute(
    directory: &Path,
    label: &str,
    pipeline: &Pipeline,
    source: &Instance,
) -> Result<engine::PipelineOutputs, PipelineError> {
    std::fs::write(
        directory.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(source).unwrap(),
    )
    .unwrap();
    let result = engine::run_pipeline(
        pipeline,
        &BTreeMap::from([(host_name(pipeline), source.clone())]),
    );
    retain(directory, label, &result);
    result
}

fn assert_late_rule(error: &PipelineError) {
    let PipelineError::StageExecution {
        stage,
        source: EngineError::MappingFailure { rule, message },
    } = error
    else {
        panic!("expected typed late global rule, got {error:?}");
    };
    assert_eq!(stage, "mfd-stage-2");
    assert_eq!(*rule, 1);
    assert_eq!(message.as_deref(), Some("late-selected"));
}

fn divide(text: &str, message: bool) -> String {
    let functions = r#"<component name="divide" library="core" kind="1"><sources><datapoint pos="0" key="71"/><datapoint pos="1" key="72"/></sources><targets><datapoint pos="0" key="73"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="74"/></targets><data><constant value="0" datatype="integer"/></data></component>"#;
    let (old, new) = if message {
        (
            r#"<edge from="33" to="61"/>"#,
            r#"<edge from="31" to="71"/><edge from="74" to="72"/><edge from="73" to="61"/>"#,
        )
    } else {
        (
            r#"<edge from="11" to="21"/>"#,
            r#"<edge from="11" to="71"/><edge from="74" to="72"/><edge from="73" to="21"/>"#,
        )
    };
    assert_eq!(text.matches(old).count(), 1);
    text.replace("</children>", &format!("{functions}</children>"))
        .replace(old, new)
}

fn assert_divide(error: &PipelineError, expected_stage: &str) {
    let PipelineError::StageExecution { stage, source } = error else {
        panic!("expected stage error: {error:?}");
    };
    assert_eq!(stage, expected_stage);
    assert!(matches!(source, EngineError::Function { .. }), "{source:?}");
    assert!(format!("{source:?}").contains("DivideByZero"), "{source:?}");
}

#[test]
fn terminal_rule_belongs_only_to_second_stage_and_retains_typed_cause_and_complete_order() {
    let root = Directory::new("owner");
    for (label, when_true) in [("false-branch", false), ("true-branch", true)] {
        let directory = root.child(label);
        let text = if when_true {
            design()
                .replace(
                    r#"<edge from="52" to="40"/>"#,
                    r#"<edge from="53" to="40"/>"#,
                )
                .replace(
                    r#"<edge from="53" to="60"/>"#,
                    r#"<edge from="52" to="60"/>"#,
                )
        } else {
            design()
        };
        let path = fixture(&directory, &text);
        let original = std::fs::read(&path).unwrap();
        let pipeline = import(&directory, &path);
        assert!(pipeline.stages[0].project.failure_rules.is_empty());
        let rules = &pipeline.stages[1].project.failure_rules;
        assert_eq!(rules.len(), 1);
        assert!(
            matches!(&rules[0].iteration, FailureIteration::Source { collection } if collection == &["Item"])
        );
        if when_true {
            assert!(matches!(
                &rules[0].selection,
                FailureSelection::WhenTrue { .. }
            ));
        } else {
            assert!(matches!(
                &rules[0].selection,
                FailureSelection::WhenFalse { .. }
            ));
        }
        let accepted = input(!when_true, !when_true);
        let result = execute(&directory, "success", &pipeline, &accepted).unwrap();
        assert_eq!(
            result
                .stages
                .iter()
                .map(|stage| stage.id.as_str())
                .collect::<Vec<_>>(),
            ["mfd-stage-1", "mfd-stage-2"]
        );
        assert_eq!(result.stage("mfd-stage-1").unwrap().primary, accepted);
        assert_eq!(result.stage("mfd-stage-2").unwrap().primary, expected());
        let failing = input(!when_true, when_true);
        let failure = execute(&directory, "late-failure", &pipeline, &failing).unwrap_err();
        assert_late_rule(&failure);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let serialized = mapping::pipeline_file::encode_pretty(&pipeline).unwrap();
        std::fs::write(directory.join("pipeline.json"), &serialized).unwrap();
        let reopened = mapping::pipeline_file::decode_str(&serialized).unwrap();
        assert_eq!(
            serde_json::to_value(&reopened).unwrap(),
            serde_json::to_value(&pipeline).unwrap()
        );
        assert_late_rule(
            &execute(&directory, "reopened-failure", &reopened, &failing).unwrap_err(),
        );
    }
}

#[test]
fn first_stage_error_and_lazy_selected_message_keep_original_pipeline_precedence() {
    let dir = Directory::new("precedence");
    let first = dir.child("first-error");
    let first_path = fixture(&first, &divide(&design(), false));
    let first_pipeline = import(&first, &first_path);
    let result = execute(
        &first,
        "first-error-before-late-rule",
        &first_pipeline,
        &input(true, false),
    );
    assert_divide(&result.unwrap_err(), "mfd-stage-1");
    let message = dir.child("message-error");
    let message_path = fixture(&message, &divide(&design(), true));
    let message_pipeline = import(&message, &message_path);
    let unselected = execute(
        &message,
        "unselected-message",
        &message_pipeline,
        &input(true, true),
    )
    .unwrap();
    assert_eq!(unselected.stage("mfd-stage-2").unwrap().primary, expected());
    assert_divide(
        &execute(
            &message,
            "selected-message",
            &message_pipeline,
            &input(true, false),
        )
        .unwrap_err(),
        "mfd-stage-2",
    );
    let absent = dir.child("absent-message");
    let absent_text = design()
        .replace(r#"<datapoint pos="1" key="61"/>"#, "")
        .replace(r#"<edge from="33" to="61"/>"#, "");
    let absent_path = fixture(&absent, &absent_text);
    let absent_pipeline = import(&absent, &absent_path);
    let error = execute(&absent, "no-message", &absent_pipeline, &input(true, false)).unwrap_err();
    assert!(
        matches!(error, PipelineError::StageExecution { stage, source: EngineError::MappingFailure { rule: 1, message: None } } if stage == "mfd-stage-2")
    );
}

fn files(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn cli_does_not_publish_earlier_selected_stage_or_replace_sentinels_after_later_rule_failure() {
    let dir = Directory::new("publication");
    let path = fixture(&dir.0, &design());
    let pipeline = import(&dir.0, &path);
    let pipeline_path = dir.0.join("pipeline.json");
    std::fs::write(
        &pipeline_path,
        mapping::pipeline_file::encode_pretty(&pipeline).unwrap(),
    )
    .unwrap();
    let input_path = dir.0.join("input.xml");
    let literal_failure = "<Input><Item><Value>10</Value><Allowed>true</Allowed><Message>earlier-unselected</Message></Item><Item><Value>20</Value><Allowed>false</Allowed><Message>late-selected</Message></Item></Input>";
    std::fs::write(&input_path, literal_failure).unwrap();
    let output_dir = dir.child("selected");
    let copy = output_dir.join("copy.xml");
    let final_path = output_dir.join("final.xml");
    let outputs = [
        cli::PipelineOutputFile {
            stage: "mfd-stage-1".into(),
            target: None,
            path: copy.clone(),
        },
        cli::PipelineOutputFile {
            stage: "mfd-stage-2".into(),
            target: None,
            path: final_path.clone(),
        },
    ];
    let inputs = [cli::PipelineHostFile {
        name: host_name(&pipeline),
        path: input_path.clone(),
    }];
    for (label, sentinel) in [("fresh", false), ("existing", true)] {
        if sentinel {
            std::fs::write(&copy, b"copy sentinel").unwrap();
            std::fs::write(&final_path, b"final sentinel").unwrap();
        }
        let before = files(&output_dir);
        let result = cli::run_pipeline_file(&pipeline_path, &inputs, &outputs);
        retain(&dir.0, &format!("{label}-cli-result"), &result);
        let error = result.unwrap_err();
        let typed = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<PipelineError>())
            .expect("original pipeline cause must remain in anyhow chain");
        assert_late_rule(typed);
        assert_eq!(files(&output_dir), before);
    }
    std::fs::write(
        &input_path,
        literal_failure.replace("<Allowed>false</Allowed>", "<Allowed>true</Allowed>"),
    )
    .unwrap();
    let success = cli::run_pipeline_file(&pipeline_path, &inputs, &outputs);
    retain(&dir.0, "cli-success", &success);
    let success = success.unwrap();
    assert_eq!(success.stages_executed, ["mfd-stage-1", "mfd-stage-2"]);
    assert_eq!(success.artifacts.len(), 2);
    let copy_xml = std::fs::read_to_string(&copy).unwrap();
    let final_xml = std::fs::read_to_string(&final_path).unwrap();
    retain(&dir.0, "copy-xml", &copy_xml);
    retain(&dir.0, "final-xml", &final_xml);
    assert_eq!(
        format_xml::from_str(&copy_xml, &pipeline.stages[0].project.target).unwrap(),
        input(true, true)
    );
    assert_eq!(
        format_xml::from_str(&final_xml, &pipeline.stages[1].project.target).unwrap(),
        expected()
    );
    assert_eq!(
        std::fs::read_to_string(&input_path).unwrap(),
        literal_failure.replace("<Allowed>false</Allowed>", "<Allowed>true</Allowed>")
    );
}

#[test]
fn malformed_exception_terminals_and_ordinary_dead_branches_never_import_a_partial_pipeline() {
    let dir = Directory::new("refusals");
    let base = design();
    let marker = r#"<data><exception/></data>"#;
    let extra_target = r#"<component name="Other" library="xml" kind="14"><data><root><entry name="Output"><entry name="Row" inpkey="90"><entry name="Result" inpkey="91"/></entry></entry></root><document schema="output.xsd" outputinstance="other.xml" instanceroot="{}Output"/></data></component>"#;
    let cases = [
        ("outputs", base.replace(marker, &format!(r#"<targets><datapoint pos="0" key="62"/></targets>{marker}"#))),
        ("marker", base.replace(marker, "<data/>")),
        ("missing-throw", base.replace(r#"<datapoint pos="0" key="60"/>"#, "")),
        ("wrong-message-position", base.replace(r#"<datapoint pos="1" key="61"/>"#, r#"<datapoint pos="2" key="61"/>"#)),
        ("alias", base.replace(r#"<datapoint pos="1" key="61"/>"#, r#"<datapoint pos="1" key="41"/>"#)),
        ("duplicate-feed", base.replace("</connections>", r#"<edge from="53" to="60"/></connections>"#)),
        ("throw-fanout", base.replace("</connections>", r#"<edge from="53" to="41"/></connections>"#)),
        ("ambiguous-owner", base.replace("</children>", &format!("{extra_target}</children>")).replace("</connections>", r#"<edge from="52" to="90"/><edge from="31" to="91"/></connections>"#)),
        ("exception-only-source", base.replace("</children>", r#"<component name="OnlyMessage" library="xml" kind="14"><data><root><entry name="Input"><entry name="Item"><entry name="Message" outkey="80"/></entry></entry></root><document schema="input.xsd" inputinstance="message.xml" instanceroot="{}Input"/></data></component></children>"#).replace(r#"<edge from="33" to="61"/>"#, r#"<edge from="80" to="61"/>"#)),
        ("ordinary-dead-function", base.replace("</children>", r#"<component name="unknown-dead" library="core" kind="1"><sources><datapoint pos="0" key="80"/></sources></component></children>"#).replace("</connections>", r#"<edge from="33" to="80"/></connections>"#)),
        ("function-cycle", base.replace(r#"<edge from="52" to="40"/>"#, r#"<edge from="52" to="82"/><edge from="73" to="81"/><edge from="83" to="71"/><edge from="83" to="40"/>"#).replace("</children>", r#"<component name="logical-not" library="core" kind="1"><sources><datapoint pos="0" key="71"/></sources><targets><datapoint pos="0" key="73"/></targets></component><component name="concat" library="core" kind="1"><sources><datapoint pos="0" key="81"/><datapoint pos="1" key="82"/></sources><targets><datapoint pos="0" key="83"/></targets></component></children>"#)),
    ];
    for (label, text) in cases {
        assert_ne!(text, base);
        let case = dir.child(label);
        let path = fixture(&case, &text);
        let original = std::fs::read(&path).unwrap();
        let result = mfd::import_pipeline_with_options(
            &path,
            &mfd::ImportOptions::default().with_package_root(&case),
        );
        match &result {
            Ok(outcome) => retain(&case, "unexpected-pipeline", &outcome.pipeline),
            Err(error) => retain(&case, "refusal", error),
        }
        let error = result
            .err()
            .expect("malformed branch must not produce a partial pipeline");
        assert!(
            matches!(error, mfd::MfdError::UnsupportedImport(_)),
            "{label}: {error:?}"
        );
        if label == "ordinary-dead-function" {
            assert!(error.to_string().contains("without a downstream target"));
        }
        if label == "function-cycle" {
            assert!(error.to_string().contains("function-feed cycle"));
        }
        if label == "exception-only-source" {
            assert!(
                error
                    .to_string()
                    .contains("connected XML output with no downstream target")
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!case.join("output.xml").exists());
    }
    let no_target = dir.child("exception-without-xml-target");
    let target = base.find("<component name=\"Output\"").unwrap();
    let end = target + base[target..].find("</component>").unwrap() + "</component>".len();
    let mut text = base.clone();
    text.replace_range(target..end, "");
    let path = fixture(&no_target, &text);
    let result = mfd::import_pipeline(&path);
    match &result {
        Ok(outcome) => retain(&no_target, "unexpected-pipeline", &outcome.pipeline),
        Err(error) => retain(&no_target, "no-target-refusal", error),
    }
    assert!(matches!(result, Err(mfd::MfdError::UnsupportedImport(_))));
    assert!(!no_target.join("output.xml").exists());
}
