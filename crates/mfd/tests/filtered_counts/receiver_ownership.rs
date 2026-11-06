//! Reader-backed fan-out exercises actual importer receiving scopes.

use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{AggregateOp, Node, NodeId, Project, Scope, ScopeIteration};

#[path = "../declared_library_reconciliation/public_hosts/xml_tree.rs"]
mod xml_tree;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filtered-count-receivers")
        .join(name)
}

struct Package(PathBuf);

impl Package {
    fn new(design: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule_filtered_receivers_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture("")).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
        }
        std::fs::write(directory.join("map.mfd"), design).unwrap();
        Self(directory)
    }

    fn path(&self) -> PathBuf {
        self.0.join("map.mfd")
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("retained Count receiver package: {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn executable(path: &Path) -> Project {
    let best = mfd::import(path).unwrap();
    assert!(best.warnings.is_empty(), "{:?}", best.warnings);
    assert!(mfd::assess_import(&best).executable);
    assert!(engine::validate(&best.project).is_empty());
    let strict = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&best.project).unwrap(),
        mapping::project_file::encode_pretty(&strict.imported.project).unwrap(),
    );
    best.project
}

fn terminal_refusal(path: &Path) {
    let best = mfd::import(path).err();
    let strict = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .err();
    for (profile, error) in [("best-effort", best), ("executable", strict)] {
        match error {
            Some(mfd::MfdError::UnsupportedImport(reason)) => {
                assert!(reason.contains("physical filtered count"), "{reason}")
            }
            Some(other) => panic!("{profile}: wrong refusal type: {other:?}"),
            None => panic!("{profile}: an unsafe Count use published a project"),
        }
    }
}

fn child<'a>(scope: &'a Scope, name: &str) -> &'a Scope {
    let mut matches = scope
        .children
        .iter()
        .filter(|scope| scope.target_field == name);
    let result = matches.next().unwrap();
    assert!(matches.next().is_none(), "duplicate receiving scope {name}");
    result
}

fn binding(scope: &Scope, name: &str) -> NodeId {
    let mut matches = scope
        .bindings
        .iter()
        .filter(|binding| binding.target_field == name);
    let result = matches.next().unwrap().node;
    assert!(matches.next().is_none(), "duplicate Count binding {name}");
    result
}

fn scalar_field(project: &Project, id: NodeId, owner: &[&str]) {
    assert!(
        matches!(&project.graph.nodes[&id], Node::SourceField { frame: Some(frame), path }
        if frame.iter().map(String::as_str).eq(owner.iter().copied()) && path == &["Text"])
    );
}

fn integer(project: &Project, id: NodeId, expected: i64) {
    assert!(
        matches!(&project.graph.nodes[&id], Node::Const { value: Value::Int(value) } if *value == expected)
    );
}

fn contribution(project: &Project, id: NodeId, owner: &[&str]) {
    let Node::If {
        condition,
        then,
        else_,
    } = &project.graph.nodes[&id]
    else {
        panic!(
            "use lost its outside presence boundary: {:?}",
            project.graph.nodes[&id]
        );
    };
    let Node::Call { function, args } = &project.graph.nodes[condition] else {
        panic!("presence is not a call")
    };
    assert_eq!(function, "exists");
    assert_eq!(args.len(), 1);
    scalar_field(project, args[0], owner);
    integer(project, *else_, 0);
    let Node::If {
        condition,
        then,
        else_,
    } = &project.graph.nodes[then]
    else {
        panic!("missing selected contribution")
    };
    integer(project, *then, 1);
    integer(project, *else_, 0);
    let Node::Call { function, args } = &project.graph.nodes[condition] else {
        panic!("predicate was dropped")
    };
    assert_eq!(function, "equal");
    assert_eq!(args.len(), 2);
    scalar_field(project, args[0], owner);
    assert!(
        matches!(&project.graph.nodes[&args[1]], Node::Const { value: Value::String(value) } if value == "keep")
    );
}

fn root_sum(project: &Project, driver: &[&str], owner: &[&str]) -> NodeId {
    let id = binding(&project.root, "Total");
    let Node::Aggregate {
        function: AggregateOp::Sum,
        collection,
        value,
        expression: Some(expression),
        arg: None,
    } = &project.graph.nodes[&id]
    else {
        panic!("root use is not the proved conditional Sum")
    };
    assert!(
        collection
            .iter()
            .map(String::as_str)
            .eq(driver.iter().copied())
    );
    assert!(value.is_empty());
    contribution(project, *expression, owner);
    *expression
}

/// Raw engine groups keep construction order (bindings, then child scopes, in
/// target-pin order), not schema order; only the writers walk the schema. A
/// target-entry permutation therefore changes raw field order without changing
/// any output. Each expected field must still occur exactly once with its exact
/// typed value, and repeated items keep their exact order.
fn assert_complete_typed_output_by_name(actual: &Instance, expected: &serde_json::Value) {
    match expected {
        serde_json::Value::Object(fields) => {
            let Instance::Group(actual_fields) = actual else {
                panic!("expected whole group: {actual:?}")
            };
            assert_eq!(actual_fields.len(), fields.len());
            for (name, expected_value) in fields {
                let mut matches = actual_fields
                    .iter()
                    .filter(|(actual_name, _)| actual_name == name);
                let (_, actual_value) = matches
                    .next()
                    .unwrap_or_else(|| panic!("typed output lacks field {name}"));
                assert!(matches.next().is_none(), "duplicate typed field {name}");
                assert_complete_typed_output_by_name(actual_value, expected_value);
            }
        }
        serde_json::Value::Array(items) => {
            let actual = actual.as_repeated().unwrap();
            assert_eq!(actual.len(), items.len());
            for (actual, expected) in actual.iter().zip(items) {
                assert_complete_typed_output_by_name(actual, expected);
            }
        }
        scalar => super::assert_complete_typed_output(actual, scalar),
    }
}

fn whole_output(project: &Project, case: &serde_json::Value, order: usize) -> Instance {
    let input = format_xml::from_str(case["input"].as_str().unwrap(), &project.source).unwrap();
    let before = input.clone();
    let output = engine::run(project, &input).unwrap();
    assert_eq!(input, before);
    if permutes_target_entries(order) {
        assert_complete_typed_output_by_name(&output, &case["expected"]);
    } else {
        super::assert_complete_typed_output(&output, &case["expected"]);
    }
    let json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap()).unwrap();
    assert_eq!(json, case["expected"], "{}", case["name"]);
    let serialized = format_xml::to_string(&project.target, &output).unwrap();
    xml_tree::assert_xml(&serialized, case["expectedXml"].as_str().unwrap());
    let expected_xml =
        format_xml::from_str(case["expectedXml"].as_str().unwrap(), &project.target).unwrap();
    let actual_xml = format_xml::from_str(&serialized, &project.target).unwrap();
    assert_eq!(actual_xml, expected_xml, "{}", case["name"]);
    // XML serialization uses schema field order; binding declaration order is
    // not an object-value difference. Repeated item order remains exact.
    super::assert_complete_typed_output(&actual_xml, &case["expected"]);
    input
}

// Only declarations are permuted. All pins, feeds, schema paths and oracles stay
// fixed, except for a bijective target-pin relabel to reverse BTreeMap binding
// traversal too. Reversing XML entries alone would not reverse that traversal.
fn ordered(design: &str, order: usize) -> String {
    fn reverse_children(xml: &str, tag: &str) -> String {
        let document = roxmltree::Document::parse(xml).unwrap();
        let parent = document
            .descendants()
            .find(|node| node.has_tag_name(tag))
            .unwrap();
        let children = parent
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let range = children.first().unwrap().range().start..children.last().unwrap().range().end;
        let replacement = children
            .iter()
            .rev()
            .map(|node| &xml[node.range()])
            .collect::<Vec<_>>()
            .join("\n");
        let mut result = xml.to_owned();
        result.replace_range(range, &replacement);
        result
    }
    fn reversed_entry(xml: &str, node: roxmltree::Node<'_, '_>) -> String {
        let entries = node
            .children()
            .filter(|node| node.has_tag_name("entry"))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return xml[node.range()].to_owned();
        }
        let start = entries.first().unwrap().range().start;
        let end = entries.last().unwrap().range().end;
        let mut result = xml[node.range().start..start].to_owned();
        for entry in entries.into_iter().rev() {
            result.push_str(&reversed_entry(xml, entry));
        }
        result.push_str(&xml[end..node.range().end]);
        result
    }
    let mut result = design.to_owned();
    if order == 1 || order == 4 {
        result = reverse_children(&result, "children");
    }
    if order == 2 || order == 4 {
        result = reverse_children(&result, "connections");
    }
    if permutes_target_entries(order) {
        let document = roxmltree::Document::parse(&result).unwrap();
        let targets = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && matches!(node.attribute("name"), Some("target" | "unsafe-target"))
            })
            .collect::<Vec<_>>();
        let mut replacements = Vec::new();
        let mut keys = Vec::new();
        for target in targets {
            let root = target
                .descendants()
                .find(|node| node.has_tag_name("entry"))
                .unwrap();
            replacements.push((root.range(), reversed_entry(&result, root)));
            keys.extend(
                target
                    .descendants()
                    .filter_map(|node| node.attribute("inpkey"))
                    .map(|key| key.parse::<u32>().unwrap()),
            );
        }
        drop(document);
        for (range, replacement) in replacements.into_iter().rev() {
            result.replace_range(range, &replacement);
        }
        for old in keys {
            assert!((100..400).contains(&old));
            let new = 900 - old;
            result = result.replace(&format!("inpkey=\"{old}\""), &format!("inpkey=\"{new}\""));
            result = result.replace(&format!("to=\"{old}\""), &format!("to=\"{new}\""));
        }
    }
    roxmltree::Document::parse(&result).unwrap();
    result
}

fn permutes_target_entries(order: usize) -> bool {
    order == 3 || order == 4
}

fn with_unsafe_use(kind: &str) -> String {
    if kind == "other-target" {
        let base = include_str!("../fixtures/filtered-count-receivers/row-cell-fanout.mfd");
        let target = r#"<component name="unsafe-target" library="xml" kind="14"><data><root><entry name="OtherResults"><entry name="Row" inpkey="210"><entry name="Inner" inpkey="230"><entry name="Count" inpkey="231"/></entry></entry></entry></root><document schema="other-target.xsd" instanceroot="{}OtherResults"/></data></component>"#;
        let changed = super::replace_one(base, "</children>", &format!("{target}</children>"));
        return super::replace_one(
            &changed,
            "</connections>",
            r#"<edge from="10" to="210"/><edge from="14" to="230"/><edge from="51" to="231"/></connections>"#,
        );
    }
    let (base, schema, unsafe_schema, anchor, replacement, edges) = match kind {
        "row-row" => (
            include_str!("../fixtures/filtered-count-receivers/row-cell-fanout.mfd"),
            "row-cell-target.xsd",
            "row-row-target.xsd",
            r#"<entry name="Cell" inpkey="120"><entry name="Label" inpkey="121"/><entry name="Count" inpkey="122"/></entry>"#,
            r#"<entry name="Cell" inpkey="120"><entry name="Label" inpkey="121"/><entry name="Count" inpkey="122"/></entry><entry name="Inner" inpkey="130"><entry name="Count" inpkey="131"/></entry>"#,
            r#"<edge from="14" to="130"/><edge from="51" to="131"/>"#,
        ),
        "wrapper" => (
            include_str!("../fixtures/filtered-count-receivers/wrapper-root-control.mfd"),
            "wrapper-target.xsd",
            "wrapper-unsafe-target.xsd",
            r#"<entry name="Label" inpkey="111"/>"#,
            r#"<entry name="Label" inpkey="111"/><entry name="Count" inpkey="112"/>"#,
            r#"<edge from="51" to="112"/>"#,
        ),
        "group" => (
            include_str!("../fixtures/filtered-count-receivers/group-control.mfd"),
            "group-target.xsd",
            "group-unsafe-target.xsd",
            r#"<entry name="Key" inpkey="141"/>"#,
            r#"<entry name="Key" inpkey="141"/><entry name="Count" inpkey="142"/>"#,
            r#"<edge from="51" to="142"/>"#,
        ),
        _ => panic!("unknown authored receiver shape"),
    };
    let changed = super::replace_one(base, schema, unsafe_schema);
    let changed = super::replace_one(&changed, anchor, replacement);
    super::replace_one(
        &changed,
        "</connections>",
        &format!("{edges}</connections>"),
    )
}

#[test]
fn one_count_has_fresh_root_exact_owner_and_distinct_child_proofs_in_all_orders() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/row-cell-cases.json"
    ))
    .unwrap();
    for order in 0..5 {
        let package = Package::new(&ordered(
            include_str!("../fixtures/filtered-count-receivers/row-cell-fanout.mfd"),
            order,
        ));
        let project = executable(&package.path());
        let root = root_sum(&project, &["Row"], &["Row"]);
        let rows = child(&project.root, "Row");
        let cells = child(rows, "Cell");
        assert_eq!(rows.iteration, ScopeIteration::Source(vec!["Row".into()]));
        assert_eq!(cells.iteration, ScopeIteration::Source(vec!["Cell".into()]));
        assert!(!rows.has_grouping() && !cells.has_grouping());
        let row_use = binding(rows, "Count");
        let cell_use = binding(cells, "Count");
        contribution(&project, row_use, &["Row"]);
        contribution(&project, cell_use, &["Row"]);
        assert_ne!(root, row_use);
        assert_ne!(root, cell_use);
        assert_ne!(row_use, cell_use);
        assert!(!project.graph.nodes.values().any(|node| matches!(
            node,
            Node::Aggregate {
                function: AggregateOp::Count,
                ..
            }
        )));
        for case in &cases {
            let input = whole_output(&project, case, order);
            let rows = input.field("Row").unwrap().as_repeated().unwrap();
            if let Some(last) = rows.last() {
                assert!(
                    last.field("Cell")
                        .unwrap()
                        .as_repeated()
                        .unwrap()
                        .is_empty()
                );
            } else {
                assert_eq!(
                    case["name"],
                    "empty_direct_repeated_row_slot_does_not_select_a_donor"
                );
            }
        }
    }
}

#[test]
fn a_separately_owned_pin_for_the_same_physical_field_keeps_the_same_oracles() {
    let design = include_str!("../fixtures/filtered-count-receivers/row-cell-fanout.mfd")
        .replace(
            r#"<entry name="Text" outkey="11"/>"#,
            r#"<entry name="Text" outkey="11"/><entry name="Text" outkey="16"/>"#,
        )
        .replace("from=\"11\" to=\"30\"", "from=\"16\" to=\"30\"");
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/row-cell-cases.json"
    ))
    .unwrap();
    for order in 0..5 {
        let package = Package::new(&ordered(&design, order));
        let project = executable(&package.path());
        for case in &cases {
            whole_output(&project, case, order);
        }
    }
}

#[test]
fn one_unsafe_row_row_ancestor_pin_use_terminally_refuses_the_same_count_fanout() {
    let reader = executable(&fixture("row-cell-fanout.mfd"));
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/row-row-inputs.json"
    ))
    .unwrap();
    for case in cases {
        let input = format_xml::from_str(case["input"].as_str().unwrap(), &reader.source).unwrap();
        let outer = &input.field("Row").unwrap().as_repeated().unwrap()[0];
        assert_eq!(outer.field("Row").unwrap().as_repeated().unwrap().len(), 1);
    }
    for order in 0..5 {
        let package = Package::new(&ordered(&with_unsafe_use("row-row"), order));
        terminal_refusal(&package.path());
    }
}

#[test]
fn an_unsafe_use_in_another_authored_target_cannot_be_lost_when_targets_reorder() {
    for order in 0..5 {
        let design = ordered(&with_unsafe_use("other-target"), order);
        let document = roxmltree::Document::parse(&design).unwrap();
        let targets = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && matches!(node.attribute("name"), Some("target" | "unsafe-target"))
            })
            .map(|node| node.attribute("name").unwrap())
            .collect::<Vec<_>>();
        if order == 1 || order == 4 {
            assert_eq!(targets, ["unsafe-target", "target"]);
        } else {
            assert_eq!(targets, ["target", "unsafe-target"]);
        }
        let package = Package::new(&design);
        terminal_refusal(&package.path());
    }
}

#[test]
fn safe_second_target_uses_keep_their_registered_slot_when_targets_reorder() {
    let design =
        with_unsafe_use("other-target").replace("from=\"14\" to=\"230\"", "from=\"12\" to=\"230\"");
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/row-cell-cases.json"
    ))
    .unwrap();
    let expected = serde_json::json!({"Row":[{"Inner":[{"Count":1},{"Count":1}]},{"Inner":[{"Count":0}]},{"Inner":[{"Count":0}]},{"Inner":[]}]});
    let expected_xml = "<OtherResults><Row><Inner><Count>1</Count></Inner><Inner><Count>1</Count></Inner></Row><Row><Inner><Count>0</Count></Inner></Row><Row><Inner><Count>0</Count></Inner></Row><Row/></OtherResults>";
    for order in 0..5 {
        let package = Package::new(&ordered(&design, order));
        let project = executable(&package.path());
        assert_eq!(project.target.name, "Results");
        assert_eq!(project.extra_targets.len(), 1);
        let descriptor = &project.extra_targets[0];
        assert_eq!(descriptor.schema.name, "OtherResults");
        let input = whole_output(&project, &cases[0], order);
        let outputs = engine::run_outputs(&project, &input).unwrap();
        assert_eq!(outputs.extras.len(), 1);
        let extra = &outputs.extras[0];
        assert_eq!(extra.name, descriptor.name);
        super::assert_complete_typed_output(&extra.instance, &expected);
        let xml = format_xml::to_string(&descriptor.schema, &extra.instance).unwrap();
        assert_eq!(
            format_xml::from_str(&xml, &descriptor.schema).unwrap(),
            format_xml::from_str(expected_xml, &descriptor.schema).unwrap()
        );
        contribution(
            &project,
            binding(child(child(&descriptor.root, "Row"), "Inner"), "Count"),
            &["Row"],
        );
    }
}

#[test]
fn singleton_before_repeat_refuses_by_shape_even_for_present_and_empty_controls() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/wrapper-cases.json"
    ))
    .unwrap();
    for order in 0..5 {
        let control = Package::new(&ordered(
            include_str!("../fixtures/filtered-count-receivers/wrapper-root-control.mfd"),
            order,
        ));
        let project = executable(&control.path());
        root_sum(&project, &["Row", "Wrap", "Leaf"], &["Row", "Wrap", "Leaf"]);
        for case in &cases {
            let input = whole_output(&project, case, order);
            assert!(
                input.field("Wrap").is_some(),
                "outer donor is real reader data"
            );
            let row = &input.field("Row").unwrap().as_repeated().unwrap()[0];
            match case["name"].as_str().unwrap() {
                "missing_singleton_with_outer_donor" => assert!(row.field("Wrap").is_none()),
                "present_singleton_with_empty_repeat" => assert!(
                    row.field("Wrap")
                        .unwrap()
                        .field("Leaf")
                        .unwrap()
                        .as_repeated()
                        .unwrap()
                        .is_empty()
                ),
                "present_singleton_with_nonempty_repeat" => assert_eq!(
                    row.field("Wrap")
                        .unwrap()
                        .field("Leaf")
                        .unwrap()
                        .as_repeated()
                        .unwrap()
                        .len(),
                    2
                ),
                _ => panic!("unknown authored wrapper case"),
            }
        }
        // Adding the Row receiver shortens the intended Count driver to
        // singleton-first Wrap/Leaf. Refusal is import-shape based for ALL
        // three reader inputs, not only when the singleton happens to be absent.
        let unsafe_use = Package::new(&ordered(&with_unsafe_use("wrapper"), order));
        terminal_refusal(&unsafe_use.path());
    }
}

#[test]
fn actual_group_by_receiver_with_the_same_source_path_cannot_reuse_ordinary_proof() {
    let case: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/filtered-count-receivers/group-case.json"
    ))
    .unwrap();
    for order in 0..5 {
        let control = Package::new(&ordered(
            include_str!("../fixtures/filtered-count-receivers/group-control.mfd"),
            order,
        ));
        let project = executable(&control.path());
        let ordinary = child(&project.root, "Ordinary");
        let grouped = child(&project.root, "Grouped");
        assert_eq!(
            ordinary.iteration,
            ScopeIteration::Source(vec!["Row".into()])
        );
        assert_eq!(grouped.iteration, ordinary.iteration);
        assert!(!ordinary.has_grouping());
        assert!(
            grouped.group_by.is_some(),
            "must exercise a real grouped MFD receiver"
        );
        root_sum(&project, &["Row"], &["Row"]);
        contribution(&project, binding(ordinary, "Count"), &["Row"]);
        whole_output(&project, &case, order);
        let unsafe_use = Package::new(&ordered(&with_unsafe_use("group"), order));
        terminal_refusal(&unsafe_use.path());
    }
}

#[test]
fn an_innermost_row_self_pin_is_not_rejected_merely_for_colliding_names() {
    let design = with_unsafe_use("row-row")
        .replace("from=\"11\" to=\"30\"", "from=\"15\" to=\"30\"")
        .replace("from=\"11\" to=\"40\"", "from=\"15\" to=\"40\"");
    let case = serde_json::json!({
        "name":"inner_row_owns_its_own_latest_pin",
        "input":"<Source><Row><Text>drop</Text><Cell><Label>safe</Label></Cell><Row><Text>keep</Text></Row><Row><Text>drop</Text></Row></Row></Source>",
        "expected":{"Total":1,"Row":[{"Count":1,"Cell":[{"Label":"safe","Count":1}],"Inner":[{"Count":1},{"Count":0}]}]},
        "expectedXml":"<Results><Total>1</Total><Row><Count>1</Count><Cell><Label>safe</Label><Count>1</Count></Cell><Inner><Count>1</Count></Inner><Inner><Count>0</Count></Inner></Row></Results>"
    });
    for order in 0..5 {
        let package = Package::new(&ordered(&design, order));
        let project = executable(&package.path());
        contribution(
            &project,
            binding(child(child(&project.root, "Row"), "Inner"), "Count"),
            &["Row", "Row"],
        );
        whole_output(&project, &case, order);
    }
}
