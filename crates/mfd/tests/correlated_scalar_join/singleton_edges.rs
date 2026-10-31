use super::*;

/// Reports, in input order, whether each kind-32 input fed by a source port has
/// a copy-all connection (`true`) or an ordinary, target driven one (`false`).
pub(super) fn source_join_inputs_copy_all(xml: &str) -> Result<Vec<bool>, Box<dyn Error>> {
    let document = roxmltree::Document::parse(xml)?;
    let graph = document
        .descendants()
        .find(|node| node.has_tag_name("graph"))
        .ok_or("missing graph")?;
    let source_outputs = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("xml"))
        .flat_map(|component| component.descendants())
        .filter_map(|node| node.attribute("outkey"))
        .collect::<Vec<_>>();
    let mut modes = Vec::new();
    for join in document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
    {
        for input in join
            .descendants()
            .filter_map(|node| node.attribute("inpkey"))
        {
            let feeds = graph
                .descendants()
                .filter(|node| node.has_tag_name("vertex"))
                .flat_map(|vertex| {
                    vertex
                        .descendants()
                        .filter(|edge| {
                            edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(input)
                        })
                        .map(move |edge| (vertex, edge))
                })
                .collect::<Vec<_>>();
            assert_eq!(feeds.len(), 1, "join input {input} must have one feed");
            let (vertex, edge) = feeds[0];
            assert!(
                vertex
                    .attribute("vertexkey")
                    .is_some_and(|output| source_outputs.contains(&output)),
                "join input {input} must be fed by a source port"
            );
            let copy_all = match edge.attribute("edgekey") {
                None => false,
                Some(key) => {
                    let kind = graph
                        .children()
                        .filter(|node| node.has_tag_name("edges"))
                        .flat_map(|node| node.children())
                        .find(|node| {
                            node.has_tag_name("edge") && node.attribute("edgekey") == Some(key)
                        })
                        .and_then(|node| {
                            node.descendants()
                                .find(|node| node.has_tag_name("dataconnection"))
                        })
                        .and_then(|node| node.attribute("type"));
                    assert_eq!(
                        kind,
                        Some("2"),
                        "join input {input} has an unknown edge kind"
                    );
                    true
                }
            };
            modes.push(copy_all);
        }
    }
    Ok(modes)
}

#[test]
fn nested_scalar_join_imports_the_native_copy_all_singleton_form() -> Result<(), Box<dyn Error>> {
    // The reference application accepts the singleton input only through a
    // copy-all connection, like the repeating input.
    let directory = TempDir::new()?;
    let legacy = mfd::import(&write_fixture(&directory.0)?)?;
    let design = directory.0.join("mapping.mfd");
    let text = std::fs::read_to_string(&design)?
        .replace(
            "<graph><vertices>",
            r#"<graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge></edges><vertices>"#,
        )
        .replace(
            r#"<edge vertexkey="10"/>"#,
            r#"<edge vertexkey="10" edgekey="90"/>"#,
        )
        .replace(
            r#"<edge vertexkey="20"/>"#,
            r#"<edge vertexkey="20" edgekey="91"/>"#,
        );
    assert_eq!(source_join_inputs_copy_all(&text)?, [true, true]);
    std::fs::write(&design, text)?;
    let native = mfd::import(&design)?;
    assert!(native.warnings.is_empty(), "{:?}", native.warnings);
    assert!(engine::validate(&native.project).is_empty());
    assert_eq!(native.project.root, legacy.project.root);
    assert_execution(&run_fixture(&native.project)?)?;
    Ok(())
}
