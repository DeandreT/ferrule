use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-document-filenames-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        std::fs::write(
            path.join("document.xsd"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Document"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;
        std::fs::write(
            path.join("document.json"),
            r#"{"type":"object","properties":{"Value":{"type":"string"}},"required":["Value"]}"#,
        )?;
        std::fs::write(
            path.join("message.Config"),
            r#"<Config><Format standard="X12"/><Elements><Data name="Value" type="string"/><Segment name="ABC"><Data ref="Value"/></Segment></Elements><Message><MessageType>777</MessageType><Group name="Message_777"><Segment ref="ABC"/></Group></Message></Config>"#,
        )?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn design(format: &str, edges: &str) -> String {
    let component = |name: &str, source: bool| {
        let (library, kind, content, settings) = match format {
            "JSON" => (
                "json",
                "31",
                format!(
                    r#"<entry name="root"><entry name="Value" {}="{}"/></entry>"#,
                    if source { "outkey" } else { "inpkey" },
                    if source { 11 } else { 21 }
                ),
                r#"<json schema="document.json"/>"#,
            ),
            "EDI" => (
                "text",
                "16",
                format!(
                    r#"<entry name="Message_777"><entry name="ABC"><entry name="Value" {}="{}"/></entry></entry>"#,
                    if source { "outkey" } else { "inpkey" },
                    if source { 11 } else { 21 }
                ),
                r#"<text type="edi" kind="EDIX12" config="message.Config"/>"#,
            ),
            _ => (
                "xml",
                "14",
                format!(
                    r#"<entry name="Document"><entry name="Value" {}="{}"/></entry>"#,
                    if source { "outkey" } else { "inpkey" },
                    if source { 11 } else { 21 }
                ),
                r#"<document schema="document.xsd" instanceroot="{}Document"/>"#,
            ),
        };
        let settings = settings.replace(
            "/>",
            &format!(
                " {}=\"preview.dat\"/>",
                if source {
                    "inputinstance"
                } else {
                    "outputinstance"
                }
            ),
        );
        format!(
            r#"<component name="{name}" library="{library}" kind="{kind}">{}<data><root><entry name="FileInstance" inpkey="{}" outkey="{}"><file role="{}" name="preview.dat"/><entry name="document">{content}</entry></entry></root>{settings}</data></component>"#,
            if source {
                ""
            } else {
                r#"<properties XSLTDefaultOutput="1"/>"#
            },
            if source { 13 } else { 23 },
            if source { 14 } else { 24 },
            if source {
                "inputinstance"
            } else {
                "outputinstance"
            }
        )
    };
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
      <component name="path" library="core" kind="6"><targets><datapoint key="1"/></targets><data><input datatype="string" required="1"/><parameter usageKind="input" name="FilePath"/></data></component>
      {}{}
      </children></structure><connections><edge from="11" to="21"/>{edges}</connections></component></mapping>"#,
        component("source", true),
        component("target", false)
    )
}

#[test]
fn json_and_edi_wrapper_connections_are_explicitly_partial() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    for format in ["JSON", "EDI"] {
        for (name, direction, port, edge) in [
            ("source", "input", 13, r#"<edge from="1" to="13"/>"#),
            ("target", "input", 23, r#"<edge from="1" to="23"/>"#),
            ("source", "output", 14, r#"<edge from="14" to="21"/>"#),
            ("target", "output", 24, r#"<edge from="24" to="21"/>"#),
        ] {
            let path = directory.0.join(format!("{format}-{port}.mfd"));
            let mut text = design(format, edge);
            if direction == "output" {
                text = text.replace(r#"<edge from="11" to="21"/>"#, "");
            }
            std::fs::write(&path, &text)?;
            let expected = format!(
                "{format} component `{name}` has connected FileInstance {direction} port `{port}`; filename-driven documents are unsupported"
            );
            let imported = mfd::import(&path)?;
            assert!(
                imported.warnings.contains(&expected),
                "{:?}",
                imported.warnings
            );
            if direction == "input" {
                assert!(engine::validate(&imported.project).is_empty());
            }
            let error = mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .err()
            .ok_or("filename connection admitted")?;
            assert!(error.to_string().contains(&expected), "{error}");
            assert_eq!(std::fs::read_to_string(path)?, text);
        }
    }
    Ok(())
}

#[test]
fn unconnected_wrappers_and_same_named_payload_fields_are_executable() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    for format in ["JSON", "EDI", "XML"] {
        let path = directory.0.join(format!("{format}.mfd"));
        std::fs::write(&path, design(format, ""))?;
        let result = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        assert_eq!(
            result.imported.project.source_path.as_deref(),
            Some("preview.dat")
        );
        assert_eq!(
            result.imported.project.target_path.as_deref(),
            Some("preview.dat")
        );
        assert_eq!(result.imported.project.graph.nodes.len(), 1);
    }
    let path = directory.0.join("payload-name.mfd");
    std::fs::write(
        directory.0.join("document.json"),
        r#"{"type":"object","properties":{"FileInstance":{"type":"string"}},"required":["FileInstance"]}"#,
    )?;
    std::fs::write(
        &path,
        design("JSON", "").replace("name=\"Value\"", "name=\"FileInstance\""),
    )?;
    mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    Ok(())
}

#[test]
fn xml_runtime_paths_cannot_fall_back_to_preview_paths() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    for (port, expected) in [
        (
            13,
            "primary source `source` has connected FileInstance input port `13`; runtime filename acquisition is unsupported",
        ),
        (
            23,
            "target `target` has a connected FileInstance path without a supported document driver; dynamic document output was skipped",
        ),
    ] {
        let path = directory.0.join(format!("xml-{port}.mfd"));
        let text = design("XML", &format!(r#"<edge from="1" to="{port}"/>"#));
        std::fs::write(&path, &text)?;
        let imported = mfd::import(&path)?;
        assert!(
            imported.warnings.iter().any(|w| w == expected),
            "{:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty());
        let error = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .err()
        .ok_or("runtime path lost without a refusal")?;
        assert!(error.to_string().contains(expected));
        assert_eq!(std::fs::read_to_string(path)?, text);
    }
    Ok(())
}
