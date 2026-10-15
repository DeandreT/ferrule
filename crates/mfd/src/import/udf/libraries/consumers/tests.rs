//! Isolated generic raw-shape controls, authored only. No engine/API execution
//! was admitted in v3. Each primary negative changes just one role selector.
use super::*;

const TARGET: &str = r#"<component library="xml" kind="14" name="Target"><properties PassThrough="0"/><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/><namespace uid="urn:payload"/></namespaces></header><entry name="Result"><entry name="Value" inpkey="20"/></entry></root><document schema="generic.xsd" instanceroot="{}Result"/></data></component>"#;
const CALL: &str = r#"<component library="helpers" kind="19" name="Leaf"><data><root rootindex="1"><entry name="result" componentid="2" outkey="10"/></root></data></component>"#;

fn mapping(target: &str, extra: &str, edges: &str) -> String {
    format!(
        r#"<mapping><component name="main"><structure><children>{CALL}{target}{extra}</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>{edges}</vertices></graph></structure></component></mapping>"#
    )
}

fn check(xml: &str, expected: Result<(), Refusal>) {
    let document = roxmltree::Document::parse(xml).unwrap();
    let structure = document
        .descendants()
        .find(|n| n.has_tag_name("structure"))
        .unwrap();
    let owner = structure.parent().unwrap();
    let children = structure
        .children()
        .find(|n| n.has_tag_name("children"))
        .unwrap();
    let call = children
        .children()
        .find(|n| n.attribute("kind") == Some("19"))
        .unwrap();
    let feeds = crate::import::graph::read_edges(&structure, Some(&owner));
    assert_eq!(validate(&call, children, &feeds), expected, "{xml}");
    let complete_gate =
        super::super::validate_call(&call, &super::super::limits::ConstructionBudget::default());
    match expected {
        Ok(()) => complete_gate.unwrap(),
        Err(reason) => assert_eq!(
            complete_gate.unwrap_err(),
            format!("imported call consumption refused: {}", reason.message())
        ),
    }
}

#[test]
fn ordinary_scalar_target_is_a_positive_control_for_both_role_mutations() {
    check(&mapping(TARGET, "", ""), Ok(()));
    let doc = roxmltree::Document::parse(TARGET).unwrap();
    let root = doc.descendants().find(|n| n.has_tag_name("root")).unwrap();
    assert_eq!(xml_file_instance_input(root), None);
    assert_eq!(
        xml_content_entry(root).unwrap().attribute("name"),
        Some("Result")
    );
}

#[test]
fn payload_fileinstance_is_not_ordinary_even_with_namespace_and_position_aliases() {
    for attributes in [
        "",
        r#"ns="0" pos="0""#,
        r#"ns="1" pos="64""#,
        r#"ns="2" pos="7""#,
        r#"ns="bogus" pos="bogus""#,
    ] {
        // The baseline retains the same namespace/position/key/graph geometry.
        // Only the leaf name changes; PassThrough is explicitly0 in both.
        let ordinary = TARGET.replace(
            r#"name="Value" inpkey="20""#,
            &format!(r#"name="Value" inpkey="20" {attributes}"#),
        );
        check(&mapping(&ordinary, "", ""), Ok(()));
        let collision = ordinary.replace(r#"name="Value""#, r#"name="FileInstance""#);
        let document = roxmltree::Document::parse(&collision).unwrap();
        let root = document
            .descendants()
            .find(|n| n.has_tag_name("root"))
            .unwrap();
        // Exact downstream filename-key selection, no namespace/pos filtering.
        assert_eq!(xml_file_instance_input(root), Some(20));
        let entry = root
            .descendants()
            .find(|n| n.attribute("inpkey") == Some("20"))
            .unwrap();
        assert!(
            super::super::ordinary_content_entry(entry),
            "isolate the v2 raw-geometry bypass"
        );
        check(&mapping(&collision, "", ""), Err(Refusal::RuntimeFilename));
    }
}

#[test]
fn legacy_name_and_numeric_port_aliases_cannot_disguise_a_filename_collision() {
    for name in [
        "0:FileInstance",
        "12:FileInstance",
        "@FileInstance",
        "02:@FileInstance",
        "File&#73;nstance",
    ] {
        let ordinary = TARGET.replace(r#"inpkey="20""#, r#"inpkey="00020""#);
        check(&mapping(&ordinary, "", ""), Ok(()));
        let collision = ordinary.replace(r#"name="Value""#, &format!(r#"name="{name}""#));
        check(&mapping(&collision, "", ""), Err(Refusal::RuntimeFilename));
    }
    // A genuine protocol wrapper is still allowed when unconnected. The
    // mutation is solely the payload leaf, not the wrapper or selected root.
    let wrapped = TARGET.replace(r#"<entry name="Result">"#, r#"<entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Result">"#)
        .replace("</entry></root>", "</entry></entry></entry></root>");
    check(&mapping(&wrapped, "", ""), Ok(()));
    let collision = wrapped.replace(r#"name="Value""#, r#"name="FileInstance""#);
    check(&mapping(&collision, "", ""), Err(Refusal::RuntimeFilename));
}

#[test]
fn pass_through_is_rejected_by_owner_role_with_no_filename_collision() {
    check(&mapping(TARGET, "", ""), Ok(()));
    let variable = TARGET.replace(r#"PassThrough="0""#, r#"PassThrough="1""#);
    let document = roxmltree::Document::parse(&variable).unwrap();
    let root = document
        .descendants()
        .find(|n| n.has_tag_name("root"))
        .unwrap();
    assert_eq!(xml_file_instance_input(root), None);
    let entry = root
        .descendants()
        .find(|n| n.attribute("inpkey") == Some("20"))
        .unwrap();
    assert!(
        super::super::ordinary_content_entry(entry),
        "PassThrough must be the only changed role selector"
    );
    assert!(
        !document
            .descendants()
            .any(|n| n.attribute("outkey").is_some())
    );
    check(&mapping(&variable, "", ""), Err(Refusal::PassThrough));

    // The reader uses ANY direct properties element, not just the first.
    let duplicate = TARGET.replace("<data>", r#"<properties PassThrough="0"/><data>"#);
    check(&mapping(&duplicate, "", ""), Ok(()));
    let variable = duplicate.replacen(
        r#"<properties PassThrough="0"/><data>"#,
        r#"<properties PassThrough="1"/><data>"#,
        1,
    );
    check(&mapping(&variable, "", ""), Err(Refusal::PassThrough));
}

#[test]
fn a_transitive_sink_never_turns_a_pass_through_owner_into_a_terminal() {
    let forwarded = TARGET
        .replace(
            r#"<entry name="Result">"#,
            r#"<entry name="Result" inpkey="24">"#,
        )
        .replace(
            "</entry></root>",
            r#"<entry name="Forwarded" outkey="21"/></entry></root>"#,
        );
    let final_target = TARGET
        .replace("name=\"Target\"", "name=\"Final\"")
        .replace("inpkey=\"20\"", "inpkey=\"30\"");
    let walk = r#"<vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>"#;
    // Raw2 inputs/1 output keeps the reader's count-based direction a target;
    // this control isolates the forwarding refusal rather than source scoring.
    let document = roxmltree::Document::parse(&forwarded).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.attribute("inpkey").is_some())
            .count(),
        2
    );
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.attribute("outkey").is_some())
            .count(),
        1
    );
    check(
        &mapping(&forwarded, &final_target, walk),
        Err(Refusal::ForwardedOwner),
    );
    let variable = forwarded.replace(r#"PassThrough="0""#, r#"PassThrough="1""#);
    // Owner refusal precedes port admission/forwarding. Finding the final sink
    // through output21 is NOT permission to traverse the variable boundary.
    check(
        &mapping(&variable, &final_target, walk),
        Err(Refusal::PassThrough),
    );
}

#[test]
fn duplicate_normalized_keys_cannot_hide_a_protocol_or_second_owner() {
    check(&mapping(TARGET, "", ""), Ok(()));
    let protocol = TARGET.replace(
        r#"<entry name="Result">"#,
        r#"<entry name="FileInstance" inpkey="00020"/><entry name="Result">"#,
    );
    check(&mapping(&protocol, "", ""), Err(Refusal::AmbiguousInput));
    let alias = r#"<component library="core" kind="5" name="Alias"><sources><datapoint pos="0" key="00020"/></sources><targets><datapoint key="31"/></targets></component>"#;
    check(&mapping(TARGET, alias, ""), Err(Refusal::AmbiguousInput));
}

#[test]
fn later_content_roots_and_reader_skipped_ports_are_not_ordinary_targets() {
    check(&mapping(TARGET, "", ""), Ok(()));
    let later = TARGET.replace(
        r#"<entry name="Result">"#,
        r#"<entry name="Other"/><entry name="Result">"#,
    );
    check(&mapping(&later, "", ""), Err(Refusal::NonOrdinaryTarget));
    for ty in [
        "xml-type",
        "comment-before",
        "comment-after",
        "processing-instruction-before",
        "processing-instruction-after",
    ] {
        let skipped = TARGET.replace(
            r#"name="Value" inpkey="20""#,
            &format!(r#"name="Value" inpkey="20" type="{ty}""#),
        );
        check(&mapping(&skipped, "", ""), Err(Refusal::NonOrdinaryTarget));
    }
}
