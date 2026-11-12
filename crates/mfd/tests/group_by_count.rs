use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

const WARNING: &str = "aggregate `count` cannot import its sequence: group-by output requires a distinct-group count, but scalar grouped reduction is unsupported; supply the distinct group total explicitly instead of counting raw records; imported as its empty-sequence result until the sequence is restored";

// All schemas, designs and values in this test are authored here. No external
// corpus or reference-application output is needed to distinguish 3 from 4.
struct Package(PathBuf);

impl Package {
    fn new(count_feed: u32, one_pin: bool, wrapper: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule_mfd_group_by_count_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("source.xsd"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Feed"><xs:complexType><xs:sequence><xs:element name="Rows" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )
        .unwrap();
        std::fs::write(
            directory.join("target.xsd"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Summary"><xs:complexType><xs:sequence><xs:element name="Count" type="xs:integer"/><xs:element name="Raw" type="xs:integer"/><xs:element name="Group" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Key" type="xs:string"/><xs:element name="Member" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )
        .unwrap();
        let pins = if one_pin {
            r#"<datapoint pos="0" key="201"/>"#
        } else {
            r#"<datapoint pos="0"/><datapoint pos="1" key="201"/>"#
        };
        let wrapper_component = if wrapper {
            r#"<component name="string" library="core" kind="5"><sources><datapoint pos="0" key="301"/></sources><targets><datapoint pos="0" key="302"/></targets></component>"#
        } else {
            ""
        };
        let count_edge = if wrapper {
            format!(r#"<edge from="{count_feed}" to="301"/><edge from="302" to="201"/>"#)
        } else {
            format!(r#"<edge from="{count_feed}" to="201"/>"#)
        };
        std::fs::write(
            directory.join("mapping.mfd"),
            format!(
                r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Feed"><entry name="Rows" outkey="10"><entry name="Key" outkey="11"/><entry name="Name" outkey="12"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Feed"/></data></component>
<component name="report" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Summary"><entry name="Count" inpkey="50"/><entry name="Raw" inpkey="51"/><entry name="Group" inpkey="30"><entry name="Key" inpkey="31"/><entry name="Member" inpkey="40"><entry name="Name" inpkey="41"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="report.xml" instanceroot="{{}}Summary"/></data></component>
<component name="group-by" library="core" kind="5"><sources><datapoint pos="0" key="111"/><datapoint pos="1" key="112"/></sources><targets><datapoint pos="0" key="121"/><datapoint pos="1" key="122"/></targets></component>
<component name="count" library="core" kind="5"><sources>{pins}</sources><targets><datapoint pos="0" key="202"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint pos="0"/><datapoint pos="1" key="211"/></sources><targets><datapoint pos="0" key="212"/></targets></component>
{wrapper_component}
</children><connections><edge from="10" to="111"/><edge from="11" to="112"/><edge from="121" to="30"/><edge from="122" to="31"/><edge from="10" to="40"/><edge from="12" to="41"/>{count_edge}<edge from="202" to="50"/><edge from="10" to="211"/><edge from="212" to="51"/></connections></structure></component></mapping>"#
            ),
        )
        .unwrap();
        Self(directory)
    }

    fn mapping(&self) -> PathBuf {
        self.0.join("mapping.mfd")
    }

    fn rewrite(&self, from: &str, to: &str) {
        let text = std::fs::read_to_string(self.mapping()).unwrap();
        assert_eq!(text.matches(from).count(), 1, "fixture fragment: {from}");
        std::fs::write(self.mapping(), text.replace(from, to)).unwrap();
    }

    fn forwarded(feed: u32) -> Self {
        let package = Self::new(302, false, false);
        let (entry, root) = if feed == 122 {
            (
                r#"<entry name="Key" inpkey="301" outkey="302"/>"#,
                "{}Feed/{}Rows/{}Key",
            )
        } else {
            (
                r#"<entry name="Rows" inpkey="301" outkey="302"><entry name="Key"/><entry name="Name"/></entry>"#,
                "{}Feed/{}Rows",
            )
        };
        package.rewrite(
            "</children>",
            &format!(r#"<component name="forward" library="xml" kind="14"><data><parameter usageKind="variable"/><root>{entry}</root><document schema="source.xsd" instanceroot="{root}"/></data></component></children>"#),
        );
        package.rewrite(
            "</connections>",
            &format!(r#"<edge from="{feed}" to="301"/></connections>"#),
        );
        package
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("retained grouped count package: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_complete_output(project: &mapping::Project, count: i64) {
    let input = format_json::from_str(
        r#"{"Rows":[{"Key":"A","Name":"first"},{"Key":"B","Name":"second"},{"Key":"A","Name":"third"},{"Key":"C","Name":"fourth"}]}"#,
        &project.source,
    )
    .unwrap();
    let output = engine::run(project, &input).unwrap();
    let expected = format_json::from_str(
        &format!(
            r#"{{"Count":{count},"Raw":4,"Group":[{{"Key":"A","Member":[{{"Name":"first"}},{{"Name":"third"}}]}},{{"Key":"B","Member":[{{"Name":"second"}}]}},{{"Key":"C","Member":[{{"Name":"fourth"}}]}}]}}"#
        ),
        &project.target,
    )
    .unwrap();
    assert_eq!(output, expected);
    assert_eq!(
        output.field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(count))
    );
    assert_eq!(
        output.field("Raw").and_then(Instance::as_scalar),
        Some(&Value::Int(4))
    );
    assert_eq!(
        output.field("Group").unwrap().as_repeated().unwrap().len(),
        3
    );
}

fn assert_refusal(package: &Package) {
    let imported = mfd::import(&package.mapping()).unwrap();
    assert_eq!(imported.warnings, [WARNING]);
    assert!(engine::validate(&imported.project).is_empty());
    // Best-effort keeps its documented repair placeholder, never the wrong 4.
    assert_complete_output(&imported.project, 0);
    let expected_report = mfd::ImportReport {
        executable: false,
        issues: vec![mfd::ImportIssue {
            kind: mfd::ImportIssueKind::ImportWarning,
            message: WARNING.into(),
        }],
    };
    assert_eq!(mfd::assess_import(&imported), expected_report);
    match mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) {
        Err(mfd::MfdError::IncompatibleImport(report)) => assert_eq!(*report, expected_report),
        Err(error) => panic!("wrong refusal: {error}"),
        Ok(_) => panic!("grouped count must not be admitted as executable"),
    }
}

#[test]
fn group_and_key_counts_refuse_instead_of_counting_raw_records() {
    for feed in [121, 122] {
        for one_pin in [false, true] {
            assert_refusal(&Package::new(feed, one_pin, false));
        }
    }
}

#[test]
fn projected_group_keys_cannot_hide_grouped_cardinality() {
    assert_refusal(&Package::new(122, false, true));
}

#[test]
fn ordinary_record_counts_remain_executable_and_count_all_four_records() {
    for one_pin in [false, true] {
        assert_raw_count(&Package::new(10, one_pin, false));
    }
}

fn assert_raw_count(package: &Package) {
    let outcome = mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(outcome.imported.warnings.is_empty());
    assert_eq!(
        outcome.report,
        mfd::ImportReport {
            executable: true,
            issues: Vec::new()
        }
    );
    assert_complete_output(&outcome.imported.project, 4);
}

#[test]
fn transparent_variable_forwards_cannot_hide_either_group_by_output() {
    for feed in [121, 122] {
        assert_refusal(&Package::forwarded(feed));
    }
}

#[test]
fn transparent_variable_forward_keeps_ordinary_record_count_executable() {
    assert_raw_count(&Package::forwarded(10));
}

#[test]
fn completed_scalar_reduction_does_not_forward_its_input_group_sequence() {
    let package = Package::new(332, false, false);
    package.rewrite(
        "</children>",
        r#"<component name="max" library="core" kind="5"><sources><datapoint pos="0"/><datapoint pos="1" key="311"/></sources><targets><datapoint pos="0" key="312"/></targets></component>
<component name="generate-sequence" library="core" kind="5"><sources><datapoint pos="0"/><datapoint pos="1" key="321"/></sources><targets><datapoint pos="0" key="332"/></targets></component></children>"#,
    );
    package.rewrite(
        "</connections>",
        r#"<edge from="122" to="311"/><edge from="312" to="321"/></connections>"#,
    );
    // MAX is insensitive to duplicate numeric keys. Reducing keys 1,3,1,5
    // before generating 1..=5 is faithfully representable with existing nodes:
    // the outer Count must return 5, not the 3 groups or the 4 input records.
    // This does not assume that arbitrary grouped reductions are supported.
    let outcome = mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(outcome.imported.warnings.is_empty());
    assert_eq!(
        outcome.report,
        mfd::ImportReport {
            executable: true,
            issues: Vec::new()
        }
    );
    let project = outcome.imported.project;
    assert!(project.graph.nodes.values().any(|node| matches!(
        node,
        mapping::Node::Aggregate {
            function: mapping::AggregateOp::Max,
            ..
        }
    )));
    assert!(project.graph.nodes.values().any(|node| matches!(
        node,
        mapping::Node::SequenceAggregate {
            function: mapping::AggregateOp::Count,
            sequence: mapping::SequenceExpr::Generate { .. },
            ..
        }
    )));
    let input = format_json::from_str(
        r#"{"Rows":[{"Key":"1","Name":"first"},{"Key":"3","Name":"second"},{"Key":"1","Name":"third"},{"Key":"5","Name":"fourth"}]}"#,
        &project.source,
    ).unwrap();
    let expected = format_json::from_str(
        r#"{"Count":5,"Raw":4,"Group":[{"Key":"1","Member":[{"Name":"first"},{"Name":"third"}]},{"Key":"3","Member":[{"Name":"second"}]},{"Key":"5","Member":[{"Name":"fourth"}]}]}"#,
        &project.target,
    ).unwrap();
    assert_eq!(engine::run(&project, &input).unwrap(), expected);
}
