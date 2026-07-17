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
