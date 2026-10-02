use mfd::{ImportOptions, ImportProfile, MfdError};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory(PathBuf);
impl Directory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-root-view-direction-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        std::fs::write(path.join("schema.xsd"), SCHEMA)?;
        Ok(Self(path))
    }
    fn write(&self, text: &str) -> std::io::Result<PathBuf> {
        let path = self.0.join("mapping.mfd");
        std::fs::write(&path, text)?;
        Ok(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:attribute name="Code" type="xs:string"/></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base"><xs:attribute name="Extra" type="xs:string"/></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#;
const CONDITION: &str = r#"<condition><expression><function name="equal" library="core"><expression><attribute ns="http://www.w3.org/2001/XMLSchema-instance" name="type"/></expression><expression><constant datatype="QName" value="{}Derived"/></expression></function></expression></condition>"#;
const FEEDS: &str = r#"<vertex vertexkey="2"><edges><edge vertexkey="5"/></edges></vertex><vertex vertexkey="3"><edges><edge vertexkey="6"/></edges></vertex>"#;
fn views(source: bool, first_ports: bool, conditioned: bool) -> String {
    let kind = if source { "outkey" } else { "inpkey" };
    let (code, extra) = if source { (2, 3) } else { (5, 6) };
    let first = if first_ports {
        format!(r#"{kind}="{}""#, code + 20)
    } else {
        String::new()
    };
    format!(
        r#"<entry name="Root"><entry name="Code" type="attribute" {first}/></entry><entry name="Root">{}<entry name="Code" type="attribute" {kind}="{code}"/><entry name="Extra" type="attribute" {kind}="{extra}"/></entry>"#,
        if conditioned { CONDITION } else { "" }
    )
}
fn mapping(source: &str, target: &str, feeds: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="main" uid="1"><structure><children>
<component name="same" library="xml" kind="14" uid="2"><data><document schema="schema.xsd" inputinstance="input.xml" instanceroot="{{}}Root"/><root><header><namespaces><namespace/></namespaces></header><entry name="FileInstance" ns="0"><entry name="document" ns="0">{source}</entry></entry></root></data></component>
<component name="same" library="xml" kind="14" uid="3"><properties XSLTDefaultOutput="1"/><data><document schema="schema.xsd" outputinstance="output.xml" instanceroot="{{}}Root"/><root><header><namespaces><namespace/></namespaces></header><entry name="FileInstance" ns="0"><entry name="document" ns="0">{target}</entry></entry></root></data></component>
</children><graph directed="1"><vertices>{feeds}</vertices></graph></structure></component></mapping>"#
    )
}
fn standard() -> String {
    mapping(&views(true, false, true), &views(false, false, true), FEEDS)
}
fn assert_unsupported(text: &str) -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    let path = directory.write(text)?;
    let before = std::fs::read(&path)?;
    for profile in [ImportProfile::BestEffort, ImportProfile::Executable] {
        let Err(MfdError::UnsupportedImport(message)) =
            mfd::import_with_profile(&path, &ImportOptions::default(), profile)
        else {
            return Err("expected typed root-view unsupported diagnostic without a project".into());
        };
        assert!(message.contains("no project is published"));
        assert!(message.contains("zero-port first-entry projection"));
        assert!(!message.contains("Best-effort projection is retained"));
        for (uid, role, pins) in [
            (2, "source", ["outkey=2", "outkey=3"]),
            (3, "target", ["inpkey=5", "inpkey=6"]),
        ] {
            let detail = message
                .split(" | ")
                .find(|detail| detail.contains(&format!("UID {uid}:")))
                .ok_or("missing owner-scoped evidence")?;
            assert!(detail.contains(&format!("role {role}")));
            for pin in pins {
                assert!(detail.contains(pin));
            }
            assert!(detail.contains("view `{}Root` (declared `{}Root`"));
            assert!(detail.contains("default `Base`"));
            if text.contains(CONDITION) {
                assert!(detail.contains("type condition `{}Derived`"));
            }
        }
        assert!(message.contains("view `{}Root` (declared `{}Root`"));
        assert!(message.contains("default `Base`"));
        assert!(message.contains("unproved XML document-root view"));
        assert!(
            message.contains("branch ancestry/condition is unrepresented")
                || message.contains("nonunique raw port owner")
        );
    }
    assert!(matches!(
        mfd::import(&path),
        Err(MfdError::UnsupportedImport(_))
    ));
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}
fn assert_no_repair(text: &str) -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    let path = directory.write(text)?;
    assert!(mfd::import(&path).is_err(), "unexpected repair for {text}");
    assert!(
        mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)
            .is_err()
    );
    Ok(())
}
#[test]
fn connected_second_root_reports_both_roles_and_loss_without_publishing_a_project()
-> Result<(), Box<dyn Error>> {
    assert_unsupported(&standard())
}
#[test]
fn unconditioned_sibling_ownership_also_returns_a_loss_diagnostic() -> Result<(), Box<dyn Error>> {
    assert_unsupported(&mapping(
        &views(true, false, false),
        &views(false, false, false),
        FEEDS,
    ))
}
#[test]
fn one_applied_zero_port_role_collects_both_source_and_target_loss_findings()
-> Result<(), Box<dyn Error>> {
    for (source_first, target_first) in [(false, true), (true, false)] {
        assert_unsupported(&mapping(
            &views(true, source_first, true),
            &views(false, target_first, true),
            FEEDS,
        ))?;
    }
    Ok(())
}
#[test]
fn component_order_does_not_truncate_source_or_target_evidence() -> Result<(), Box<dyn Error>> {
    let text = standard();
    let (prefix, body) = text
        .split_once("<children>\n")
        .ok_or("missing authored children")?;
    let (children, suffix) = body
        .split_once("\n</children>")
        .ok_or("missing authored children end")?;
    assert_unsupported(&format!(
        "{prefix}<children>\n{}\n</children>{suffix}",
        children.lines().rev().collect::<Vec<_>>().join("\n")
    ))
}
#[test]
fn ambiguous_graph_shape_and_document_metadata_do_not_recover_direction()
-> Result<(), Box<dyn Error>> {
    for text in [
        standard().replace("directed=\"1\"", "directed=\"0\""),
        standard().replace(" directed=\"1\"", ""),
        standard().replace("</vertices>", "</vertices><vertices/>"),
        standard().replace(
            "</graph>",
            "</graph><graph directed=\"1\"><vertices/></graph>",
        ),
        standard().replace("</edges>", "</edges><edges/>"),
        standard().replace(
            "<root><header>",
            "<document outputinstance=\"conflict.xml\"/><root><header>",
        ),
    ] {
        assert_no_repair(&text)?;
        let directory = Directory::new()?;
        let path = directory.write(&text)?;
        assert!(
            !matches!(mfd::import(&path), Err(MfdError::UnsupportedImport(message)) if message.contains("zero-port first-entry projection"))
        );
    }
    Ok(())
}
#[test]
fn empty_vertices_and_unconnected_dormant_pins_do_not_vote_on_direction()
-> Result<(), Box<dyn Error>> {
    let text = standard()
        .replace(
            "</vertices>",
            r#"<vertex vertexkey="700"/><vertex vertexkey="bad"><edges/></vertex></vertices>"#,
        )
        .replacen(
            "</data></component>",
            r#"<entry inpkey="700"/></data></component>"#,
            1,
        );
    assert_unsupported(&text)
}
#[test]
fn disconnected_views_and_missing_or_ambiguous_document_roles_do_not_repair()
-> Result<(), Box<dyn Error>> {
    for text in [
        mapping(&views(true, false, true), &views(false, false, true), ""),
        standard().replace(" inputinstance=\"input.xml\"", ""),
        standard().replace(" outputinstance=\"output.xml\"", ""),
        standard().replace(
            "inputinstance=\"input.xml\"",
            "inputinstance=\"input.xml\" outputinstance=\"both.xml\"",
        ),
        standard().replace(
            "outputinstance=\"output.xml\"",
            "inputinstance=\"input.xml\"",
        ),
    ] {
        assert_no_repair(&text)?;
    }
    Ok(())
}
#[test]
fn duplicate_and_malformed_uid_or_pin_owners_do_not_repair() -> Result<(), Box<dyn Error>> {
    for text in [
        standard().replace("uid=\"3\"", "uid=\"2\""),
        standard().replace("uid=\"3\"", "uid=\"02\""),
        standard().replace("uid=\"3\"", "uid=\"bad\""),
        standard().replace(" uid=\"3\"", ""),
        standard().replace("outkey=\"3\"", "outkey=\"2\""),
        standard().replace("outkey=\"3\"", "outkey=\"02\""),
        standard().replace("inpkey=\"6\"", "outkey=\"2\""),
        standard().replace("outkey=\"3\"", "outkey=\"bad\""),
        standard().replace("inpkey=\"6\"", "inpkey=\"bad\""),
    ] {
        assert_no_repair(&text)?;
    }
    Ok(())
}
#[test]
fn dangling_malformed_reversed_and_duplicate_feeds_do_not_repair() -> Result<(), Box<dyn Error>> {
    for feeds in [FEEDS.replace("vertexkey=\"6\"", "vertexkey=\"999\""), FEEDS.replace("vertexkey=\"2\"", "vertexkey=\"bad\""), FEEDS.replace("vertexkey=\"6\"", "vertexkey=\"bad\""), r#"<vertex vertexkey="5"><edges><edge vertexkey="2"/></edges></vertex><vertex vertexkey="6"><edges><edge vertexkey="3"/></edges></vertex>"#.into(), format!("{FEEDS}{FEEDS}"), FEEDS.replace("vertexkey=\"6\"", "vertexkey=\"5\"")] { assert_no_repair(&mapping(&views(true, false, true), &views(false, false, true), &feeds))?; }
    Ok(())
}
#[test]
fn variable_and_pass_through_endpoints_do_not_change_direction() -> Result<(), Box<dyn Error>> {
    for text in [
        standard().replacen(
            "<data><document",
            "<data><parameter usageKind=\"variable\"/><document",
            1,
        ),
        standard().replace(
            "uid=\"2\"><data>",
            "uid=\"2\"><properties PassThrough=\"1\"/><data>",
        ),
    ] {
        let directory = Directory::new()?;
        let path = directory.write(&text)?;
        if let Ok(best) = mfd::import(&path) {
            assert!(
                !best
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("diagnostic direction is retained"))
            );
        }
        assert!(
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)
                .is_err()
        );
    }
    Ok(())
}
#[test]
fn ordinary_first_entry_role_remains_counted_without_diagnostic_override()
-> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    let path = directory.write(&mapping(
        &views(true, true, true),
        &views(false, true, true),
        FEEDS,
    ))?;
    let best = mfd::import(&path)?;
    assert!(
        !best
            .warnings
            .iter()
            .any(|w| w.contains("diagnostic direction is retained"))
    );
    assert!(
        mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)
            .is_err()
    );
    Ok(())
}
#[test]
fn multiple_protocol_wrappers_retain_second_view_ownership() -> Result<(), Box<dyn Error>> {
    let text = standard().replace("</entry><entry name=\"Root\"><condition>", "</entry></entry></entry><entry name=\"FileInstance\" ns=\"0\"><entry name=\"document\" ns=\"0\"><entry name=\"Root\"><condition>");
    assert_unsupported(&text)
}
#[test]
fn nested_structure_pin_and_uid_reuse_does_not_own_outer_graph_feeds() -> Result<(), Box<dyn Error>>
{
    let nested = r#"<structure><children><component uid="2"><data><entry inpkey="2"/><entry outkey="5"/></data></component></children><graph><vertices><vertex vertexkey="5"><edges><edge vertexkey="2"/></edges></vertex></vertices></graph></structure>"#;
    assert_unsupported(&standard().replacen(
        "</data></component>",
        &format!("{nested}</data></component>"),
        1,
    ))
}
