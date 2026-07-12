use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mapped_group_sequence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn write_fixture(dir: &Path) {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Person" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Include" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Selected"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Include" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Person" outkey="10"><entry name="Name" outkey="11"/><entry name="Include" outkey="12"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="30"/><datapoint/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Selected" inpkey="40"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );
}

fn rewrite_mapping(dir: &Path, rewrite: impl FnOnce(String) -> String) {
    let path = dir.join("mapping.mfd");
    let mapping = std::fs::read_to_string(&path).unwrap();
    write(&path, &rewrite(mapping));
}

fn output_xml(project: &mapping::Project, source_xml: &str) -> String {
    let source = format_xml::from_str(source_xml, &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    format_xml::to_string(&project.target, &target).unwrap()
}

fn mapped_names(project: &mapping::Project, source_xml: &str) -> Vec<String> {
    let source = format_xml::from_str(source_xml, &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    target
        .field("Selected")
        .and_then(Instance::as_mapped_sequence)
        .unwrap()
        .iter()
        .filter_map(|item| item.field("Name").and_then(Instance::as_scalar))
        .filter_map(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn nested_source_group_project(copy_extra: bool) -> Project {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Order",
                vec![SchemaNode::group(
                    "Customer",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Extra", ScalarType::String),
                    ],
                )],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Header",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Extra", ScalarType::String),
            ],
        )],
    );
    let mut nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            frame: Some(vec!["Order".into()]),
            path: vec!["Customer".into(), "Name".into()],
        },
    )]);
    let mut bindings = vec![Binding {
        target_field: "Name".into(),
        node: 0,
    }];
    if copy_extra {
        nodes.insert(
            1,
            Node::SourceField {
                frame: Some(vec!["Order".into()]),
                path: vec!["Customer".into(), "Extra".into()],
            },
        );
        bindings.push(Binding {
            target_field: "Extra".into(),
            node: 1,
        });
    }
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: mapping::FormatOptions::default(),
        target_options: mapping::FormatOptions::default(),
        extra_sources: Vec::new(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Header".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                iteration_output: IterationOutput::MappedSequence,
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn nested_source_xml() -> &'static str {
    "<Source><Order><Customer><Name>Ada</Name><Extra>A</Extra></Customer></Order><Order><Customer><Name>Grace</Name><Extra>G</Extra></Customer></Order></Source>"
}

#[test]
fn filtered_group_port_emits_zero_one_or_many_non_repeating_xml_elements() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let selected = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Selected")
        .unwrap();
    assert_eq!(selected.iteration_output, IterationOutput::MappedSequence);
    assert!(!imported.project.target.child("Selected").unwrap().repeating);

    let cases = [
        (
            "<Source><Person><Name>none</Name><Include>false</Include></Person></Source>",
            Vec::<String>::new(),
        ),
        (
            "<Source><Person><Name>one</Name><Include>true</Include></Person></Source>",
            vec!["one".to_string()],
        ),
        (
            "<Source><Person><Name>first</Name><Include>true</Include></Person><Person><Name>discarded</Name><Include>false</Include></Person><Person><Name>second</Name><Include>true</Include></Person></Source>",
            vec!["first".to_string(), "second".to_string()],
        ),
    ];
    for (source, expected) in &cases {
        assert_eq!(mapped_names(&imported.project, source), *expected);
        assert_eq!(
            output_xml(&imported.project, source)
                .matches("<Selected>")
                .count(),
            expected.len()
        );
    }

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        std::fs::read_to_string(&exported)
            .unwrap()
            .contains("dataconnection type=\"2\"")
    );
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(
        engine::validate(&reimported.project).is_empty(),
        "{:?}",
        engine::validate(&reimported.project)
    );
    for (source, _) in &cases {
        assert_eq!(
            mapped_names(&imported.project, source),
            mapped_names(&reimported.project, source)
        );
        assert_eq!(
            output_xml(&imported.project, source),
            output_xml(&reimported.project, source)
        );
    }

    let mut non_xml_target = imported.project.clone();
    non_xml_target.target_path = Some("target.json".to_string());
    let non_xml_path = dir.0.join("non-xml.mfd");
    assert!(matches!(
        mfd::export(&non_xml_target, &non_xml_path),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("only for XML targets")
    ));
    assert!(!non_xml_path.exists());

    let mut lossy = imported.project.clone();
    let selected = lossy
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Selected")
        .unwrap();
    let name = selected
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Name")
        .unwrap()
        .node;
    lossy.graph.nodes.insert(
        name,
        Node::Const {
            value: Value::String("computed".into()),
        },
    );
    let rejected = dir.0.join("lossy.mfd");
    assert!(matches!(
        mfd::export(&lossy, &rejected),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("mapped XML group sequences")
    ));
    assert!(!rejected.exists());

    let mut first = imported.project.clone();
    first
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "Selected")
        .unwrap()
        .iteration_output = IterationOutput::First;
    let first_path = dir.0.join("first.mfd");
    write(&first_path, "existing design");
    assert!(matches!(
        mfd::export(&first, &first_path),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("first-item")
    ));
    assert_eq!(
        std::fs::read_to_string(first_path).unwrap(),
        "existing design"
    );
}

#[test]
fn export_uses_the_selected_group_below_the_repeated_collection() {
    let dir = TempDir::new();
    let project = nested_source_group_project(true);
    let path = dir.0.join("nested-copy.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let design = std::fs::read_to_string(&path).unwrap();
    assert!(design.contains("dataconnection type=\"2\""));

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        output_xml(&project, nested_source_xml()),
        output_xml(&imported.project, nested_source_xml())
    );
}

#[test]
fn explicit_subset_exports_as_an_ordinary_occurrence_wire() {
    let dir = TempDir::new();
    let project = nested_source_group_project(false);
    let path = dir.0.join("nested-explicit.mfd");
    assert!(mfd::export(&project, &path).unwrap().is_empty());
    let design = std::fs::read_to_string(&path).unwrap();
    assert!(!design.contains("dataconnection type=\"2\""));

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let expected = output_xml(&project, nested_source_xml());
    assert!(!expected.contains("<Extra>"));
    assert_eq!(expected, output_xml(&imported.project, nested_source_xml()));
}

#[test]
fn duplicate_target_port_aliases_for_one_feed_create_one_mapped_scope() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<entry name="Selected" inpkey="40"/>"#,
                r#"<entry name="Selected" inpkey="40"/><entry name="Selected" inpkey="41"/>"#,
            )
            .replace(
                r#"<edge vertexkey="40" edgekey="90"/>"#,
                r#"<edge vertexkey="40" edgekey="90"/><edge vertexkey="41" edgekey="90"/>"#,
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = imported
        .project
        .root
        .children
        .iter()
        .filter(|scope| scope.target_field == "Selected")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].iteration_output,
        IterationOutput::MappedSequence
    );
}

#[test]
fn competing_mapped_group_feeds_warn_once_and_create_no_occurrence_scope() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<entry name="Selected" inpkey="40"/>"#,
                r#"<entry name="Selected" inpkey="40"/><entry name="Selected" inpkey="41"/>"#,
            )
            .replace(
                r#"<edge edgekey="90"><data><dataconnection type="2"/></data></edge>"#,
                r#"<edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge>"#,
            )
            .replace(
                r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>"#,
                r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="41" edgekey="91"/></edges></vertex>"#,
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("multiple connected structural sequence feeds"));
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .all(|scope| scope.target_field != "Selected")
    );
}
