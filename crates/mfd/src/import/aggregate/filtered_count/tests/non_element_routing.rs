//! Bounded, actual reproductions of the non-element negative-work omission.
use super::review_regressions::{is_count_refusal, ordinary_design, outcome, reverse_sections};
use super::*;

const COMPACT: &str = r#"<mapping version="26"><component name="map"><structure><children><component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"/></entry></root><document schema="row-cell-source.xsd" instanceroot="{}Source"/></data></component><component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="50"/></sources><targets><datapoint key="51"/></targets></component><component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Results"><entry name="Total" inpkey="100"/></entry></root><document schema="row-cell-target.xsd" instanceroot="{}Results"/></data></component></children><connections><edge from="10" to="50"/><edge from="51" to="100"/></connections></structure></component></mapping>"#;
const INPUT: &str = "<Source><Row/><Row/></Source>";
const EXPECTED: &str = "<Results><Total>2</Total></Results>";

fn graph(body: &str, edge_child: bool) -> String {
    let mut graph = String::from("<graph><vertices>");
    for _ in 0..20 {
        graph.push_str(r#"<vertex vertexkey="90001">"#);
    }
    if edge_child {
        graph.push_str(r#"<edges><edge vertexkey="90000">"#);
    }
    graph.push_str(body);
    if edge_child {
        graph.push_str("</edge></edges>");
    }
    for _ in 0..20 {
        graph.push_str("</vertex>");
    }
    graph.push_str("</vertices></graph>");
    graph
}
fn design(body: &str, edge_child: bool, wrapper: bool) -> String {
    let graph = graph(body, edge_child);
    COMPACT.replace(
        "</structure>",
        &if wrapper {
            format!("</structure>{graph}")
        } else {
            format!("{graph}</structure>")
        },
    )
}
fn prepare(text: &str) -> routing::Report {
    let document = crate::design::parse(text).unwrap();
    let structure = document
        .descendants()
        .find(|n| n.has_tag_name("structure"))
        .unwrap();
    let header = routing::Headers::read(structure).unwrap();
    reset();
    let (_, negative) = crate::import::graph::read_edge_inputs_with_probe(
        &structure,
        structure.parent().as_ref(),
        |claims| routing::Routing::prepare(header, structure, claims),
    );
    assert!(negative.unwrap().is_some());
    assert_eq!(counts()[Counter::EdgeIndex as usize], 1);
    routing::report()
}
fn preserve(label: &str, text: &str) {
    if let Some(root) = std::env::var_os("FERRULE_COUNT_REVIEW84_EVIDENCE") {
        let folder = PathBuf::from(root).join(label);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("map.mfd"), text).unwrap();
    }
}

#[test]
fn non_element_nested_walk_work_must_grow_with_ignored_dom_nodes() {
    let mut failures = Vec::new();
    for (kind, token) in [
        ("comments", "<!---->"),
        ("text", "text<!---->"),
        ("pi", "<?probe?>"),
    ] {
        for edge_child in [false, true] {
            for wrapper in [false, true] {
                let empty = prepare(&design("", edge_child, wrapper));
                let text = design(&token.repeat(256), edge_child, wrapper);
                let populated = prepare(&text);
                let label = format!("non-elements-{kind}-edge-{edge_child}-wrapper-{wrapper}");
                preserve(&label, &text);
                eprintln!("REVIEWAD {label} empty={empty:?} populated={populated:?}");
                assert!(
                    populated.work > empty.work,
                    "the actual charged traversal must grow"
                );
                assert!(populated.header_work > empty.header_work);
                assert_eq!(populated.dimensions.nodes, empty.dimensions.nodes);
                assert!(populated.dimensions.walk_nodes > empty.dimensions.walk_nodes);
                assert_eq!(populated.dimensions.decoded, empty.dimensions.decoded);
                assert_eq!(
                    populated.limits.1, empty.limits.1,
                    "ignored visits cannot grant retained slots"
                );
                assert_eq!(populated.limits.2, empty.limits.2);
                assert_eq!(
                    (populated.slots, populated.bytes),
                    (empty.slots, empty.bytes)
                );
                if populated.limits.0 <= empty.limits.0 {
                    failures.push(label);
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "actual work grows while its derived ceiling stays constant: {}",
        failures.join(", ")
    );
}

#[test]
fn non_element_compact_ordinary_count_keeps_independent_whole_output() {
    for (kind, token) in [
        ("comments", "<!---->"),
        ("text", "text<!---->"),
        ("pi", "<?probe?>"),
    ] {
        for edge_child in [false, true] {
            for wrapper in [false, true] {
                let design = design(&token.repeat(64), edge_child, wrapper);
                for parent_wires in [false, true] {
                    let design = if parent_wires {
                        super::resource_followup::parent_connections(&design, 0)
                    } else {
                        design.clone()
                    };
                    for reverse in [false, true] {
                        let design = if reverse {
                            reverse_sections(&design)
                        } else {
                            design.clone()
                        };
                        let package = Package::new(&design);
                        preserve(
                            &format!(
                                "ordinary-{kind}-edge-{edge_child}-wrapper-{wrapper}-parent-wires-{parent_wires}-reverse-{reverse}"
                            ),
                            &design,
                        );
                        for limits in [None, Some((0, 0)), Some((12, 1024))] {
                            let _guard = LimitsGuard;
                            LIMITS.set(limits);
                            for strict in [false, true] {
                                reset();
                                let imported = outcome(&package, strict).unwrap();
                                assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                                let input =
                                    format_xml::from_str(INPUT, &imported.project.source).unwrap();
                                let output = engine::run(&imported.project, &input).unwrap();
                                let actual =
                                    format_xml::to_string(&imported.project.target, &output)
                                        .unwrap();
                                assert_eq!(
                                    format_xml::from_str(&actual, &imported.project.target)
                                        .unwrap(),
                                    format_xml::from_str(EXPECTED, &imported.project.target)
                                        .unwrap(),
                                    "{actual}"
                                );
                                assert_eq!(counts()[Counter::RawBuild as usize], 0);
                                assert_eq!(counts()[Counter::FinalBuild as usize], 0);
                                assert_eq!(counts()[Counter::Origin as usize], 0);
                                assert_eq!(counts()[Counter::EdgeIndex as usize], 1);
                                eprintln!(
                                    "REVIEWAD ordinary kind={kind} edge_child={edge_child} wrapper={wrapper} parent_wires={parent_wires} reverse={reverse} limits={limits:?} strict={strict} output={actual:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn non_element_parent_records_keep_affected_count_refusal() {
    for (kind, token) in [
        ("comments", "<!---->"),
        ("text", "text<!---->"),
        ("pi", "<?probe?>"),
    ] {
        for graph in [false, true] {
            let body = token.repeat(64);
            let extra = if graph {
                format!(
                    r#"<graph><vertices><vertex vertexkey="42"><edges><edge vertexkey="50">{body}</edge></edges></vertex></vertices></graph>"#
                )
            } else {
                format!(r#"<connections><edge from="42" to="50">{body}</edge></connections>"#)
            };
            let design = ordinary_design(DESIGN).replace(
                "</structure>",
                &format!("</structure><connections/>{extra}"),
            );
            for reverse in [false, true] {
                let design = if reverse {
                    reverse_sections(&design)
                } else {
                    design.clone()
                };
                let package = Package::new(&design);
                preserve(
                    &format!("affected-{kind}-graph-{graph}-reverse-{reverse}"),
                    &design,
                );
                for limits in [None, Some((0, 0)), Some((12, 1024))] {
                    let _guard = LimitsGuard;
                    LIMITS.set(limits);
                    for strict in [false, true] {
                        reset();
                        let result = outcome(&package, strict);
                        assert!(is_count_refusal(&result), "{:?}", result.as_ref().err());
                    }
                }
            }
        }
    }
}

fn all_kind_basis(text: &str) -> usize {
    let document = crate::design::parse(text).unwrap();
    let structure = document
        .descendants()
        .find(|n| n.has_tag_name("structure"))
        .unwrap();
    let parent_records = structure.parent().into_iter().flat_map(|parent| {
        parent
            .children()
            .filter(|n| n.has_tag_name("connections") || n.has_tag_name("graph"))
            .flat_map(|container| container.descendants())
    });
    structure.descendants().chain(parent_records).count()
        + std::iter::once(structure)
            .chain(structure.parent())
            .map(|parent| parent.children().count())
            .sum::<usize>()
}

#[test]
fn non_element_work_basis_includes_owner_walks_and_wrapper_selectors_without_storage() {
    let body = "<!---->text<?probe?>".repeat(64);
    let text = design(&body, true, true)
        .replace("<children>", &format!("<children>{body}"))
        .replace("<sources>", &format!("<sources>{body}"))
        .replace("</structure>", &format!("</structure>{body}"));
    let report = prepare(&text);
    assert_eq!(report.dimensions.walk_nodes, all_kind_basis(&text));
    let d = report.dimensions;
    let retained = d.nodes + d.pins + d.edges + d.attributes + d.registered + 1;
    let work = retained as u128 + d.walk_nodes as u128;
    assert_eq!(
        report.limits.0,
        1024 * work * (work + d.decoded as u128 + 1)
    );
    assert_eq!(report.limits.1, 256 * retained);
    assert_eq!(report.limits.2, 128 * report.limits.1);
    preserve("owner-selector-dimension-basis", &text);
    eprintln!(
        "REVIEWAD owner/selectors all-kind basis={} report={report:?}",
        all_kind_basis(&text)
    );
    let package = Package::new(&text);
    for strict in [false, true] {
        let _guard = LimitsGuard;
        LIMITS.set(Some((0, 0)));
        reset();
        let imported = outcome(&package, strict).unwrap();
        assert!(imported.warnings.is_empty());
        let input = format_xml::from_str(INPUT, &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        let actual = format_xml::to_string(&imported.project.target, &output).unwrap();
        assert_eq!(
            format_xml::from_str(&actual, &imported.project.target).unwrap(),
            format_xml::from_str(EXPECTED, &imported.project.target).unwrap()
        );
        assert_eq!(counts()[Counter::RawBuild as usize], 0);
        assert_eq!(counts()[Counter::FinalBuild as usize], 0);
    }
}

#[test]
fn non_element_no_count_keeps_zero_negative_storage_and_ordinary_output() {
    let body = "<!---->text<?probe?>".repeat(64);
    let text = design(&body, true, true);
    let doc = crate::design::parse(&text).unwrap();
    let range = doc
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("count"))
        .unwrap()
        .range();
    let mut text = text.clone();
    text.replace_range(range,r#"<component name="constant" library="core" kind="2"><targets><datapoint key="51"/></targets><data><constant value="2" datatype="integer"/></data></component>"#);
    let text = text.replace(r#"<edge from="10" to="50"/>"#, "");
    for parent in [false, true] {
        let text = if parent {
            super::resource_followup::parent_connections(&text, 0)
        } else {
            text.clone()
        };
        preserve(&format!("no-count-non-elements-parent-{parent}"), &text);
        let package = Package::new(&text);
        for strict in [false, true] {
            reset();
            let imported = outcome(&package, strict).unwrap();
            assert!(imported.warnings.is_empty());
            let input = format_xml::from_str(INPUT, &imported.project.source).unwrap();
            let output = engine::run(&imported.project, &input).unwrap();
            let actual = format_xml::to_string(&imported.project.target, &output).unwrap();
            assert_eq!(
                format_xml::from_str(&actual, &imported.project.target).unwrap(),
                format_xml::from_str(EXPECTED, &imported.project.target).unwrap()
            );
            let report = routing::report();
            assert_eq!(report.dimensions.walk_nodes, all_kind_basis(&text));
            assert!(report.header_work >= report.dimensions.walk_nodes);
            assert_eq!(report.slots, 0);
            assert_eq!(report.bytes, 0);
            assert_eq!(report.retained_graph_slots, 0);
            assert_eq!(counts()[Counter::NegativeBuild as usize], 0);
            assert_eq!(counts()[Counter::Origin as usize], 0);
            assert_eq!(counts()[Counter::RawBuild as usize], 0);
            assert_eq!(counts()[Counter::FinalBuild as usize], 0);
            assert_eq!(counts()[Counter::EdgeIndex as usize], 1);
            eprintln!("REVIEWAD NoCount parent={parent} strict={strict} report={report:?}");
        }
    }
}
