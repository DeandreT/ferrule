//! Actual MFD receiver contexts, with admitted executable controls before the
//! physical Count use is added. These are not generated/join Count sequences.
use super::*;

const INPUT: &str = "<Source><Cell><Label>keep</Label></Cell><Row><Text>keep</Text><Cell><Label>keep</Label></Cell></Row></Source>";
const ONE: &str = "<Results><Total>1</Total><Row><Count>1</Count><Cell><Label>keep</Label></Cell></Row></Results>";
const TWO: &str = "<Results><Total>1</Total><Row><Count>1</Count><Cell><Label>keep</Label></Cell><Cell><Label>keep</Label></Cell></Row></Results>";

fn import(package: &Package, strict: bool) -> Result<crate::Imported, crate::MfdError> {
    if strict {
        crate::import_with_profile(
            &package.0.join("map.mfd"),
            &crate::ImportOptions::default(),
            crate::ImportProfile::Executable,
        )
        .map(|o| o.imported)
    } else {
        package.import()
    }
}
fn child<'a>(scope: &'a mapping::Scope, name: &str) -> &'a mapping::Scope {
    scope
        .children
        .iter()
        .find(|s| s.target_field == name)
        .unwrap()
}
fn expected(project: &mapping::Project, output: &ir::Instance, literal: &str) {
    let xml = format_xml::to_string(&project.target, output).unwrap();
    assert_eq!(
        format_xml::from_str(&xml, &project.target).unwrap(),
        format_xml::from_str(literal, &project.target).unwrap(),
        "{xml}"
    );
}

fn configure(package: &Package, kind: &str) {
    if kind == "joined" {
        // The two join collections need distinct stored drivers. A root
        // Lookup also cannot be shadowed by the current Row's Cell collection.
        let path = package.0.join("row-cell-source.xsd");
        let schema = std::fs::read_to_string(&path).unwrap().replacen(
            r#"name="Cell""#,
            r#"name="Lookup""#,
            1,
        );
        std::fs::write(path, schema).unwrap();
    }
}

#[test]
fn generated_joined_concatenated_and_document_receivers_refuse_only_the_count_use() {
    const GENERATOR: &str = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="70"/></targets><data><constant value="0" datatype="decimal"/></data></component><component name="constant" library="core" kind="2"><targets><datapoint key="71"/></targets><data><constant value="1" datatype="decimal"/></data></component><component name="generate-sequence" library="core" kind="5"><sources><datapoint key="72"/><datapoint pos="1" key="73"/></sources><targets><datapoint key="74"/></targets></component>"#;
    const JOIN: &str = r#"<component name="join" library="core" kind="32"><data><root><entry name="document"><entry name="tuple" outkey="82"><entry name="dynamic_tree_node0"><entry name="Row" inpkey="80"/></entry><entry name="dynamic_tree_node1"><entry name="Cell" inpkey="81"/></entry></entry></entry></root><join><joinkeys><keypair><first-key path-id="101"/><second-key path-id="102"/></keypair></joinkeys><keypaths><entry><condition/><entry name="Text" outkey="101"><condition/></entry><entry name="Label" outkey="102"><condition/></entry></entry></keypaths></join></data></component>"#;
    let generated = DESIGN
        .replace("</children>", &format!("{GENERATOR}</children>"))
        .replace(
            r#"<edge from="12" to="120"/>"#,
            r#"<edge from="70" to="72"/><edge from="71" to="73"/><edge from="74" to="120"/>"#,
        )
        .replace(
            r#"<edge from="13" to="121"/>"#,
            r#"<edge from="20" to="121"/>"#,
        );
    let joined = DESIGN
        .replace("</children>", &format!("{JOIN}</children>"))
        .replace(
            r#"<entry name="Row" inpkey="80"/>"#,
            r#"<entry name="Cell" inpkey="80"/>"#,
        )
        .replace(
            r#"<entry name="Text" outkey="101">"#,
            r#"<entry name="Label" outkey="101">"#,
        )
        .replace(
            r#"<entry name="Cell" inpkey="81"/>"#,
            r#"<entry name="Lookup" inpkey="81"/>"#,
        )
        .replace(
            r#"name="join" library="core" kind="32""#,
            r#"name="join" library="core" uid="32" kind="32""#,
        )
        .replace(
            r#"<entry name="Cell"><entry name="Label"/>"#,
            r#"<entry name="Lookup" outkey="16"><entry name="Label" outkey="17"/>"#,
        )
        .replace(
            r#"<edge from="12" to="120"/>"#,
            r#"<edge from="12" to="80"/><edge from="16" to="81"/><edge from="82" to="120"/>"#,
        )
        .replace(
            r#"<edge from="13" to="121"/>"#,
            r#"<edge from="20" to="121"/>"#,
        );
    let concatenated = DESIGN.replace(r#"<entry name="Cell" inpkey="120"><entry name="Label" inpkey="121"/><entry name="Count" inpkey="122"/></entry>"#, r#"<entry name="Cell" inpkey="120"><entry name="Label" inpkey="121"/><entry name="Count" inpkey="122"/></entry><entry name="Cell" inpkey="130"><entry name="Label" inpkey="131"/><entry name="Count" inpkey="132"/></entry>"#)
        .replace("</connections>", r#"<edge from="12" to="130"/><edge from="13" to="131"/><edge from="51" to="132"/></connections>"#);
    let document = DESIGN.replace(r#"<root><entry name="Results">"#, r#"<root><entry name="FileInstance" inpkey="210"><file role="outputinstance" name="fallback.xml"/><entry name="document"><entry name="Results">"#)
        .replace(r#"</entry></root>
    <document schema="row-cell-target.xsd""#, r#"</entry></entry></entry></root>
    <document schema="row-cell-target.xsd""#)
        .replace("</connections>", r#"<edge from="11" to="210"/></connections>"#);

    for (kind, design, literal) in [
        ("generated", generated, TWO),
        ("joined", joined, ONE),
        ("concatenated", concatenated, TWO),
        (
            "document",
            document,
            "<Results><Row><Cell><Label>keep</Label></Cell></Row></Results>",
        ),
    ] {
        for reverse in [false, true] {
            let mut control = design
                .replace(r#"<edge from="51" to="122"/>"#, "")
                .replace(r#"<edge from="51" to="132"/>"#, "");
            if kind == "document" {
                control = control
                    .replace(r#"<edge from="51" to="100"/>"#, "")
                    .replace(r#"<edge from="51" to="111"/>"#, "");
            }
            let control = if reverse {
                super::review_regressions::reverse_sections(&control)
            } else {
                control
            };
            let package = Package::new(&control);
            configure(&package, kind);
            for strict in [false, true] {
                let imported = import(&package, strict).unwrap_or_else(|error| {
                    panic!("{kind} control reverse={reverse} strict={strict}: {error}")
                });
                assert!(
                    imported.warnings.is_empty(),
                    "{kind}: {:?}",
                    imported.warnings
                );
                let project = &imported.project;
                if kind != "document" {
                    assert!(
                        matches!(&child(&project.root, "Row").iteration, mapping::ScopeIteration::Source(path) if path == &["Row"])
                    );
                }
                let text = if kind == "joined" {
                    INPUT
                        .replacen("<Cell>", "<Lookup>", 1)
                        .replacen("</Cell>", "</Lookup>", 1)
                } else {
                    INPUT.into()
                };
                let input = format_xml::from_str(&text, &project.source).unwrap();
                let output = engine::run(project, &input).unwrap();
                eprintln!("executed receiver control {kind} reverse={reverse} strict={strict}");
                match kind {
                    "generated" => {
                        assert!(matches!(
                            child(child(&project.root, "Row"), "Cell").iteration,
                            mapping::ScopeIteration::Sequence(_)
                        ));
                        expected(project, &output, literal);
                    }
                    "joined" => {
                        assert!(matches!(
                            child(child(&project.root, "Row"), "Cell").iteration,
                            mapping::ScopeIteration::InnerJoin { .. }
                        ));
                        expected(project, &output, literal);
                    }
                    "concatenated" => {
                        assert!(
                            child(child(&project.root, "Row"), "Cell")
                                .concatenated()
                                .is_some()
                        );
                        expected(project, &output, literal);
                    }
                    "document" => {
                        assert!(matches!(
                            project.root.iteration,
                            mapping::ScopeIteration::DynamicDocuments { .. }
                        ));
                        let ir::Instance::DocumentSet(documents) = output else {
                            panic!("document receiver did not execute documents");
                        };
                        assert_eq!(documents.len(), 1);
                        assert_eq!(documents[0].path(), "keep");
                        expected(project, documents[0].value(), literal);
                    }
                    _ => unreachable!(),
                }
            }
            let unsafe_design = if reverse {
                super::review_regressions::reverse_sections(&design)
            } else {
                design.clone()
            };
            let package = Package::new(&unsafe_design);
            configure(&package, kind);
            for strict in [false, true] {
                refused(import(&package, strict));
            }
        }
    }
}
