use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

const WARNING: &str = "source output-node function `missing-rule` for output key 11 has no supported definition; rule skipped; restore a supported definition or remove the rule";
const MISSING: &str = r#"<outputnodefunctions><rule applyto="self"><function name="missing-rule"/><filter datatype="string"/></rule></outputnodefunctions>"#;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "ferrule_mfd_source_rule_diagnostics_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create fixture directory: {error}"),
            }
        }
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(
    directory: &Path,
    root_rules: &str,
    raw_rules: &str,
    unused_rules: &str,
) -> PathBuf {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Raw" type="xs:string" minOccurs="0"/>
            <xs:element name="Unused" type="xs:string" minOccurs="0"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
            <xs:element name="Copy" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="31"><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data>
                <root><entry name="Source" outkey="10">{root_rules}
                  <entry name="Raw" outkey="11">{raw_rules}</entry>
                  <entry name="Unused" outkey="12">{unused_rules}</entry>
                </entry></root>
                <document schema="source.xsd" instanceroot="{{}}Source"/>
              </data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
                <root><entry name="Target" inpkey="20">
                  <entry name="Value" inpkey="21"/><entry name="Copy" inpkey="22"/>
                </entry></root>
                <document schema="target.xsd" instanceroot="{{}}Target"/>
              </data></component>
            </children><graph><vertices>
              <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
              <vertex vertexkey="11"><edges><edge vertexkey="21"/><edge vertexkey="22"/></edges></vertex>
            </vertices></graph></structure></component></mapping>"#
        ),
    )
    .unwrap();
    design
}

fn source(value: Value) -> Instance {
    Instance::Group(vec![("Raw".into(), Instance::Scalar(value))].into())
}

fn assert_outputs(project: &mapping::Project, input: Value, expected: &str) {
    assert!(engine::validate(project).is_empty());
    let output = engine::run(project, &source(input)).unwrap();
    for field in ["Value", "Copy"] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&Value::String(expected.to_string())),
            "{field}: {output:?}"
        );
    }
}

fn assert_same_graph(actual: &mapping::Project, expected: &mapping::Project) {
    assert_eq!(
        serde_json::to_value(&actual.graph).unwrap(),
        serde_json::to_value(&expected.graph).unwrap()
    );
}

fn clean_executable(design: &Path) -> mfd::Imported {
    let outcome = mfd::import_with_profile(
        design,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(outcome.imported.warnings.is_empty());
    assert!(outcome.report.executable);
    outcome.imported
}

#[test]
fn used_missing_rule_warns_once_and_preserves_the_repair_graph() {
    let directory = Directory::new();
    let design = write_design(&directory.0, "", MISSING, "");
    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings, vec![WARNING.to_string()]);
    let baseline = write_design(&directory.0, "", "", "");
    let baseline = clean_executable(&baseline);
    assert_same_graph(&imported.project, &baseline.project);
    assert_outputs(&imported.project, Value::String("Ada".into()), "Ada");
}

#[test]
fn executable_profile_refuses_the_used_missing_rule() {
    let directory = Directory::new();
    let design = write_design(&directory.0, "", MISSING, "");
    let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
        &design,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) else {
        panic!("used unresolved source rule must refuse executable import");
    };
    assert!(!report.executable);
    assert_eq!(
        report.issues,
        vec![mfd::ImportIssue {
            kind: mfd::ImportIssueKind::ImportWarning,
            message: WARNING.to_string(),
        }]
    );
}

#[test]
fn unconnected_missing_rule_is_inert() {
    let directory = Directory::new();
    let design = write_design(&directory.0, "", "", MISSING);
    let imported = clean_executable(&design);
    assert_outputs(&imported.project, Value::String("Ada".into()), "Ada");
}

#[test]
fn nonmatching_scalar_filter_is_inert() {
    let directory = Directory::new();
    let rules = MISSING.replace("datatype=\"string\"", "datatype=\"integer\"");
    let design = write_design(&directory.0, "", &rules, "");
    let imported = clean_executable(&design);
    let baseline = write_design(&directory.0, "", "", "");
    assert_same_graph(&imported.project, &clean_executable(&baseline).project);
}

#[test]
fn defaults_keep_their_order_around_an_unresolved_rule() {
    let directory = Directory::new();
    let rules = r#"<outputnodefunctions>
      <rule><default value="first"/><filter datatype="string"/></rule>
      <rule><function name="missing-rule"/><filter datatype="string"/></rule>
      <rule><default value="second"/><filter datatype="string"/></rule>
    </outputnodefunctions>"#;
    let design = write_design(&directory.0, "", rules, "");
    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings, vec![WARNING.to_string()]);
    let baseline_rules = r#"<outputnodefunctions>
      <rule><default value="first"/><filter datatype="string"/></rule>
      <rule><default value="second"/><filter datatype="string"/></rule>
    </outputnodefunctions>"#;
    let baseline = write_design(&directory.0, "", baseline_rules, "");
    assert_same_graph(&imported.project, &clean_executable(&baseline).project);
    assert_outputs(&imported.project, Value::Null, "first");
    assert_outputs(&imported.project, Value::String("Ada".into()), "Ada");
}

#[test]
fn inherited_missing_rule_respects_the_existing_block_boundary() {
    let directory = Directory::new();
    let inherited = MISSING.replace("applyto=\"self\"", "applyto=\"descendants\"");
    let design = write_design(&directory.0, &inherited, "", "");
    let imported = mfd::import(&design).unwrap();
    assert_eq!(imported.warnings, vec![WARNING.to_string()]);
    assert_outputs(&imported.project, Value::String("Ada".into()), "Ada");
    let blocked = write_design(
        &directory.0,
        &inherited,
        r#"<outputnodefunctions inherit="block"/>"#,
        "",
    );
    let blocked = clean_executable(&blocked);
    assert_outputs(&blocked.project, Value::String("Ada".into()), "Ada");
}

#[test]
fn malformed_or_unsupported_rules_remain_inert() {
    let directory = Directory::new();
    let baseline = write_design(&directory.0, "", "", "");
    let baseline = clean_executable(&baseline);
    for rules in [
        r#"<outputnodefunctions><rule><function/></rule></outputnodefunctions>"#,
        r#"<outputnodefunctions><rule><function name=" "/></rule></outputnodefunctions>"#,
        r#"<outputnodefunctions><rule><function name="missing-rule"/><filter datatype="date"/></rule></outputnodefunctions>"#,
        r#"<outputnodefunctions><rule applyto="somewhere"><function name="missing-rule"/><filter datatype="string"/></rule></outputnodefunctions>"#,
    ] {
        let design = write_design(&directory.0, "", rules, "");
        let imported = clean_executable(&design);
        assert_same_graph(&imported.project, &baseline.project);
    }
}

#[test]
fn non_scalar_source_rule_remains_inert() {
    let directory = Directory::new();
    let design = write_design(&directory.0, MISSING, "", "");
    let imported = clean_executable(&design);
    let baseline = write_design(&directory.0, "", "", "");
    assert_same_graph(&imported.project, &clean_executable(&baseline).project);
}
