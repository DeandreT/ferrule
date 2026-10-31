//! Self-authored contracts for sequence ordering, count ownership and parser endpoints.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct Package(PathBuf);

impl Package {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-corpus-gap-contracts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }

    fn mapping(&self) -> PathBuf {
        self.0.join("mapping.mfd")
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn assert_refusal(path: &Path, warning: &str) -> mfd::Imported {
    let imported = mfd::import(path).unwrap();
    assert_eq!(imported.warnings, vec![warning.to_string()]);
    let report = mfd::assess_import(&imported);
    assert!(!report.executable);
    assert!(report.issues.iter().any(|issue| {
        issue.kind == mfd::ImportIssueKind::ImportWarning && issue.message == warning
    }));
    match mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) {
        Err(mfd::MfdError::IncompatibleImport(actual)) => assert_eq!(*actual, report),
        Err(other) => panic!("expected the specific incompatibility report: {other}"),
        Ok(_) => panic!("unsupported mapping was admitted for execution"),
    }
    imported
}

enum SequenceOrder {
    FilterDistinct,
    DistinctFilter,
    DistinctSort,
}

fn sequence_package(order: SequenceOrder) -> Package {
    let package = Package::new();
    package.write(
        "source.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Item" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Key" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    package.write(
        "target.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Key" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    let distinct = r#"<component name="distinct-values" library="core" kind="5"><sources><datapoint pos="0" key="30"/></sources><targets><datapoint pos="0" key="31"/></targets></component>"#;
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint pos="0" key="42"/><datapoint/></targets></component>
<component name="not-equal" library="core" kind="5"><sources><datapoint pos="0" key="50"/><datapoint pos="1" key="51"/></sources><targets><datapoint key="52"/></targets></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="60"/></targets><data><constant value="omit" datatype="string"/></data></component>"#;
    let sort = r#"<component name="sort" library="core" kind="30"><sources><datapoint pos="0" key="70"/><datapoint pos="1" key="71"/></sources><targets><datapoint pos="0" key="72"/></targets><data><sort><collation/><key direction="ascending"/></sort></data></component>"#;
    let (control, edges) = match order {
        SequenceOrder::FilterDistinct => (
            filter,
            r#"<edge from="11" to="40"/><edge from="11" to="50"/><edge from="60" to="51"/><edge from="52" to="41"/><edge from="42" to="30"/><edge from="31" to="20"/>"#,
        ),
        SequenceOrder::DistinctFilter => (
            filter,
            r#"<edge from="11" to="30"/><edge from="31" to="40"/><edge from="11" to="50"/><edge from="60" to="51"/><edge from="52" to="41"/><edge from="42" to="20"/>"#,
        ),
        SequenceOrder::DistinctSort => (
            sort,
            r#"<edge from="11" to="30"/><edge from="31" to="70"/><edge from="11" to="71"/><edge from="72" to="20"/>"#,
        ),
    };
    package.write(
        "mapping.mfd",
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="document"><entry name="Input"><entry name="Item" outkey="10"><entry name="Key" outkey="11"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Input"/></data></component>
<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="Output"><entry name="Row" inpkey="20"><entry name="Key" inpkey="21"/></entry></entry></entry></root><document schema="target.xsd" instanceroot="{{}}Output"/></data></component>
{distinct}{control}
</children><connections>{edges}<edge from="31" to="21"/></connections></structure></component></mapping>"#
        ),
    );
    package
}

#[test]
fn filter_before_distinct_preserves_stable_string_identity() {
    let package = sequence_package(SequenceOrder::FilterDistinct);
    let imported = mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let input = format_xml::from_str(
        "<Input><Item><Key>001</Key></Item><Item><Key>omit</Key></Item><Item><Key>1</Key></Item><Item><Key>001</Key></Item><Item><Key>a</Key></Item><Item><Key>A</Key></Item><Item><Key>omit</Key></Item></Input>",
        &imported.project.source,
    )
    .unwrap();
    let expected = format_xml::from_str(
        "<Output><Row><Key>001</Key></Row><Row><Key>1</Key></Row><Row><Key>a</Key></Row><Row><Key>A</Key></Row></Output>",
        &imported.project.target,
    )
    .unwrap();
    assert_eq!(engine::run(&imported.project, &input).unwrap(), expected);
}

#[test]
fn distinct_before_filter_refuses_executable_import() {
    let package = sequence_package(SequenceOrder::DistinctFilter);
    assert_refusal(
        &package.mapping(),
        "sequence into `Row` applies distinct-values before filter, which cannot be represented exactly; imported using ferrule's sequence order",
    );
}

#[test]
fn distinct_before_sort_refuses_executable_import() {
    let package = sequence_package(SequenceOrder::DistinctSort);
    assert_refusal(
        &package.mapping(),
        "sequence into `Row` applies distinct-values before sort, which cannot be represented exactly; imported using ferrule's sequence order",
    );
}

enum CountContext {
    Enclosing,
    Sibling,
    OtherSource,
}

fn count_package(context: CountContext) -> Package {
    let package = Package::new();
    package.write(
        "message.Config",
        r#"<Config><Format standard="HL7"/><Message><MessageType>DOC</MessageType><Group name="DOC">
<Segment name="HDR"><Data name="Label" type="string"/></Segment>
<Group name="Item" minOccurs="0" maxOccurs="unbounded"><Segment name="ABC"><Data name="Code" type="string"/></Segment></Group>
</Group></Message></Config>"#,
    );
    package.write(
        "target.schema.json",
        r#"{"type":"object","properties":{"Count":{"type":"integer"}},"required":["Count"]}"#,
    );
    let source = |name: &str, root: u32, item: u32, sibling: u32| {
        format!(
            r#"<component name="{name}" library="text" kind="16"><data><root><entry name="document"><entry name="DOC" outkey="{root}"><entry name="HDR" outkey="{sibling}"><entry name="Label"/></entry><entry name="Item" outkey="{item}"><entry name="ABC"><entry name="Code"/></entry></entry></entry></entry></root><text type="edi" kind="EDIHL7" config="message.Config" inputinstance="{name}.hl7"/></data></component>"#
        )
    };
    let primary = source("source", 10, 11, 12);
    let (secondary, context_port) = match context {
        CountContext::Enclosing => (String::new(), 10),
        CountContext::Sibling => (String::new(), 12),
        CountContext::OtherSource => (source("other", 110, 111, 112), 110),
    };
    package.write(
        "mapping.mfd",
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>{primary}{secondary}
<component name="count" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint key="22"/></targets></component>
<component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="root"><entry name="object"><entry name="Count" type="json-property"><entry name="number" inpkey="30"/></entry></entry></entry></entry></root><json schema="target.schema.json"/></data></component>
</children><connections><edge from="{context_port}" to="20"/><edge from="11" to="21"/><edge from="22" to="30"/></connections></structure></component></mapping>"#
        ),
    );
    package
}

#[test]
fn enclosing_context_counts_the_selected_group() {
    let package = count_package(CountContext::Enclosing);
    let imported = mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    // Typed instances isolate count ownership from wire-format parsing.
    let input = format_json::from_str(
        r#"{"HDR":{"Label":"sample"},"Item":[{"ABC":{"Code":"001"}},{"ABC":{"Code":"1"}}]}"#,
        &imported.project.source,
    )
    .unwrap();
    let expected = Instance::Group(vec![("Count".into(), Instance::Scalar(Value::Int(2)))].into());
    assert_eq!(engine::run(&imported.project, &input).unwrap(), expected);
}

const COUNT_CONTEXT_WARNING: &str = "aggregate `count` cannot import its sequence: structured EDI count context must enclose its group feed; imported as its empty-sequence result until the sequence is restored";

#[test]
fn sibling_context_cannot_scope_an_edi_count() {
    let package = count_package(CountContext::Sibling);
    assert_refusal(&package.mapping(), COUNT_CONTEXT_WARNING);
}

#[test]
fn another_source_cannot_supply_an_edi_count_context() {
    let package = count_package(CountContext::OtherSource);
    let imported = assert_refusal(&package.mapping(), COUNT_CONTEXT_WARNING);
    assert_eq!(imported.project.extra_sources.len(), 1);
    assert_eq!(
        imported.project.source,
        imported.project.extra_sources[0].schema
    );
}

fn parser_package(connected_filename: bool) -> Package {
    let package = Package::new();
    package.write(
        "source.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Payload" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    let schema = r#"{"type":"object","properties":{"Code":{"type":"string"},"Label":{"type":"string"}},"required":["Code","Label"]}"#;
    package.write("payload.schema.json", schema);
    package.write("target.schema.json", schema);
    let (parameter, edge) = if connected_filename {
        (
            r#"<component name="path" library="core" kind="6"><targets><datapoint key="80"/></targets><data><input datatype="string" required="1"/><parameter usageKind="input" name="OutputPath"/></data></component>"#,
            r#"<edge from="80" to="90"/>"#,
        )
    } else {
        ("", "")
    };
    package.write(
        "mapping.mfd",
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="document"><entry name="Input"><entry name="Payload" outkey="10"/></entry></entry></root><document schema="source.xsd" instanceroot="{{}}Input"/></data></component>
<component name="parsed" library="json" kind="31"><data><root><entry name="FileInstance" inpkey="20"><entry name="document"><entry name="root"><entry name="object"><entry name="Code" type="json-property"><entry name="string" outkey="21"/></entry><entry name="Label" type="json-property"><entry name="string" outkey="22"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="payload.schema.json" inputinstance="absent-preview.json"/></data></component>
<component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance" inpkey="90"><entry name="document"><entry name="root"><entry name="object"><entry name="Code" type="json-property"><entry name="string" inpkey="30"/></entry><entry name="Label" type="json-property"><entry name="string" inpkey="31"/></entry></entry></entry></entry></entry></root><json schema="target.schema.json"/></data></component>
{parameter}</children><connections><edge from="10" to="20"/><edge from="21" to="30"/><edge from="22" to="31"/>{edge}</connections></structure></component></mapping>"#
        ),
    );
    package
}

#[test]
fn string_parser_with_unconnected_target_wrapper_executes() {
    let package = parser_package(false);
    let imported = mfd::import_with_profile(
        &package.mapping(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty());
    assert!(imported.project.extra_sources.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let input = format_xml::from_str(
        r#"<Input><Payload>{"Code":"001","Label":"aA café Ω"}</Payload></Input>"#,
        &imported.project.source,
    )
    .unwrap();
    let expected = Instance::Group(
        vec![
            ("Code".into(), Instance::Scalar(Value::String("001".into()))),
            (
                "Label".into(),
                Instance::Scalar(Value::String("aA café Ω".into())),
            ),
        ]
        .into(),
    );
    assert_eq!(engine::run(&imported.project, &input).unwrap(), expected);
    assert!(!package.0.join("absent-preview.json").exists());
}

#[test]
fn string_parser_does_not_mask_connected_target_filename() {
    let baseline = parser_package(false);
    let baseline = mfd::import(&baseline.mapping()).unwrap();
    let package = parser_package(true);
    let imported = assert_refusal(
        &package.mapping(),
        "JSON component `target` has connected FileInstance input port `90`; filename-driven documents are unsupported",
    );
    assert!(imported.project.extra_sources.is_empty());
    let parser_calls = |project: &mapping::Project| {
        project
            .graph
            .nodes
            .values()
            .filter_map(|node| match node {
                mapping::Node::Call { function, .. } if function.starts_with("json_parse") => {
                    Some(function.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let expected = parser_calls(&baseline.project);
    assert!(
        !expected.is_empty(),
        "the positive control must contain parser calls"
    );
    assert_eq!(parser_calls(&imported.project), expected);
    assert!(!package.0.join("absent-preview.json").exists());
}
