//! Self-authored X12 fixtures. Refusal is a safety contract, not parity evidence.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::Instance;
use mapping::IterationOutput;
use serde_json::{Value, json};

struct Fixture(PathBuf);

impl Fixture {
    fn new(repeating: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_filtered_json_object_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let data = |prefix: &str, count| {
            (1..=count)
                .map(|index| format!(r#"<Data name="{prefix}{index:02}" type="string"/>"#))
                .collect::<String>()
        };
        std::fs::write(
            path.join("Envelope.Config"),
            format!(
                r#"<Config><Format standard="X12"/><Group name="Envelope">
                <Segment name="ISA">{}<Condition path="ISA12" value="00501"/></Segment>
                <Group name="Group" maxOccurs="unbounded">
                <Segment name="GS">{}<Condition path="GS08" value="005010"/></Segment>
                <Select field="ST/ST01"/>
                <Segment name="GE">{}</Segment></Group>
                <Segment name="IEA">{}</Segment></Group></Config>"#,
                data("ISA", 16),
                data("GS", 8),
                data("GE", 2),
                data("IEA", 2)
            ),
        )
        .unwrap();
        std::fs::write(
            path.join("940.Config"),
            r#"<Config><Format standard="X12"/><Message><MessageType>940</MessageType>
            <Group name="Transaction">
            <Segment name="ST"><Data name="ST01" type="string"/><Data name="ST02" type="string"/><Condition path="ST01" value="940"/></Segment>
            <Group name="Party" minOccurs="0" maxOccurs="unbounded"><Segment name="N1"><Data name="N101" type="string"/><Data name="N102" type="string"/></Segment></Group>
            <Segment name="SE"><Data name="SE01" type="string"/><Data name="SE02" type="string"/></Segment>
            </Group></Message></Config>"#,
        )
        .unwrap();
        let object = json!({"type":"object", "properties":{
            "name":{"type":"string"}, "qualifier":{"type":"string"}
        }});
        let deliver_to = if repeating {
            json!({"type":"array", "items":object})
        } else {
            object
        };
        std::fs::write(
            path.join("target.schema.json"),
            json!({"type":"object", "properties":{
                "deliverTo":deliver_to, "status":{"type":"string"}
            }})
            .to_string(),
        )
        .unwrap();
        let object_entry = r#"<entry name="object" inpkey="40"><entry name="name" type="json-property"><entry name="string" inpkey="41"/></entry><entry name="qualifier" type="json-property"><entry name="string" inpkey="42"/></entry></entry>"#;
        let selection = if repeating {
            format!(
                r#"<entry name="array"><entry name="item" type="json-item">{object_entry}</entry></entry>"#
            )
        } else {
            object_entry.to_string()
        };
        std::fs::write(
            path.join("mapping.mfd"),
            format!(
                r#"<mapping version="26"><component name="map"><structure><children>
                <component name="source" library="text" kind="16"><data><root><entry name="document"><entry name="Envelope"><entry name="Group"><entry name="Message"><entry name="Party" outkey="10"><entry name="N1"><entry name="N101" outkey="11"/><entry name="N102" outkey="12"/></entry></entry></entry></entry></entry></entry></root><text type="edi" kind="EDIX12" config="940.Config" inputinstance="source.x12"/></data></component>
                <component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="deliverTo" type="json-property">{selection}</entry><entry name="status" type="json-property"><entry name="string" inpkey="43"/></entry></entry></entry></entry></entry></root><json schema="target.schema.json" outputinstance="target.json"/></data></component>
                <component name="constant" library="core" kind="2"><targets><datapoint key="100"/></targets><data><constant value="ST" datatype="string"/></data></component>
                <component name="constant" library="core" kind="2"><targets><datapoint key="101"/></targets><data><constant value="KEPT" datatype="string"/></data></component>
                <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
                <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/><datapoint pos="1" key="33"/></targets></component>
                </children><connections>
                <edge from="10" to="30"/><edge from="11" to="20"/><edge from="100" to="21"/><edge from="22" to="31"/>
                <edge from="32" to="40"/><edge from="12" to="41"/><edge from="11" to="42"/><edge from="101" to="43"/>
                </connections></structure></component></mapping>"#
            ),
        )
        .unwrap();
        Self(path)
    }

    fn rewrite(&self, from: &str, to: &str) {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "missing replacement: {from}");
        std::fs::write(path, text.replace(from, to)).unwrap();
    }

    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("mapping.mfd")).unwrap()
    }

    fn executable(&self) -> Result<mfd::ImportOutcome, mfd::MfdError> {
        mfd::import_with_profile(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(project: &mapping::Project, segments: &str) -> Instance {
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );
    let count = segments.matches("N1*").count() + 2;
    let wire = format!(
        "ISA*00*          *00*          *ZZ*GENERIC-SENDER *ZZ*GENERIC-RECEIVE*261111*1200*^*00501*000000001*0*P*:~GS*OW*GENERIC*GENERIC*20261111*1200*1*X*005010~ST*940*0001~{segments}SE*{count}*0001~GE*1*1~IEA*1*000000001~"
    );
    let input = format_edi::x12::from_str(
        &wire,
        &project.source,
        project.source_options.lenient_segments,
    )
    .unwrap();
    engine::run(project, &input).unwrap()
}

fn json_output(project: &mapping::Project, output: &Instance) -> Value {
    serde_json::from_str(&format_json::to_string(&project.target, output).unwrap()).unwrap()
}

fn assert_refused(fixture: &Fixture) -> mfd::Imported {
    let imported = fixture.import();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains("filtered source into non-repeating JSON object `deliverTo`")
    );
    assert!(imported.warnings[0].contains("group and descendant bindings skipped"));
    assert!(!mfd::assess_import(&imported).executable);
    match fixture.executable() {
        Err(mfd::MfdError::IncompatibleImport(report)) => {
            assert!(!report.executable);
            assert_eq!(report.issues.len(), 1, "{report:?}");
            assert_eq!(report.issues[0].kind, mfd::ImportIssueKind::ImportWarning);
            assert_eq!(report.issues[0].message, imported.warnings[0]);
        }
        Err(error) => panic!("unexpected refusal: {error}"),
        Ok(_) => panic!("filtered singular JSON object admitted without a proved contract"),
    }
    imported
}

#[test]
fn singular_filtered_object_warns_and_skips_descendants_instead_of_using_bill_party() {
    for branch in [32, 33] {
        let fixture = Fixture::new(false);
        fixture.rewrite(
            r#"<edge from="32" to="40"/>"#,
            &format!(r#"<edge from="{branch}" to="40"/>"#),
        );
        let imported = assert_refused(&fixture);
        assert!(imported.project.root.children.is_empty());
        // These are complete repair-oriented outputs, not supported parity results.
        for segments in [
            "N1*BT*BILL PARTY~N1*ST*SHIP PARTY~",
            "N1*BT*BILL PARTY~",
            "N1*ST*SHIP FIRST~N1*ST*SHIP SECOND~",
            "",
        ] {
            let output = run(&imported.project, segments);
            assert_eq!(
                json_output(&imported.project, &output),
                json!({"status":"KEPT"})
            );
        }
    }
}

#[test]
fn array_control_retains_filter_branch_and_all_selected_objects_in_order() {
    for branch in [32, 33] {
        let fixture = Fixture::new(true);
        fixture.rewrite(
            r#"<edge from="32" to="40"/>"#,
            &format!(r#"<edge from="{branch}" to="40"/>"#),
        );
        let imported = fixture.executable().unwrap().imported;
        assert!(imported.warnings.is_empty());
        let scope = &imported.project.root.children[0];
        assert!(scope.iterates());
        assert!(scope.filter.is_some());
        assert_eq!(scope.iteration_output, IterationOutput::Repeated);
        for (segments, selected, rejected) in [
            (
                "N1*BT*BILL PARTY~N1*ST*SHIP PARTY~",
                json!([{"name":"SHIP PARTY","qualifier":"ST"}]),
                json!([{"name":"BILL PARTY","qualifier":"BT"}]),
            ),
            (
                "N1*BT*BILL PARTY~",
                json!([]),
                json!([{"name":"BILL PARTY","qualifier":"BT"}]),
            ),
            (
                "N1*ST*SHIP FIRST~N1*ST*SHIP SECOND~",
                json!([{"name":"SHIP FIRST","qualifier":"ST"},{"name":"SHIP SECOND","qualifier":"ST"}]),
                json!([]),
            ),
            ("", json!([]), json!([])),
        ] {
            let output = run(&imported.project, segments);
            let expected = if branch == 32 { selected } else { rejected };
            assert_eq!(
                json_output(&imported.project, &output),
                json!({"deliverTo":expected,"status":"KEPT"})
            );
        }
    }
}

#[test]
fn missing_or_unsupported_predicate_cannot_restore_unfiltered_singular_children() {
    for (from, to) in [
        (r#"<edge from="22" to="31"/>"#, ""),
        (
            r#"<edge from="22" to="31"/>"#,
            r#"<edge from="11" to="31"/>"#,
        ),
    ] {
        let fixture = Fixture::new(false);
        fixture.rewrite(from, to);
        let imported = assert_refused(&fixture);
        let output = run(&imported.project, "N1*BT*BILL PARTY~N1*ST*SHIP PARTY~");
        assert_eq!(
            json_output(&imported.project, &output),
            json!({"status":"KEPT"})
        );
    }
}

#[test]
fn existing_singular_output_modes_do_not_supply_an_interchange_selection_contract() {
    let fixture = Fixture::new(true);
    let mut project = fixture.executable().unwrap().imported.project;
    let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("expected the authored JSON object schema")
    };
    children
        .iter_mut()
        .find(|child| child.name == "deliverTo")
        .unwrap()
        .repeating = false;
    // Inspect existing IR behavior explicitly; neither mode is imported for the
    // unqualified singular design, and neither is a reference-application oracle.
    for mode in [IterationOutput::First, IterationOutput::MappedSequence] {
        project.root.children[0].iteration_output = mode;
        for (segments, names) in [
            ("N1*BT*BILL PARTY~", vec![]),
            ("N1*BT*BILL PARTY~N1*ST*SHIP PARTY~", vec!["SHIP PARTY"]),
            (
                "N1*ST*SHIP FIRST~N1*ST*SHIP SECOND~",
                vec!["SHIP FIRST", "SHIP SECOND"],
            ),
        ] {
            let output = run(&project, segments);
            let deliver_to = output.field("deliverTo").unwrap();
            if mode == IterationOutput::MappedSequence {
                let Instance::MappedSequence(items) = deliver_to else {
                    panic!("{output:?}")
                };
                assert_eq!(items.len(), names.len());
                if names.len() > 1 {
                    assert!(matches!(
                        format_json::to_string(&project.target, &output),
                        Err(format_json::JsonFormatError::Shape {
                            expected: "one mapped item",
                            got: "mapped sequence",
                            ..
                        })
                    ));
                    continue;
                }
            } else {
                assert!(matches!(deliver_to, Instance::Group(_)));
            }
            let expected = match names.first() {
                Some(name) => json!({"deliverTo":{"name":name,"qualifier":"ST"},"status":"KEPT"}),
                None if mode == IterationOutput::First => json!({"deliverTo":{},"status":"KEPT"}),
                None => json!({"status":"KEPT"}),
            };
            assert_eq!(json_output(&project, &output), expected);
        }
    }
}
