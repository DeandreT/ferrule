use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::IterationOutput;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_first_parent_presence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Fixture {
    let directory = Fixture::new();
    std::fs::write(directory.0.join("source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Department" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Id" type="xs:string"/><xs:element name="Person" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Keep" type="xs:boolean"/><xs:element name="Score" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    std::fs::write(directory.0.join("target.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Department" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Id" type="xs:string"/><xs:element name="Selected"><xs:complexType><xs:sequence><xs:element name="Name" minOccurs="0" type="xs:string"/><xs:element name="Position" minOccurs="0" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    std::fs::write(directory.0.join("mapping.mfd"), r#"<mapping version="26"><component name="map"><structure><children>
      <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Department" outkey="10"><entry name="Id" outkey="11"/><entry name="Person" outkey="12"><entry name="Name" outkey="13"/><entry name="Keep" outkey="14"/><entry name="Score" outkey="15"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
      <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Department" inpkey="80"><entry name="Id" inpkey="81"/><entry name="Selected" inpkey="82"><entry name="Name" inpkey="83"/><entry name="Position" inpkey="84"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
      <component name="sort" library="core" kind="30"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets><data><sort><collation/><key direction="descending"/></sort></data></component>
      <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/><datapoint/></targets></component>
      <component name="skip-first-items" library="core" kind="5"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint pos="0" key="42"/></targets></component>
      <component name="first-items" library="core" kind="5"><sources><datapoint pos="0" key="50"/><datapoint pos="1" key="51"/></sources><targets><datapoint pos="0" key="52"/></targets></component>
      <component name="first-items" library="core" kind="5"><sources><datapoint pos="0" key="60"/></sources><targets><datapoint pos="0" key="62"/></targets></component>
      <component name="payload" library="xml" kind="14"><data><parameter usageKind="variable"/><root><entry name="compute-when" inpkey="105"/><entry name="document"><entry name="Person" inpkey="101" outkey="100"><entry name="Name" outkey="102"/><entry name="Keep" outkey="103"/><entry name="Score" outkey="104"/></entry></entry></root><document schema="source.xsd" instanceroot="{}Source/{}Department/{}Person"/></data></component>
      <component name="position" library="core" kind="5"><sources><datapoint pos="0" key="109"/></sources><targets><datapoint pos="0" key="110"/></targets></component>
      <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="200"/></targets><data><constant value="1" datatype="integer"/></data></component>
      <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="201"/></targets><data><constant value="2" datatype="integer"/></data></component>
    </children><graph><edges><edge edgekey="999"><data><dataconnection type="2"/></data></edge></edges><vertices>
      <vertex vertexkey="10"><edges><edge vertexkey="80"/><edge vertexkey="82"/><edge vertexkey="105"/></edges></vertex>
      <vertex vertexkey="11"><edges><edge vertexkey="81"/></edges></vertex>
      <vertex vertexkey="12"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="15"><edges><edge vertexkey="21"/></edges></vertex>
      <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="14"><edges><edge vertexkey="31"/></edges></vertex>
      <vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="200"><edges><edge vertexkey="41"/></edges></vertex>
      <vertex vertexkey="42"><edges><edge vertexkey="50"/></edges></vertex><vertex vertexkey="201"><edges><edge vertexkey="51"/></edges></vertex>
      <vertex vertexkey="52"><edges><edge vertexkey="60"/></edges></vertex><vertex vertexkey="62"><edges><edge vertexkey="101" edgekey="999"/></edges></vertex>
      <vertex vertexkey="100"><edges><edge vertexkey="109"/></edges></vertex><vertex vertexkey="102"><edges><edge vertexkey="83"/></edges></vertex><vertex vertexkey="110"><edges><edge vertexkey="84"/></edges></vertex>
    </vertices></graph></structure></component></mapping>"#).unwrap();
    directory
}

fn rewrite(directory: &Fixture, change: impl FnOnce(String) -> String) {
    let path = directory.0.join("mapping.mfd");
    std::fs::write(&path, change(std::fs::read_to_string(&path).unwrap())).unwrap();
}

fn assert_rejected(directory: &Fixture) {
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.starts_with("first-item parent-driven XML group")),
        "{:?}",
        imported.warnings
    );
    assert!(matches!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
    assert_ne!(
        imported.project.root.children[0].children[0].iteration_output,
        IterationOutput::First
    );
}

#[test]
fn parent_presence_reconstructs_controls_and_empty_first_group() {
    let directory = fixture();
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = &imported.project.root.children[0].children[0];
    assert_eq!(selected.iteration_output, IterationOutput::First);
    assert!(selected.filter.is_some());
    assert!(selected.sort_by.is_some());
    assert!(selected.sort_descending);
    assert_eq!(selected.windows.len(), 2);
    let source = format_xml::from_str("<Source><Department><Id>A</Id><Person><Name>discard</Name><Keep>false</Keep><Score>100</Score></Person><Person><Name>first</Name><Keep>true</Keep><Score>90</Score></Person><Person><Name>selected</Name><Keep>true</Keep><Score>80</Score></Person><Person><Name>later</Name><Keep>true</Keep><Score>70</Score></Person></Department><Department><Id>B</Id><Person><Name>none</Name><Keep>false</Keep><Score>10</Score></Person></Department></Source>", &imported.project.source).unwrap();
    let actual = engine::run(&imported.project, &source).unwrap();
    let departments = actual
        .field("Department")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        departments[0]
            .field("Selected")
            .and_then(|item| item.field("Name"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("selected".into()))
    );
    assert_eq!(
        departments[0]
            .field("Selected")
            .and_then(|item| item.field("Position"))
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        departments[1].field("Selected"),
        Some(&Instance::Group(Vec::new()))
    );
    let second = directory.0.join("second.mfd");
    assert!(mfd::export(&imported.project, &second).unwrap().is_empty());
    let imported_twice = mfd::import(&second).unwrap();
    assert!(
        imported_twice.warnings.is_empty(),
        "{:?}",
        imported_twice.warnings
    );
    assert_eq!(
        engine::run(&imported_twice.project, &source).unwrap(),
        actual
    );
}

#[test]
fn independent_constant_and_parent_broadcast_leaves_warn() {
    for port in [200, 11] {
        let directory = fixture();
        rewrite(&directory, |xml| {
            xml.replace(
                "<vertex vertexkey=\"102\"><edges><edge vertexkey=\"83\"/></edges></vertex>",
                &format!(
                    "<vertex vertexkey=\"{port}\"><edges><edge vertexkey=\"83\"/></edges></vertex>"
                ),
            )
        });
        assert_rejected(&directory);
    }
}

#[test]
fn missing_structural_copy_and_default_first_warn() {
    let directory = fixture();
    rewrite(&directory, |xml| xml.replace(" edgekey=\"999\"", ""));
    assert_rejected(&directory);
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace(
            "<datapoint pos=\"0\" key=\"60\"/>",
            "<datapoint pos=\"0\" key=\"60\"/><datapoint pos=\"1\" key=\"61\"/>",
        )
        .replace(
            "<vertex vertexkey=\"201\"><edges><edge vertexkey=\"51\"/>",
            "<vertex vertexkey=\"201\"><edges><edge vertexkey=\"51\"/><edge vertexkey=\"61\"/>",
        )
    });
    assert_rejected(&directory);
}

#[test]
fn multiple_payload_contexts_warn() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        let document = roxmltree::Document::parse(&xml).unwrap();
        let component = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("payload")
            })
            .unwrap();
        let duplicate = xml[component.range()]
            .replace("name=\"payload\"", "name=\"second-payload\"")
            .replace("\"100\"", "\"300\"")
            .replace("\"101\"", "\"301\"")
            .replace("\"102\"", "\"302\"")
            .replace("\"103\"", "\"303\"")
            .replace("\"104\"", "\"304\"")
            .replace("\"105\"", "\"305\"");
        xml.replace(
            "</children><graph>",
            &format!("{duplicate}</children><graph>"),
        )
        .replace(
            "<edge vertexkey=\"105\"/>",
            "<edge vertexkey=\"105\"/><edge vertexkey=\"305\"/>",
        )
        .replace(
            "<edge vertexkey=\"101\" edgekey=\"999\"/>",
            "<edge vertexkey=\"101\" edgekey=\"999\"/><edge vertexkey=\"301\" edgekey=\"999\"/>",
        )
        .replace(
            "<vertex vertexkey=\"102\"><edges><edge vertexkey=\"83\"/></edges></vertex>",
            "<vertex vertexkey=\"302\"><edges><edge vertexkey=\"83\"/></edges></vertex>",
        )
    });
    assert_rejected(&directory);
}

#[test]
fn foreign_parent_driver_warns() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace("<edge vertexkey=\"80\"/>", "").replace(
            "<vertex vertexkey=\"12\"><edges>",
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"80\"/>",
        )
    });
    assert_rejected(&directory);
}

#[test]
fn missing_position_and_wrong_compute_parent_warn() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace(
            "<vertex vertexkey=\"110\"><edges><edge vertexkey=\"84\"/></edges></vertex>",
            "",
        )
    });
    assert_rejected(&directory);
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace("<edge vertexkey=\"105\"/>", "").replace(
            "<vertex vertexkey=\"12\"><edges>",
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"105\"/>",
        )
    });
    assert_rejected(&directory);
}

#[test]
fn explicit_projection_and_uninterpreted_condition_warn() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace(
            "<entry name=\"Name\" outkey=\"102\"/>",
            "<entry name=\"Name\" inpkey=\"106\" outkey=\"102\"/>",
        )
        .replace(
            "<vertex vertexkey=\"15\"><edges>",
            "<vertex vertexkey=\"15\"><edges><edge vertexkey=\"106\"/>",
        )
    });
    assert_rejected(&directory);
    for port in ["inpkey=\"82\"", "outkey=\"100\"", "inpkey=\"83\""] {
        let directory = fixture();
        rewrite(&directory, |xml| {
            let needle = format!("{port}>");
            if xml.contains(&needle) {
                xml.replace(&needle, &format!("{port}><condition><expression><constant value=\"false\" datatype=\"boolean\"/></expression></condition>"))
            } else {
                xml.replace(&format!("{port}/>"), &format!("{port}><condition><expression><constant value=\"false\" datatype=\"boolean\"/></expression></condition></entry>"))
            }
        });
        assert_rejected(&directory);
    }
}

#[test]
fn scalar_function_does_not_certify_exact_item_context() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace("</children><graph>", "<component name=\"string\" library=\"core\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"210\"/></sources><targets><datapoint pos=\"0\" key=\"211\"/></targets></component></children><graph>").replace("<vertex vertexkey=\"110\"><edges><edge vertexkey=\"84\"/></edges></vertex>", "<vertex vertexkey=\"110\"><edges><edge vertexkey=\"210\"/></edges></vertex><vertex vertexkey=\"211\"><edges><edge vertexkey=\"84\"/></edges></vertex>")
    });
    assert_rejected(&directory);
}

#[test]
fn unkeyed_target_variable_and_source_conditions_warn() {
    const CONDITION: &str = "<condition><expression><constant value=\"false\" datatype=\"boolean\"/></expression></condition>";
    let directory = fixture();
    rewrite(&directory, |xml| {
        let document = roxmltree::Document::parse(&xml).unwrap();
        let component = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
            .unwrap();
        let body = &xml[component.range()];
        xml.replace(
            body,
            &body
                .replace(
                    "<root>",
                    &format!("<root><entry name=\"document\">{CONDITION}"),
                )
                .replace("</root>", "</entry></root>"),
        )
    });
    assert_rejected(&directory);
    for (needle, occurrence) in [
        ("<entry name=\"Target\">", 0),
        ("<entry name=\"document\">", 0),
        ("<entry name=\"Source\">", 0),
        ("<entry name=\"Person\" outkey=\"12\">", 0),
        ("<entry name=\"Name\" outkey=\"13\"/>", 0),
    ] {
        let directory = fixture();
        rewrite(&directory, |xml| {
            assert_eq!(occurrence, 0);
            if needle.ends_with("/>") {
                let open = needle.trim_end_matches("/>");
                xml.replacen(needle, &format!("{open}>{CONDITION}</entry>"), 1)
            } else {
                xml.replacen(needle, &format!("{needle}{CONDITION}"), 1)
            }
        });
        assert_rejected(&directory);
    }
}

#[test]
fn unrelated_unkeyed_sibling_condition_keeps_first_reconstruction() {
    let directory = fixture();
    rewrite(&directory, |xml| {
        xml.replace(
        "<entry name=\"Selected\" inpkey=\"82\">",
        "<entry name=\"Ignored\"><condition><expression><constant value=\"false\" datatype=\"boolean\"/></expression></condition><entry name=\"Marker\" inpkey=\"270\"/></entry><entry name=\"Selected\" inpkey=\"82\">",
    )
    });
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .all(|warning| !warning.starts_with("first-item parent-driven XML group")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.warnings.iter().any(|warning| {
        warning.starts_with("conditional XML type alternatives at `Department/Ignored`")
    }));
    assert_eq!(
        imported.project.root.children[0].children[0].iteration_output,
        IterationOutput::First
    );
}
