use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, NodeId, Project, Scope};
use mfd::{ImportIssueKind, ImportOptions, ImportOutcome, ImportProfile, MfdError};

const GLOBAL_ID_WARNING: &str =
    "auto-number with a global identifier is unsupported; imported as Null";

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_auto_number_diagnostics_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
enum CounterUse {
    Direct,
    Reused,
    ScalarFunction,
    Unused,
    UnusedConsumer,
}

fn write_design(dir: &Path, global_id: bool, reset: bool, usage: CounterUse) -> PathBuf {
    std::fs::write(
        dir.join("packet.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Packet"><xs:complexType><xs:sequence>
    <xs:element name="Batch" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Header" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Token" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
      <xs:element name="Entry" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Label" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("result.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Result"><xs:complexType><xs:sequence>
    <xs:element name="Batch" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Number" type="xs:integer" minOccurs="0"/>
        <xs:element name="Copy" type="xs:integer" minOccurs="0"/>
        <xs:element name="Label" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();

    let id_edge = if global_id {
        r#"<edge from="590" to="610"/>"#
    } else {
        ""
    };
    let reset_edge = if reset {
        r#"<edge from="501" to="613"/>"#
    } else {
        ""
    };
    let (consumer, counter_edges, definition) = match usage {
        CounterUse::Direct => ("", r#"<edge from="614" to="702"/>"#, ""),
        CounterUse::Reused => (
            "",
            r#"<edge from="614" to="702"/><edge from="614" to="703"/>"#,
            "",
        ),
        CounterUse::ScalarFunction => (
            r#"<component name="AddOne" library="generic" kind="19" uid="4"><data>
              <root><entry name="Value" inpkey="620" componentid="100"/></root>
              <root rootindex="1"><entry name="Result" outkey="621" componentid="102"/></root>
            </data></component>"#,
            r#"<edge from="614" to="620"/><edge from="621" to="702"/>"#,
            r#"<component name="AddOne" library="generic"><structure><children>
              <component name="Value" library="core" kind="6" uid="100">
                <targets><datapoint pos="0" key="1000"/></targets>
                <data><input datatype="integer"/></data>
              </component>
              <component name="constant" library="core" kind="2" uid="101">
                <targets><datapoint pos="0" key="1001"/></targets>
                <data><constant value="1" datatype="integer"/></data>
              </component>
              <component name="add" library="core" kind="5" uid="103">
                <sources><datapoint pos="0" key="1002"/><datapoint pos="1" key="1003"/></sources>
                <targets><datapoint pos="0" key="1004"/></targets>
              </component>
              <component name="Result" library="core" kind="7" uid="102">
                <sources><datapoint pos="0" key="1005"/></sources>
                <data><output datatype="integer"/></data>
              </component>
            </children><connections>
              <edge from="1000" to="1002"/><edge from="1001" to="1003"/>
              <edge from="1004" to="1005"/>
            </connections></structure></component>"#,
        ),
        CounterUse::Unused => ("", "", ""),
        CounterUse::UnusedConsumer => (
            r#"<component name="string" library="core" kind="5" uid="4">
              <sources><datapoint pos="0" key="620"/></sources>
              <targets><datapoint pos="0" key="621"/></targets>
            </component>"#,
            r#"<edge from="614" to="620"/>"#,
            "",
        ),
    };
    let design = format!(
        r#"<mapping version="26"><component name="main"><structure><children>
  <component name="packet" library="xml" kind="14" uid="1"><data>
    <root><entry name="Packet"><entry name="Batch" outkey="500">
      <entry name="Header"><entry name="Token" outkey="501"/></entry>
      <entry name="Entry" outkey="502"><entry name="Label" outkey="503"/></entry>
    </entry></entry></root>
    <document schema="packet.xsd" instanceroot="{{}}Packet"/>
  </data></component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint pos="0" key="590"/></targets>
    <data><constant value="generic-sequence" datatype="string"/></data>
  </component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint pos="0" key="591"/></targets>
    <data><constant value="3" datatype="integer"/></data>
  </component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint pos="0" key="592"/></targets>
    <data><constant value="4" datatype="integer"/></data>
  </component>
  <component name="auto-number" library="core" kind="5" uid="3">
    <sources><datapoint pos="0" key="610"/><datapoint pos="1" key="611"/>
      <datapoint pos="2" key="612"/><datapoint pos="3" key="613"/></sources>
    <targets><datapoint pos="0" key="614"/></targets>
  </component>
  {consumer}
  <component name="result" library="xml" kind="14" uid="2"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Result"><entry name="Batch" inpkey="700"><entry name="Item" inpkey="701">
      <entry name="Number" inpkey="702"/><entry name="Copy" inpkey="703"/><entry name="Label" inpkey="704"/>
    </entry></entry></entry></root>
    <document schema="result.xsd" instanceroot="{{}}Result"/>
  </data></component>
</children><connections>
  <edge from="500" to="700"/><edge from="502" to="701"/><edge from="503" to="704"/>
  <edge from="591" to="611"/><edge from="592" to="612"/>
  {id_edge}{reset_edge}{counter_edges}
</connections></structure></component>{definition}</mapping>"#
    );
    let path = dir.join("counter.mfd");
    std::fs::write(&path, design).unwrap();
    path
}

fn options(path: &Path) -> ImportOptions {
    ImportOptions::default().with_package_root(path.parent().unwrap())
}

fn best_effort(path: &Path) -> ImportOutcome {
    mfd::import_with_profile(path, &options(path), ImportProfile::BestEffort).unwrap()
}

fn assert_global_id_refused(path: &Path) -> Project {
    let outcome = best_effort(path);
    assert_eq!(outcome.imported.warnings, [GLOBAL_ID_WARNING]);
    assert!(!outcome.report.executable);
    assert_eq!(outcome.report.issues.len(), 1);
    assert_eq!(
        outcome.report.issues[0].kind,
        ImportIssueKind::ImportWarning
    );
    assert_eq!(outcome.report.issues[0].message, GLOBAL_ID_WARNING);
    assert!(engine::validate(&outcome.imported.project).is_empty());

    let legacy = mfd::import(path).unwrap();
    assert_eq!(legacy.warnings, [GLOBAL_ID_WARNING]);
    assert_eq!(
        mapping::project_file::encode_pretty(&legacy.project).unwrap(),
        mapping::project_file::encode_pretty(&outcome.imported.project).unwrap()
    );
    match mfd::import_with_profile(path, &options(path), ImportProfile::Executable) {
        Err(MfdError::IncompatibleImport(report)) => assert_eq!(*report, outcome.report),
        Err(other) => panic!("unexpected strict import error: {other}"),
        Ok(_) => panic!("strict import admitted unsupported global-ID auto-number semantics"),
    }
    outcome.imported.project
}

fn assert_admitted(path: &Path) -> Project {
    let outcome = best_effort(path);
    assert!(
        outcome.imported.warnings.is_empty(),
        "{:?}",
        outcome.imported.warnings
    );
    assert!(outcome.report.executable);
    assert!(outcome.report.issues.is_empty());
    let strict = mfd::import_with_profile(path, &options(path), ImportProfile::Executable).unwrap();
    assert!(strict.imported.warnings.is_empty());
    assert_eq!(strict.report, outcome.report);
    assert_eq!(
        mapping::project_file::encode_pretty(&strict.imported.project).unwrap(),
        mapping::project_file::encode_pretty(&outcome.imported.project).unwrap()
    );
    assert!(engine::validate(&strict.imported.project).is_empty());
    strict.imported.project
}

fn item_scope(project: &Project) -> &Scope {
    let batch = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Batch")
        .unwrap();
    batch
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap()
}

fn binding(project: &Project, field: &str) -> NodeId {
    item_scope(project)
        .bindings
        .iter()
        .find(|binding| binding.target_field == field)
        .unwrap()
        .node
}

fn null_nodes(project: &Project) -> Vec<NodeId> {
    project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| matches!(node, Node::Const { value: Value::Null }).then_some(id))
        .collect()
}

#[test]
fn used_global_id_has_explicit_best_effort_diagnostic_and_strict_refusal() {
    for reset in [false, true] {
        let dir = Directory::new();
        let design = write_design(&dir.0, true, reset, CounterUse::Direct);
        let project = assert_global_id_refused(&design);
        assert_eq!(null_nodes(&project), [binding(&project, "Number")]);
        assert!(
            !project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Position { .. }))
        );
    }
}

#[test]
fn global_id_used_through_unanchored_scalar_function_is_also_refused() {
    let dir = Directory::new();
    let design = write_design(&dir.0, true, false, CounterUse::ScalarFunction);
    let project = assert_global_id_refused(&design);
    let Node::UserFunctionCall { args, .. } = &project.graph.nodes[&binding(&project, "Number")]
    else {
        panic!("fixture must retain the scalar callable consumer, not inline its argument");
    };
    assert_eq!(args, &null_nodes(&project));
    assert_eq!(args.len(), 1);
    assert_eq!(project.user_functions.len(), 1);
}

#[test]
fn reusing_one_connected_global_id_component_reports_it_once() {
    let dir = Directory::new();
    let design = write_design(&dir.0, true, false, CounterUse::Reused);
    let project = assert_global_id_refused(&design);
    let number = binding(&project, "Number");
    assert_eq!(number, binding(&project, "Copy"));
    assert_eq!(null_nodes(&project), [number]);
}

#[test]
fn unused_global_id_placeholders_do_not_reject_executable_import() {
    for usage in [CounterUse::Unused, CounterUse::UnusedConsumer] {
        let dir = Directory::new();
        let design = write_design(&dir.0, true, true, usage);
        let project = assert_admitted(&design);
        assert_eq!(item_scope(&project).bindings.len(), 1);
        assert_eq!(item_scope(&project).bindings[0].target_field, "Label");
        assert!(null_nodes(&project).is_empty());
        assert!(
            project
                .graph
                .nodes
                .values()
                .all(|node| matches!(node, Node::SourceField { .. }))
        );
    }
}

fn assert_positional_output(project: &Project) {
    assert!(null_nodes(project).is_empty());
    assert!(matches!(
        &project.graph.nodes[&binding(project, "Number")],
        Node::Call { function, .. } if function == "add"
    ));
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Position { collection } if collection.is_empty()))
            .count(),
        1
    );
    let input = format_xml::from_str(
        "<Packet><Batch><Header><Token>A</Token></Header><Entry><Label>a</Label></Entry><Entry><Label>b</Label></Entry></Batch>\
         <Batch><Header><Token>B</Token></Header><Entry><Label>c</Label></Entry><Entry><Label>d</Label></Entry></Batch></Packet>",
        &project.source,
    ).unwrap();
    // Build the complete expected tree directly. Parsing an expected XML
    // document would insert Null for the unbound optional Copy field, whereas
    // the mapping must leave that field absent.
    let item = |number, label: &str| {
        Instance::Group(
            vec![
                ("Number".into(), Instance::Scalar(Value::Int(number))),
                (
                    "Label".into(),
                    Instance::Scalar(Value::String(label.into())),
                ),
            ]
            .into(),
        )
    };
    let batch = |items| Instance::Group(vec![("Item".into(), Instance::Repeated(items))].into());
    let expected = Instance::Group(
        vec![(
            "Batch".into(),
            Instance::Repeated(vec![
                batch(vec![item(3, "a"), item(7, "b")]),
                batch(vec![item(3, "c"), item(7, "d")]),
            ]),
        )]
        .into(),
    );
    assert_eq!(engine::run(project, &input).unwrap(), expected);
}

#[test]
fn disconnected_global_id_pin_preserves_ordinary_positional_counter() {
    let dir = Directory::new();
    let design = write_design(&dir.0, false, false, CounterUse::Direct);
    let project = assert_admitted(&design);
    assert_positional_output(&project);
}

#[test]
fn supported_related_scope_reset_does_not_warn_for_eager_null_placeholder() {
    let dir = Directory::new();
    let design = write_design(&dir.0, false, true, CounterUse::Direct);
    let project = assert_admitted(&design);
    assert_positional_output(&project);
}

fn replace_once(design: &mut String, original: &str, replacement: &str) {
    assert_eq!(
        design.matches(original).count(),
        1,
        "fixture mutation must be unique"
    );
    *design = design.replacen(original, replacement, 1);
}

fn write_multiple_counter_design(
    dir: &Path,
    same_id: bool,
    second_used: bool,
    unused_tail: bool,
) -> PathBuf {
    let path = write_design(dir, true, false, CounterUse::Direct);
    let mut design = std::fs::read_to_string(&path).unwrap();
    assert_eq!(design.matches("name=\"auto-number\"").count(), 1);
    assert_eq!(design.matches(r#"<edge from="614" to="702"/>"#).count(), 1);
    assert!(!design.contains(r#"to="703""#));
    let extra_id = if same_id {
        ""
    } else {
        r#"<component name="constant" library="core" kind="2" uid="8">
          <targets><datapoint pos="0" key="595"/></targets>
          <data><constant value="another-generic-sequence" datatype="string"/></data>
        </component>"#
    };
    let tail_components = if unused_tail {
        r#"<component name="auto-number" library="core" kind="5" uid="6">
          <sources><datapoint pos="0" key="650"/><datapoint/><datapoint/><datapoint/></sources>
          <targets><datapoint pos="0" key="654"/></targets>
        </component>
        <component name="string" library="core" kind="5" uid="7">
          <sources><datapoint pos="0" key="660"/></sources>
          <targets><datapoint pos="0" key="661"/></targets>
        </component>"#
    } else {
        ""
    };
    replace_once(
        &mut design,
        "</children><connections>",
        &format!(
            r#"{extra_id}
            <component name="auto-number" library="core" kind="5" uid="5">
              <sources><datapoint pos="0" key="630"/><datapoint/><datapoint/><datapoint/></sources>
              <targets><datapoint pos="0" key="634"/></targets>
            </component>
            {tail_components}</children><connections>"#
        ),
    );
    let id_feed = if same_id { "590" } else { "595" };
    let second_output = if second_used {
        r#"<edge from="634" to="703"/>"#
    } else {
        ""
    };
    let tail_edges = if unused_tail {
        r#"<edge from="590" to="650"/><edge from="654" to="660"/>"#
    } else {
        ""
    };
    replace_once(
        &mut design,
        "</connections></structure></component>",
        &format!(
            r#"<edge from="{id_feed}" to="630"/>{second_output}{tail_edges}
            </connections></structure></component>"#
        ),
    );

    // Establish distinct physical components and exact connected ID/output
    // ownership before an importer result can count as a diagnostic control.
    let document = roxmltree::Document::parse(&design).unwrap();
    let counters = document
        .descendants()
        .filter(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("auto-number")
        })
        .collect::<Vec<_>>();
    let expected_uids = if unused_tail {
        vec!["3", "5", "6"]
    } else {
        vec!["3", "5"]
    };
    assert_eq!(
        counters
            .iter()
            .map(|node| node.attribute("uid").unwrap())
            .collect::<Vec<_>>(),
        expected_uids
    );
    let expected_outputs = if unused_tail {
        vec!["614", "634", "654"]
    } else {
        vec!["614", "634"]
    };
    assert_eq!(
        counters
            .iter()
            .map(|node| {
                node.children()
                    .find(|child| child.has_tag_name("targets"))
                    .unwrap()
                    .children()
                    .find(|child| child.has_tag_name("datapoint"))
                    .unwrap()
                    .attribute("key")
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        expected_outputs
    );
    let feeds = |input| {
        document
            .descendants()
            .filter(|node| node.has_tag_name("edge") && node.attribute("to") == Some(input))
            .map(|node| node.attribute("from").unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(feeds("610"), ["590"]);
    assert_eq!(feeds("630"), [id_feed]);
    assert_eq!(feeds("702"), ["614"]);
    assert_eq!(
        feeds("703"),
        if second_used { vec!["634"] } else { Vec::new() }
    );
    if unused_tail {
        assert_eq!(feeds("650"), ["590"]);
        assert_eq!(feeds("660"), ["654"]);
        assert!(
            !document
                .descendants()
                .any(|node| node.has_tag_name("edge") && node.attribute("from") == Some("661"))
        );
    }
    std::fs::write(&path, &design).unwrap();
    path
}

fn assert_retained_counter_diagnostics(path: &Path, counter_fields: &[&str]) {
    assert!(!counter_fields.is_empty());
    let expected_warnings = vec![GLOBAL_ID_WARNING.to_string(); counter_fields.len()];
    let expected_report = mfd::ImportReport {
        executable: false,
        issues: vec![
            mfd::ImportIssue {
                kind: ImportIssueKind::ImportWarning,
                message: GLOBAL_ID_WARNING.to_string(),
            };
            counter_fields.len()
        ],
    };
    let outcome = best_effort(path);
    assert_eq!(outcome.imported.warnings, expected_warnings);
    assert_eq!(outcome.report, expected_report);
    let project = &outcome.imported.project;
    assert!(engine::validate(project).is_empty());
    let mut expected_fields = counter_fields.to_vec();
    expected_fields.push("Label");
    assert_eq!(
        item_scope(project)
            .bindings
            .iter()
            .map(|binding| binding.target_field.as_str())
            .collect::<Vec<_>>(),
        expected_fields
    );
    let counter_nodes = counter_fields
        .iter()
        .map(|field| binding(project, field))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        counter_nodes.len(),
        counter_fields.len(),
        "distinct components must not share their Null placeholder"
    );
    let label = binding(project, "Label");
    assert!(!counter_nodes.contains(&label));
    assert_eq!(
        null_nodes(project),
        counter_nodes.iter().copied().collect::<Vec<_>>()
    );
    let mut expected_nodes = counter_nodes
        .into_iter()
        .map(|id| (id, Node::Const { value: Value::Null }))
        .collect::<std::collections::BTreeMap<_, _>>();
    expected_nodes.insert(
        label,
        Node::SourceField {
            path: vec!["Label".into()],
            frame: Some(vec!["Batch".into(), "Entry".into()]),
        },
    );
    assert_eq!(
        serde_json::to_value(&project.graph.nodes).unwrap(),
        serde_json::to_value(expected_nodes).unwrap()
    );
    assert!(project.extra_targets.is_empty());
    assert!(project.user_functions.is_empty());

    let legacy = mfd::import(path).unwrap();
    assert_eq!(legacy.warnings, expected_warnings);
    assert_eq!(
        mapping::project_file::encode_pretty(&legacy.project).unwrap(),
        mapping::project_file::encode_pretty(project).unwrap()
    );
    match mfd::import_with_profile(path, &options(path), ImportProfile::Executable) {
        Err(MfdError::IncompatibleImport(report)) => assert_eq!(*report, expected_report),
        Err(other) => panic!("unexpected strict import error: {other}"),
        Ok(_) => panic!("strict import admitted retained global-ID auto-number semantics"),
    }
}

#[test]
fn distinct_retained_global_id_components_each_produce_one_diagnostic() {
    for same_id in [false, true] {
        let dir = Directory::new();
        let design = write_multiple_counter_design(&dir.0, same_id, true, false);
        assert_retained_counter_diagnostics(&design, &["Number", "Copy"]);
    }
}

#[test]
fn mixed_used_and_unused_global_id_components_only_warn_for_retained_counters() {
    for second_used in [false, true] {
        let dir = Directory::new();
        let design = write_multiple_counter_design(&dir.0, true, second_used, true);
        let counter_fields: &[&str] = if second_used {
            &["Number", "Copy"]
        } else {
            &["Number"]
        };
        assert_retained_counter_diagnostics(&design, counter_fields);
    }
}
