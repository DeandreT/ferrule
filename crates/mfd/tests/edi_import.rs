use std::path::{Path, PathBuf};

use mapping::{EdiBoundaryKind, Node, X12Separators};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn assert_source_boundary_roundtrip(project: &mapping::Project, directory: &Path) {
    let design = directory.join("roundtrip.mfd");
    let warnings = mfd::export(project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&design).unwrap();
    assert!(exported.contains("library=\"text\""));
    assert!(exported.contains("type=\"edi\""));
    assert!(exported.contains("ferrule-repeating="));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.source_options, project.source_options);
    assert_eq!(reimported.project.source_path, project.source_path);
}

#[test]
fn imports_edi_entry_tree_paths_and_honors_default_output() {
    let imported = mfd::import(&fixture("edi-entry-tree.mfd")).unwrap();
    let project = &imported.project;

    assert_eq!(project.source.name, "MFD-EDIFACT");
    assert_eq!(project.source_path.as_deref(), Some("orders.edi"));
    assert!(project.source_options.lenient_segments);
    assert_eq!(
        project.source_options.edi_kind,
        Some(EdiBoundaryKind::Edifact)
    );
    assert_eq!(project.target.name, "People");
    assert_eq!(project.target_path.as_deref(), Some("people.xml"));
    assert_eq!(project.extra_targets.len(), 1);
    assert_eq!(project.extra_targets[0].name, "ignored");
    assert_eq!(project.extra_targets[0].schema.name, "Ignored");
    assert_eq!(project.extra_targets[0].root.bindings.len(), 1);
    assert_eq!(
        project.extra_targets[0].root.bindings[0].target_field,
        "Value"
    );

    let interchange = project.source.child("Interchange").unwrap();
    assert!(interchange.repeating);
    let group = interchange.child("Group").unwrap();
    assert!(group.repeating);
    let message = group.child("Message").unwrap();
    assert!(message.repeating);
    let sg2 = message.child("SG2").unwrap();
    assert!(sg2.repeating);
    assert!(message.child("SG3").is_none());

    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(
        person.source(),
        Some(
            ["Interchange", "Group", "Message", "SG2"]
                .map(String::from)
                .as_slice()
        )
    );
    assert!(person.bindings.iter().all(|binding| {
        matches!(
            project.graph.nodes.get(&binding.node),
            Some(Node::SourceField { .. })
        )
    }));

    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("entry-tree schema inferred") && warning.contains("execution is disabled")
    }));
}

#[test]
fn imports_hl7_without_external_config_as_non_executable_graph() {
    let imported = mfd::import(&fixture("edi-unsupported.mfd")).unwrap();

    assert_eq!(imported.project.source.name, "HL7");
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("messages.hl7")
    );
    assert_eq!(imported.project.graph.nodes.len(), 1);
    assert_eq!(
        imported.project.source_options.edi_kind,
        Some(EdiBoundaryKind::Hl7)
    );
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("entry-tree schema inferred"))
    );
}

#[test]
fn imports_self_describing_edi_entry_tree_without_external_config() {
    let directory = TempDir::new("embedded_schema");
    let design = directory.path().join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="messages" library="text" kind="16"><data>
            <root><entry name="FileInstance"><entry name="document">
              <entry name="Envelope" ferrule-repeating="0">
                <entry name="Message" ferrule-repeating="1" outkey="10">
                  <entry name="Code" ferrule-repeating="0" ferrule-fixed="A" datatype="string" outkey="11"/>
                  <entry name="Count" ferrule-repeating="0" datatype="integer" outkey="12"/>
                </entry>
              </entry>
            </entry></entry></root>
            <text type="edi" kind="EDIHL7" inputinstance="messages.hl7"/>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="Result"><entry name="Code" inpkey="20"/><entry name="Count" inpkey="21"/></entry></root>
            <document outputinstance="result.xml" instanceroot="{}Result"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
          <vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_options.edi_kind,
        Some(EdiBoundaryKind::Hl7)
    );
    let message = imported.project.source.child("Message").unwrap();
    assert!(message.repeating);
    assert_eq!(message.child("Code").unwrap().fixed.as_deref(), Some("A"));
    assert!(matches!(
        message.child("Count").unwrap().kind,
        ir::SchemaKind::Scalar {
            ty: ir::ScalarType::Int
        }
    ));
    assert_source_boundary_roundtrip(&imported.project, directory.path());
}

#[test]
fn compiles_relative_x12_configuration_into_an_executable_schema() {
    let directory = TempDir::new("x12_config");
    let config_directory = directory.path().join("X12");
    std::fs::create_dir_all(&config_directory).unwrap();
    std::fs::write(
        config_directory.join("Defs.Segment"),
        r#"<Config><Elements>
          <Data name="F1" type="string"/>
          <Segment name="ISA"><Data ref="F1"/></Segment>
          <Segment name="GS"><Data ref="F1"/></Segment>
          <Segment name="ST"><Data ref="F1"/></Segment>
          <Segment name="SE"><Data ref="F1"/></Segment>
          <Segment name="GE"><Data ref="F1"/></Segment>
          <Segment name="IEA"><Data ref="F1"/></Segment>
        </Elements></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config_directory.join("Envelope.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
            <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
              <Segment ref="GS"/><Select field="ST/F1"/><Segment ref="GE" minOccurs="0"/>
            </Group><Segment ref="IEA" minOccurs="0"/>
          </Group></Group></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config_directory.join("850.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded">
            <Segment ref="ST"/><Segment ref="SE"/>
          </Group></Message></Config>"#,
    )
    .unwrap();
    let mfd_path = directory.path().join("mapping.mfd");
    std::fs::write(
        &mfd_path,
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1">
          <structure><children>
            <component name="edi" library="text" uid="2" kind="16"><properties/><data>
              <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
                <entry name="Interchange"><entry name="Group"><entry name="Message">
                  <entry name="ST"><entry name="F1" outkey="10"/></entry>
                  <entry name="ParserErrors_Message">
                    <entry name="LoopMF_AK3" outkey="30">
                      <entry name="MF_AK3" outkey="31"><entry name="F721" outkey="11"/></entry>
                      <entry name="MF_AK4" outkey="32"><entry name="C030" outkey="33">
                        <entry name="F722" outkey="12"/>
                      </entry></entry>
                    </entry>
                    <entry name="MF_AK5" outkey="34"><entry name="F717" outkey="13"/></entry>
                  </entry>
                </entry><entry name="ParserErrors_Group">
                  <entry name="MF_AK9" outkey="35"><entry name="F715" outkey="14"/></entry>
                </entry></entry></entry>
              </entry></entry></entry></root>
              <text type="edi" kind="EDIX12" config="X12\850.Config" inputinstance="input.x12">
                <settings interchangecontrolversionnumber="00505"><separators dataelement="+" component=":" segment="%27" repetition="%21" escape="%3F"/></settings>
              </text>
            </data></component>
            <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data>
              <root><entry name="Outputs">
                <entry name="Physical" inpkey="20"/><entry name="Ak3" inpkey="21"/>
                <entry name="Ak4" inpkey="22"/><entry name="Ak5" inpkey="23"/>
                <entry name="Ak9" inpkey="24"/>
              </entry></root>
            </data></component>
          </children><graph directed="1"><vertices>
            <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
            <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
            <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
            <vertex vertexkey="13"><edges><edge vertexkey="23"/></edges></vertex>
            <vertex vertexkey="14"><edges><edge vertexkey="24"/></edges></vertex>
          </vertices></graph></structure>
        </component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mfd_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_options.edi_kind,
        Some(EdiBoundaryKind::X12)
    );
    assert_eq!(
        imported.project.source_options.x12_separators,
        Some(X12Separators {
            element: '+',
            component: ':',
            segment: '\'',
            repetition: Some('!'),
            release: Some('?'),
        })
    );
    assert_eq!(
        imported
            .project
            .source_options
            .x12_interchange_version
            .as_deref(),
        Some("00505")
    );
    assert_eq!(
        format_edi::dialect_of(&imported.project.source).unwrap(),
        format_edi::Dialect::X12
    );
    assert!(
        imported
            .project
            .source
            .child("Interchange")
            .and_then(|node| node.child("Group"))
            .and_then(|node| node.child("Message"))
            .and_then(|node| node.child("ST"))
            .and_then(|node| node.child("F1"))
            .is_some()
    );
    let group = imported
        .project
        .source
        .child("Interchange")
        .and_then(|node| node.child("Group"))
        .unwrap();
    let message_errors = group
        .child("Message")
        .and_then(|node| node.child("ParserErrors_Message"))
        .unwrap();
    assert!(message_errors.repeating);
    assert!(
        message_errors
            .child("LoopMF_AK3")
            .and_then(|node| node.child("MF_AK3"))
            .and_then(|node| node.child("F721"))
            .is_some()
    );
    assert!(
        message_errors
            .child("LoopMF_AK3")
            .and_then(|node| node.child("MF_AK4"))
            .and_then(|node| node.child("C030"))
            .and_then(|node| node.child("F722"))
            .is_some()
    );
    assert!(
        message_errors
            .child("MF_AK5")
            .and_then(|node| node.child("F717"))
            .is_some()
    );
    assert!(
        group
            .child("ParserErrors_Group")
            .and_then(|node| node.child("MF_AK9"))
            .and_then(|node| node.child("F715"))
            .is_some()
    );
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert_source_boundary_roundtrip(&imported.project, directory.path());
}

#[test]
fn compiles_and_embeds_relative_idoc_configuration() {
    let directory = TempDir::new("idoc_config");
    std::fs::write(
        directory.path().join("parser.txt"),
        "BEGIN_SEGMENT_SECTION\nBEGIN_IDOC TEST\nBEGIN_SEGMENT HEADER0001\nSTATUS MANDATORY\nLOOPMAX 1\nBEGIN_FIELDS\nNAME DOCNO\nTYPE CHARACTER\nBYTE_FIRST 12\nBYTE_LAST 16\nEND_FIELDS\nEND_SEGMENT\nEND_IDOC\nEND_SEGMENT_SECTION\n",
    )
    .unwrap();
    let mfd_path = directory.path().join("mapping.mfd");
    std::fs::write(
        &mfd_path,
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1">
          <structure><children>
            <component name="idoc" library="text" uid="2" kind="16"><properties/><data>
              <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
                <entry name="HEADER0001"><entry name="DOCNO" outkey="10"/></entry>
              </entry></entry></entry></root>
              <text type="edi" kind="EDIFIXED" config="parser.txt" inputinstance="input.idoc"/>
            </data></component>
            <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data>
              <root><entry name="Outputs"><entry name="Value" inpkey="20"/></entry></root>
            </data></component>
          </children><graph directed="1"><vertices>
            <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
          </vertices></graph></structure>
        </component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mfd_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source.name, "IDOC");
    let layout = imported.project.source_options.idoc.as_ref().unwrap();
    assert_eq!(layout.segments().len(), 1);
    assert_eq!(layout.segments()[0].fields()[0].name(), "DOCNO");

    let encoded = serde_json::to_string(&imported.project).unwrap();
    let decoded: mapping::Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.source_options.idoc,
        imported.project.source_options.idoc
    );
    assert_source_boundary_roundtrip(&imported.project, directory.path());
}

#[test]
fn compiles_and_embeds_selected_swift_configuration() {
    let directory = TempDir::new("swift_config");
    let config = directory.path().join("SWIFT");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("Envelope.Config"),
        r#"<Config><Format standard="SWIFTMT"/><Include href="Common.Config"/><GenericRoot ref="Envelope"/></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config.join("Common.Config"),
        r#"<Config><GenericItems><Choice name="Mark" type="string"><Constant value="C"/><Constant value="D"/></Choice></GenericItems></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config.join("MT950.Config"),
        r#"<Config><Format standard="SWIFTMT"/><GenericItems>
          <SwiftField name="Reference" tag="20" format="16x"/>
          <Sequence name="MT950"><SwiftField ref="Reference" nodeName="20"/></Sequence>
        </GenericItems><Message><MessageType>MT950</MessageType><GenericRoot ref="MT950"/></Message></Config>"#,
    )
    .unwrap();
    let mfd_path = directory.path().join("mapping.mfd");
    std::fs::write(
        &mfd_path,
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1"><structure><children>
          <component name="swift" library="text" uid="2" kind="16"><properties/><data>
            <root><entry name="FileInstance"><entry name="document"><entry name="Messages"><entry name="Message"><entry name="MT950"><entry name="20" outkey="10"/></entry></entry></entry></entry></entry></root>
            <text type="edi" kind="SWIFTMT" config="SWIFT/Envelope.Config"><messages><message type="MT950"/></messages></text>
          </data></component>
          <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Outputs"><entry name="Value" inpkey="20"/></entry></root></data></component>
        </children><graph directed="1"><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mfd_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let layout = imported.project.source_options.swift_mt.as_ref().unwrap();
    assert_eq!(layout.message("MT950").unwrap().fields().len(), 1);
    let encoded = serde_json::to_string(&imported.project).unwrap();
    let decoded: mapping::Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.source_options.swift_mt,
        imported.project.source_options.swift_mt
    );
    assert_source_boundary_roundtrip(&imported.project, directory.path());
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("ferrule_mfd_edi_{label}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
