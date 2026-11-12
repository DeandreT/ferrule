//! Self-authored EDI scalar-filter lookups must use their receiving scope,
//! not a repetition that happens to be iterated by a sibling target branch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::Node;
use serde_json::{Value, json};

struct Package(PathBuf);

impl Package {
    fn new(sibling: bool, nested_lookup: bool, predicate: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-filtered-scalar-anchor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let package = Self(path);
        package.write(
            "Envelope.Config",
            r#"<Config><Format standard="X12"/><Group name="Envelope"><Group name="Group"><Select field="ST/ST01"/></Group></Group></Config>"#,
        );
        package.write(
            "856.Config",
            r#"<Config><Format standard="X12"/><Message><MessageType>856</MessageType><Group name="Transaction">
  <Segment name="ST"><Data name="ST01" type="string"/><Data name="ST02" type="string"/><Condition path="ST01" value="856"/></Segment>
  <Group name="Loop0100" minOccurs="0" maxOccurs="unbounded">
    <Segment name="HL"><Data name="HL01" type="string"/></Segment>
    <Segment name="REF" minOccurs="0" maxOccurs="unbounded"><Data name="REF01" type="string"/><Data name="REF02" type="string" minOccurs="0"/></Segment>
  </Group>
  <Segment name="SE"><Data name="SE01" type="string"/><Data name="SE02" type="string"/></Segment>
</Group></Message></Config>"#,
        );
        let mut properties = json!({"waybill": {"type": "string"}});
        if sibling {
            let mut row_properties = json!({"id": {"type": "string"}});
            if nested_lookup {
                row_properties["waybill"] = json!({"type": "string"});
            }
            properties["loops"] = json!({
                "type": "array",
                "items": {"type": "object", "properties": row_properties}
            });
        }
        package.write(
            "target.schema.json",
            &json!({"type": "object", "properties": properties}).to_string(),
        );
        let nested_entry = if nested_lookup {
            r#"<entry name="waybill" type="json-property"><entry name="string" inpkey="113"/></entry>"#
        } else {
            ""
        };
        let sibling_entry = if sibling {
            format!(
                r#"<entry name="loops" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" inpkey="110"><entry name="id" type="json-property"><entry name="string" inpkey="111"/></entry>{nested_entry}</entry></entry></entry></entry>"#
            )
        } else {
            String::new()
        };
        let sibling_edges = if sibling {
            r#"<edge from="20" to="110"/><edge from="21" to="111"/>"#
        } else {
            ""
        };
        let nested_edge = if nested_lookup {
            r#"<edge from="45" to="113"/>"#
        } else {
            ""
        };
        package.write(
            "mapping.mfd",
            &format!(
                r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="text" kind="16"><data><root><entry name="document"><entry name="Envelope"><entry name="Group"><entry name="Message">
    <entry name="ST"><entry name="ST01"/><entry name="ST02"/></entry>
    <entry name="Loop0100" outkey="20"><entry name="HL"><entry name="HL01" outkey="21"/></entry><entry name="REF"><entry name="REF01" outkey="25"/><entry name="REF02" outkey="26"/></entry></entry>
    <entry name="SE"><entry name="SE01"/><entry name="SE02"/></entry>
  </entry></entry></entry></entry></root><text type="edi" kind="EDIX12" config="856.Config" inputinstance="source.x12"/></data></component>
  <component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="root"><entry name="object">
    <entry name="waybill" type="json-property"><entry name="string" inpkey="100"/></entry>{sibling_entry}
  </entry></entry></entry></root><json schema="target.schema.json"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="30"/></targets><data><constant value="BM" datatype="string"/></data></component>
  <component name="{predicate}" library="core" kind="5"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint key="42"/></targets></component>
  <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="43"/><datapoint pos="1" key="44"/></sources><targets><datapoint pos="0" key="45"/></targets></component>
</children><connections>
  <edge from="25" to="40"/><edge from="30" to="41"/><edge from="26" to="43"/><edge from="42" to="44"/><edge from="45" to="100"/>{sibling_edges}{nested_edge}
</connections></structure></component></mapping>"#
            ),
        );
        package
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }

    fn import(&self) -> mfd::Imported {
        mfd::import_with_profile(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source(loops: Value) -> Value {
    json!({
        "Group": {"Message": [{
        "ST": {"ST01": "856", "ST02": "0001"},
        "Loop0100": loops,
        "SE": {"SE01": "2", "SE02": "0001"}
        }]}
    })
}

fn assert_output(project: &mapping::Project, source: &Value, expected: Value) {
    // Typed transport isolates importer frame ownership from envelope parsing.
    let input = format_json::from_str(&source.to_string(), &project.source).unwrap();
    let before = input.clone();
    let output = engine::run(project, &input).unwrap();
    assert_eq!(input, before, "execution changed the caller's source");
    let actual: Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap()).unwrap();
    assert_eq!(actual, expected, "complete mapped document differs");
}

fn assert_lookup_path(project: &mapping::Project, expected: &[&str]) {
    assert!(
        project.graph.nodes.values().any(|node| matches!(
            node,
            Node::Lookup { collection, key, value, .. }
                if collection.iter().map(String::as_str).eq(expected.iter().copied())
                    && key == &["REF01"] && value == &["REF02"]
        )),
        "missing lookup at {expected:?}: {:?}",
        project.graph
    );
}

#[test]
fn outer_filtered_scalar_keeps_its_path_with_or_without_a_sibling_iteration() {
    for sibling in [false, true] {
        let package = Package::new(sibling, false, "equal");
        let imported = package.import();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(engine::validate(&imported.project).is_empty());
        assert_lookup_path(&imported.project, &["Group", "Message", "Loop0100", "REF"]);
        let input = source(json!([
            {"HL": {"HL01": "1"}, "REF": [
                {"REF01": "PO", "REF02": "not-a-waybill"},
                {"REF01": "BM", "REF02": "BILL-A"},
                {"REF01": "BM", "REF02": "later-duplicate"}
            ]},
            {"HL": {"HL01": "2"}, "REF": [{"REF01": "BM", "REF02": "BILL-B"}]}
        ]));
        let expected = if sibling {
            json!({"waybill": "BILL-A", "loops": [{"id": "1"}, {"id": "2"}]})
        } else {
            json!({"waybill": "BILL-A"})
        };
        assert_output(&imported.project, &input, expected);
    }
}

#[test]
fn one_filter_feed_keeps_outer_and_current_loop_receivers_independent() {
    let package = Package::new(true, true, "equal");
    let project = package.import().project;
    assert_lookup_path(&project, &["Group", "Message", "Loop0100", "REF"]);
    assert_lookup_path(&project, &["REF"]);
    assert_output(
        &project,
        &source(json!([
            {"HL": {"HL01": "1"}, "REF": [{"REF01": "BM", "REF02": "BILL-A"}]},
            {"HL": {"HL01": "2"}, "REF": [{"REF01": "BM", "REF02": "BILL-B"}]}
        ])),
        json!({"waybill": "BILL-A", "loops": [
            {"id": "1", "waybill": "BILL-A"},
            {"id": "2", "waybill": "BILL-B"}
        ]}),
    );
    assert_output(
        &project,
        &source(json!([
            {"HL": {"HL01": "1"}, "REF": [{"REF01": "PO", "REF02": "not-a-waybill"}]},
            {"HL": {"HL01": "2"}, "REF": [{"REF01": "BM", "REF02": "BILL-B"}]}
        ])),
        json!({"waybill": "BILL-B", "loops": [
            {"id": "1"},
            {"id": "2", "waybill": "BILL-B"}
        ]}),
    );
}

#[test]
fn absent_matches_and_empty_collections_do_not_select_unfiltered_values() {
    for sibling in [false, true] {
        let package = Package::new(sibling, false, "equal");
        let project = package.import().project;
        for (loops, rows) in [
            (json!([]), json!([])),
            (
                json!([{"HL": {"HL01": "1"}, "REF": []}]),
                json!([{"id": "1"}]),
            ),
            (
                json!([{"HL": {"HL01": "1"}, "REF": [{"REF01": "PO", "REF02": "not-a-waybill"}]}]),
                json!([{"id": "1"}]),
            ),
            (
                json!([{"HL": {"HL01": "1"}, "REF": [{"REF01": "BM"}, {"REF01": "BM", "REF02": "later-value"}]}]),
                json!([{"id": "1"}]),
            ),
        ] {
            let expected = if sibling {
                json!({"loops": rows})
            } else {
                json!({})
            };
            assert_output(&project, &source(loops), expected);
        }
    }
}

#[test]
fn unsupported_filter_predicate_still_refuses_executable_import() {
    for sibling in [false, true] {
        let package = Package::new(sibling, false, "unknown-predicate");
        let path = package.0.join("mapping.mfd");
        let imported = mfd::import(&path).unwrap();
        assert!(!mfd::assess_import(&imported).executable);
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("filter") && warning.contains("affected binding skipped")
            }),
            "{:?}",
            imported.warnings
        );
        assert!(matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            ),
            Err(mfd::MfdError::IncompatibleImport(_))
        ));
    }
}

#[test]
fn lookup_key_errors_propagate_even_when_the_collection_is_empty() {
    for sibling in [false, true] {
        let package = Package::new(sibling, false, "equal");
        let path = package.0.join("mapping.mfd");
        let design = std::fs::read_to_string(&path).unwrap();
        let design = design
            .replace(
                "</children>",
                r#"<component name="constant" library="core" kind="2"><targets><datapoint key="60"/></targets><data><constant value="0" datatype="integer"/></data></component>
<component name="divide" library="core" kind="5"><sources><datapoint pos="0" key="61"/><datapoint pos="1" key="62"/></sources><targets><datapoint key="63"/></targets></component></children>"#,
            )
            .replace(
                r#"<edge from="30" to="41"/>"#,
                r#"<edge from="60" to="61"/><edge from="60" to="62"/><edge from="63" to="41"/>"#,
            );
        package.write("mapping.mfd", &design);
        let project = package.import().project;
        for loops in [
            json!([]),
            json!([{"HL": {"HL01": "1"}, "REF": [{"REF01": "BM", "REF02": "BILL-A"}]}]),
        ] {
            let input = format_json::from_str(&source(loops).to_string(), &project.source).unwrap();
            let before = input.clone();
            assert!(matches!(
                engine::run(&project, &input),
                Err(engine::EngineError::Function(
                    functions::FunctionError::DivideByZero
                ))
            ));
            assert_eq!(
                input, before,
                "failed execution changed the caller's source"
            );
        }
    }
}
