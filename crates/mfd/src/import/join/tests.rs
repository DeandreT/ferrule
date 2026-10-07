use super::*;

const TWO_WAY: &str = include_str!("../../../tests/fixtures/join-two-way.mfd");
const THREE_WAY: &str = include_str!("../../../tests/fixtures/join-three-way.mfd");
const MALFORMED: &str = include_str!("../../../tests/fixtures/join-malformed.mfd");

fn parse_fixture(text: &str) -> Result<ParsedJoin, String> {
    let document = roxmltree::Document::parse(text).map_err(|error| error.to_string())?;
    let component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .ok_or("fixture has no join component")?;
    parse(&component)
}

#[test]
fn parses_two_way_join_ports_outputs_and_paths() {
    let join = parse_fixture(TWO_WAY).unwrap();
    assert_eq!(join.tuple_output, Some(90));
    assert_eq!(join.inputs.len(), 2);
    assert_eq!(
        join.inputs[0],
        JoinInput {
            index: 0,
            name: "Left".to_string(),
            input_port: 10,
            outputs: vec![
                JoinOutput {
                    port: 11,
                    path: Vec::new(),
                },
                JoinOutput {
                    port: 12,
                    path: vec!["Label".to_string()],
                },
            ],
        }
    );
    assert_eq!(join.equalities.len(), 1);
    assert_eq!(join.equalities[0].first.input_index, 0);
    assert_eq!(join.equalities[0].first.path, ["Id"]);
    assert_eq!(join.equalities[0].second.input_index, 1);
    assert_eq!(join.equalities[0].second.path, ["Code"]);

    let planned = join
        .to_plan(&[vec!["Orders".into()], vec!["Catalog".into()]])
        .unwrap();
    assert_eq!(planned.plan.sources().count(), 2);
    assert_eq!(planned.plan.stages().count(), 1);
    assert!(planned.outputs.iter().any(|output| {
        output.port == 12 && output.collection == ["Orders"] && output.path == ["Label"]
    }));
}

#[test]
fn parses_three_way_join_with_explicit_later_input_indices() {
    let join = parse_fixture(THREE_WAY).unwrap();
    assert_eq!(join.inputs.len(), 3);
    assert_eq!(join.equalities.len(), 2);
    assert_eq!(
        join.equalities
            .iter()
            .map(|equality| (equality.first.input_index, equality.second.input_index))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2)]
    );
    let planned = join
        .to_plan(&[
            vec!["Office".into()],
            vec!["Department".into()],
            vec!["Person".into()],
        ])
        .unwrap();
    assert_eq!(planned.plan.sources().count(), 3);
    assert_eq!(planned.plan.stages().count(), 2);
}

#[test]
fn rejects_noncontiguous_input_fixture() {
    let error = parse_fixture(MALFORMED).unwrap_err();
    assert!(error.contains("contiguous from 0"), "{error}");
}

#[test]
fn rejects_ambiguous_or_unsupported_join_metadata() {
    for (text, expected) in [
        (
            TWO_WAY.replace(
                "<joinkeys><keypair>",
                "<joinkeys><keypair><first-key path-id=\"1\" input-index=\"0\"/><second-key path-id=\"2\" input-index=\"0\"/></keypair><keypair>",
            ),
            "distinct inputs",
        ),
        (
            TWO_WAY.replace(
                "<second-key path-id=\"2\"/>",
                "<second-key path-id=\"2\" input-index=\"2\"/>",
            ),
            "out of range",
        ),
        (
            TWO_WAY.replace(
                "<joinkeys><keypair><first-key path-id=\"1\"/><second-key path-id=\"2\"/></keypair></joinkeys>",
                "<joinkeys/>",
            ),
            "at least one equality",
        ),
        (
            TWO_WAY.replace("<condition/>", "<condition><expression/></condition>"),
            "custom key-path conditions",
        ),
        (
            TWO_WAY.replace("outkey=\"2\"", "outkey=\"1\""),
            "repeats key path id",
        ),
    ] {
        let error = parse_fixture(&text).unwrap_err();
        assert!(error.contains(expected), "expected `{expected}`, got `{error}`");
    }
}

const CORRELATED_THREE: &str = r#"<mapping version="26"><component name="map"><structure><children>
<component name="join" library="core" kind="32"><data>
<root><entry name="document"><entry name="tuple" outkey="90">
<entry name="dynamic_tree_node0"><entry name="CustomerNumber" inpkey="10"/></entry>
<entry name="dynamic_tree_node1"><entry name="Customer" inpkey="20"><entry name="Name" outkey="22"/></entry></entry>
<entry name="dynamic_tree_node2"><entry name="Offer" inpkey="30"><entry name="Promo" outkey="32"/></entry></entry>
</entry></entry></root>
<join><joinkeys>
<keypair><first-key path-id="1" input-index="0"/><second-key path-id="2" input-index="1"/></keypair>
<keypair><first-key path-id="3" input-index="1"/><second-key path-id="3" input-index="2"/></keypair>
<keypair><first-key path-id="1" input-index="0"/><second-key path-id="2" input-index="2"/></keypair>
</joinkeys><keypaths><entry outkey="1"><condition/>
<entry name="Number" outkey="2"><condition/></entry><entry name="Region" outkey="3"><condition/></entry>
</entry></keypaths></join></data></component>
</children></structure></component></mapping>"#;

#[test]
fn correlated_three_input_metadata_keeps_both_earlier_owners_and_condition_order() {
    let parsed = parse_fixture(CORRELATED_THREE);
    eprintln!("correlated three input XML: {CORRELATED_THREE}\nparsed: {parsed:#?}");
    let parsed = parsed.unwrap();
    assert_eq!(
        parsed
            .equalities
            .iter()
            .map(|key| (key.first.input_index, key.second.input_index))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2), (0, 2)]
    );
    let sources = [
        JoinSource::singleton(vec!["CustomerNumber".into()]),
        JoinSource::new(vec!["Customers".into(), "Customer".into()]),
        JoinSource::new(vec!["Offers".into(), "Offer".into()]),
    ];
    let planned = parsed.to_plan_sources(&sources);
    eprintln!("correlated three planned sources: {sources:#?}\nplanned: {planned:#?}");
    let planned = planned.unwrap();
    let stages = planned.plan.stages().collect::<Vec<_>>();
    assert_eq!(planned.plan.sources().cloned().collect::<Vec<_>>(), sources);
    assert_eq!(stages.len(), 2);
    assert_eq!(
        stages[0].1.iter().cloned().collect::<Vec<_>>(),
        [MappingJoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()]
        )]
    );
    assert_eq!(
        stages[1].1.iter().cloned().collect::<Vec<_>>(),
        [
            MappingJoinKey::new(
                vec!["Customers".into(), "Customer".into()],
                vec!["Region".into()],
                vec!["Region".into()]
            ),
            MappingJoinKey::new(vec!["CustomerNumber".into()], vec![], vec!["Number".into()]),
        ]
    );
    assert_eq!(planned.tuple_output, Some(90));
    assert_eq!(
        planned.outputs,
        [
            PlannedJoinOutput {
                port: 22,
                input_index: 1,
                collection: vec!["Customers".into(), "Customer".into()],
                path: vec!["Name".into()]
            },
            PlannedJoinOutput {
                port: 32,
                input_index: 2,
                collection: vec!["Offers".into(), "Offer".into()],
                path: vec!["Promo".into()]
            },
        ]
    );
}

#[test]
fn correlated_three_input_metadata_refuses_future_owners_and_unknown_inputs() {
    for (xml, expected) in [
        (
            CORRELATED_THREE.replacen(
                "<first-key path-id=\"1\" input-index=\"0\"/>",
                "<first-key path-id=\"1\" input-index=\"2\"/>",
                1,
            ),
            "join input 1 must have an equality with an earlier input",
        ),
        (
            CORRELATED_THREE.replace(
                "<first-key path-id=\"3\" input-index=\"1\"/>",
                "<first-key path-id=\"3\" input-index=\"3\"/>",
            ),
            "input index 3 is out of range for 3 inputs",
        ),
    ] {
        let parsed = parse_fixture(&xml);
        eprintln!("rejected correlated three input XML: {xml}\nparsed: {parsed:#?}");
        assert!(matches!(parsed, Err(reason) if reason.contains(expected)));
    }
}
