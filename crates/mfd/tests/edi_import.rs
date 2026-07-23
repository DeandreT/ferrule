use std::path::{Path, PathBuf};

use mapping::{EdiBoundaryKind, EdiImpliedDecimal, Node, X12Separators};

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
    if let Some(path) = project.source_path.as_deref() {
        assert!(
            exported.contains(&format!("<file role=\"inputinstance\" name=\"{path}\"/>")),
            "{exported}"
        );
    }
    assert_edi_text_has_no_instance_attributes(&exported);

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.source_options, project.source_options);
    assert_eq!(reimported.project.source_path, project.source_path);
}

fn assert_edi_text_has_no_instance_attributes(text: &str) {
    let document = roxmltree::Document::parse(text).unwrap();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"))
    {
        assert!(node.attribute("inputinstance").is_none());
        assert!(node.attribute("outputinstance").is_none());
    }
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
fn retains_nested_edifact_output_instance_through_export() {
    let directory = TempDir::new("nested_edifact_output");
    let design = directory.path().join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="text" kind="16"><data>
            <root><entry name="FileInstance"><file role="inputinstance" name="orders.x12"/><entry name="document">
              <entry name="Envelope" ferrule-repeating="0"><entry name="Message" ferrule-repeating="1">
                <entry name="Code" ferrule-repeating="0" datatype="string" outkey="10"/>
              </entry></entry>
            </entry></entry></root>
            <text type="edi" kind="EDIX12"/>
          </data></component>
          <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="FileInstance"><file role="outputinstance" name="result.edi"/><entry name="document">
              <entry name="Envelope" ferrule-repeating="0"><entry name="Message" ferrule-repeating="1">
                <entry name="Code" ferrule-repeating="0" datatype="string" inpkey="20"/>
              </entry></entry>
            </entry></entry></root>
            <text type="edi" kind="EDIFACT"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let mut imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source_path.as_deref(), Some("orders.x12"));
    assert_eq!(imported.project.target_path.as_deref(), Some("result.edi"));
    assert_eq!(
        imported.project.target_options.edi_kind,
        Some(EdiBoundaryKind::Edifact)
    );
    imported.project.target_options.edi_implied_decimals = vec![
        EdiImpliedDecimal::new(vec!["Envelope".into(), "Message".into(), "Code".into()], 2)
            .unwrap(),
    ];

    let roundtrip = directory.path().join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&roundtrip).unwrap();
    assert!(exported.contains("<file role=\"inputinstance\" name=\"orders.x12\"/>"));
    assert!(exported.contains("<file role=\"outputinstance\" name=\"result.edi\"/>"));
    assert_edi_text_has_no_instance_attributes(&exported);

    let reimported = mfd::import(&roundtrip).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source_path, imported.project.source_path);
    assert_eq!(reimported.project.target_path, imported.project.target_path);
    assert_eq!(
        reimported.project.target_options,
        imported.project.target_options
    );
}

#[test]
fn cloned_edi_target_segments_keep_branch_local_bindings() {
    let directory = TempDir::new("cloned_target_branches");
    let x12 = directory.path().join("X12");
    std::fs::create_dir_all(&x12).unwrap();
    std::fs::write(
        x12.join("Defs.Segment"),
        r#"<Config><Elements>
          <Data name="Text" type="string"/>
          <Segment name="ISA"><Data ref="Text"/></Segment>
          <Segment name="GS"><Data ref="Text"/></Segment>
          <Segment name="ST"><Data ref="Text"/></Segment>
          <Segment name="BAL"><Data ref="Text"/></Segment>
          <Segment name="SE"><Data ref="Text"/></Segment>
          <Segment name="GE"><Data ref="Text"/></Segment>
          <Segment name="IEA"><Data ref="Text"/></Segment>
        </Elements></Config>"#,
    )
    .unwrap();
    std::fs::write(
        x12.join("Envelope.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
            <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
              <Segment ref="GS"/><Select field="ST/Text"/><Segment ref="GE" minOccurs="0"/>
            </Group><Segment ref="IEA" minOccurs="0"/>
          </Group></Group></Config>"#,
    )
    .unwrap();
    std::fs::write(
        x12.join("850.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
          <Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded">
            <Segment ref="ST"/><Segment ref="BAL" maxOccurs="unbounded"/><Segment ref="SE"/>
          </Group></Message></Config>"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Debit" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element>
            <xs:element name="Credit" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let design = directory.path().join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data>
            <root><entry name="Source">
              <entry name="Debit" outkey="10"><entry name="Text" outkey="11"/></entry>
              <entry name="Credit" outkey="20"><entry name="Text" outkey="21"/></entry>
            </entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
          </data></component>
          <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Interchange"><entry name="Group"><entry name="Message">
              <entry name="BAL" inpkey="30"><entry name="Text" inpkey="31"/></entry>
              <entry name="BAL" clone="1" inpkey="40"><entry name="Text" inpkey="41"/></entry>
            </entry></entry></entry></entry></entry></entry></root>
            <text type="edi" kind="EDIX12" config="X12/850.Config"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
          <vertex vertexkey="20"><edges><edge vertexkey="40"/></edges></vertex>
          <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");
    let balance = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Interchange")
        .and_then(|scope| {
            scope
                .children
                .iter()
                .find(|scope| scope.target_field == "Group")
        })
        .and_then(|scope| {
            scope
                .children
                .iter()
                .find(|scope| scope.target_field == "Message")
        })
        .and_then(|scope| {
            scope
                .children
                .iter()
                .find(|scope| scope.target_field == "BAL")
        })
        .expect("cloned BAL target scope is imported");
    let branches = balance
        .concatenated()
        .expect("cloned BAL branches are concatenated")
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].source(), Some(["Debit".to_string()].as_slice()));
    assert_eq!(
        branches[1].source(),
        Some(["Credit".to_string()].as_slice())
    );
    assert_eq!(branches[0].bindings.len(), 1);
    assert_eq!(branches[1].bindings.len(), 1);

    let roundtrip_path = directory.path().join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtrip = mfd::import(&roundtrip_path).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    let validation = engine::validate(&roundtrip.project);
    assert!(validation.is_empty(), "{validation:?}");
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
          <Data name="FDate" type="date" minLength="8" maxLength="8"/>
          <Segment name="ISA"><Data ref="F1"/></Segment>
          <Segment name="GS"><Data ref="F1"/></Segment>
          <Segment name="ST"><Data ref="F1"/><Data ref="FDate"/></Segment>
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
              <root><entry name="FileInstance"><file role="inputinstance" name="input.x12"/><entry name="document"><entry name="Envelope">
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
              <text type="edi" kind="EDIX12" config="X12\850.Config">
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
    assert_eq!(imported.project.source_path.as_deref(), Some("input.x12"));
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
            .source_options
            .edi_lexical_formats
            .iter()
            .any(|format| {
                format.kind() == mapping::EdiLexicalKind::CompactDate8
                    && format.path().ends_with(&["ST".into(), "FDate".into()])
            })
    );
    assert!(
        imported
            .project
            .source_options
            .edi_value_constraints
            .iter()
            .any(|constraint| {
                constraint.min_chars() == 8
                    && constraint.max_chars() == 8
                    && constraint.path().ends_with(&["ST".into(), "FDate".into()])
            })
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
            <root><entry name="FileInstance"><file role="inputinstance" name="input.mt950"/><entry name="document"><entry name="Messages"><entry name="Message"><entry name="MT950"><entry name="20" outkey="10"/></entry></entry></entry></entry></entry></root>
            <text type="edi" kind="SWIFTMT" config="SWIFT/Envelope.Config"><messages><message type="MT950"/></messages></text>
          </data></component>
          <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Outputs"><entry name="Value" inpkey="20"/></entry></root></data></component>
        </children><graph directed="1"><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mfd_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source_path.as_deref(), Some("input.mt950"));
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

#[test]
fn compiles_and_executes_tradacoms_implied_decimals() {
    let directory = TempDir::new("tradacoms_implicit_decimal");
    let config = directory.path().join("TRADACOMS");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("Defs.Segment"),
        r#"<Config><Elements>
          <Data name="Code" type="string"/>
          <Data name="Amount" type="decimal" implicitDecimals="3"/>
          <Segment name="STX"><Data ref="Code"/></Segment>
          <Segment name="MHD"><Data ref="Code"/></Segment>
          <Segment name="LIN"><Data ref="Amount"/></Segment>
          <Segment name="END"><Data ref="Code"/></Segment>
        </Elements></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config.join("Envelope.Config"),
        r#"<Config><Format standard="TRADACOMS"/><Include href="Defs.Segment"/>
          <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
            <Segment ref="STX"/><Select field="MHD/Code"/><Segment ref="END"/>
          </Group></Group></Config>"#,
    )
    .unwrap();
    std::fs::write(
        config.join("Invoice.Config"),
        r#"<Config><Format standard="TRADACOMS"/><Include href="Defs.Segment"/>
          <Message><MessageType>INVOICE</MessageType><Group name="Invoice" maxOccurs="unbounded">
            <Segment ref="MHD"><Condition path="Code" value="INVOICE"/></Segment>
            <Segment ref="LIN"/>
          </Group></Message></Config>"#,
    )
    .unwrap();
    let input = directory.path().join("input.edi");
    std::fs::write(&input, "STX=START'MHD=INVOICE'LIN=12345'END=1'").unwrap();
    let mfd_path = directory.path().join("mapping.mfd");
    std::fs::write(
        &mfd_path,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="text" kind="16"><data>
            <root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Interchange"><entry name="Message"><entry name="LIN"><entry name="Amount" outkey="10"/></entry></entry></entry></entry></entry></entry></root>
            <text type="edi" kind="EDITRADACOMS" inputinstance="input.edi" config="TRADACOMS/Invoice.Config"/>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="Result"><entry name="Amount" inpkey="20"/></entry></root>
            <document outputinstance="result.xml" instanceroot="{}Result"/>
          </data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&mfd_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_options.edi_implied_decimals.len(),
        1
    );
    let mut source = format_edi::tradacoms::read(&input, &imported.project.source, false).unwrap();
    format_edi::apply_implied_decimals(
        &mut source,
        &imported.project.source_options.edi_implied_decimals,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        output.field("Amount").and_then(ir::Instance::as_scalar),
        Some(&ir::Value::Float(12.345))
    );
    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(xml.contains("<Amount>12.345</Amount>"));
    assert_source_boundary_roundtrip(&imported.project, directory.path());
}

#[test]
fn malformed_embedded_implied_decimals_warn_and_disable_scaling() {
    let directory = TempDir::new("invalid_implied_decimal_metadata");
    let design = directory.path().join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="text" kind="16"><data>
            <root><entry name="Envelope"><entry name="Amount" datatype="decimal" outkey="10"/></entry></root>
            <text type="edi" kind="EDITRADACOMS">
              <ferrule-implied-decimals>[{"path":[],"places":0}]</ferrule-implied-decimals>
              <ferrule-value-constraints>[{"path":[],"min_chars":3,"max_chars":2}]</ferrule-value-constraints>
            </text>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="Result"><entry name="Amount" inpkey="20"/></entry></root>
          </data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid embedded implied-decimal metadata")),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported
            .project
            .source_options
            .edi_implied_decimals
            .is_empty()
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid embedded value-constraint metadata")),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported
            .project
            .source_options
            .edi_value_constraints
            .is_empty()
    );
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
