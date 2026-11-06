//! Complete expanded-name XML element trees; only XML space/tab/CR/LF layout is ignored.
#[derive(Debug, PartialEq, Eq)]
enum Part {
    Element {
        name: (String, String),
        attributes: Vec<((String, String), String)>,
        content: Vec<Part>,
    },
    Text(String),
    Comment(String),
    Instruction(String, Option<String>),
}

fn tree(node: roxmltree::Node<'_, '_>) -> Part {
    match node.node_type() {
        roxmltree::NodeType::Element => {
            let name = node.tag_name();
            let mut attributes = node
                .attributes()
                .map(|attribute| {
                    (
                        (
                            attribute.namespace().unwrap_or("").to_owned(),
                            attribute.name().to_owned(),
                        ),
                        attribute.value().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            attributes.sort();
            let has_elements = node.children().any(|child| child.is_element());
            Part::Element {
                name: (
                    name.namespace().unwrap_or("").to_owned(),
                    name.name().to_owned(),
                ),
                attributes,
                content: node
                    .children()
                    .filter(|child| {
                        !(has_elements
                            && child.is_text()
                            && child
                                .text()
                                .unwrap_or("")
                                .bytes()
                                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n')))
                    })
                    .map(tree)
                    .collect(),
            }
        }
        roxmltree::NodeType::Text => Part::Text(node.text().unwrap().to_owned()),
        roxmltree::NodeType::Comment => Part::Comment(node.text().unwrap().to_owned()),
        roxmltree::NodeType::PI => {
            let instruction = node.pi().unwrap();
            Part::Instruction(
                instruction.target.to_owned(),
                instruction.value.map(str::to_owned),
            )
        }
        roxmltree::NodeType::Root => panic!("compare document elements, not XML declarations"),
    }
}

pub(super) fn assert_xml(actual: &str, expected: &str) {
    let actual = roxmltree::Document::parse(actual).unwrap();
    let expected = roxmltree::Document::parse(expected).unwrap();
    assert_eq!(tree(actual.root_element()), tree(expected.root_element()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_xml_layout_between_elements_is_ignored() {
        assert_xml(
            "<Root> \t\r\n<A>value</A>\n \t<B/>\r\n</Root>",
            "<Root><A>value</A><B/></Root>",
        );
    }

    #[test]
    fn unicode_whitespace_between_elements_is_preserved() {
        let compact = roxmltree::Document::parse("<Root><A/><B/></Root>").unwrap();
        for whitespace in ["\u{a0}", "\u{2003}"] {
            let xml = format!("<Root><A/>{whitespace}<B/></Root>");
            let document = roxmltree::Document::parse(&xml).unwrap();
            let actual = tree(document.root_element());
            assert_ne!(actual, tree(compact.root_element()));
            let Part::Element { content, .. } = actual else {
                panic!("expected element")
            };
            assert_eq!(content[1], Part::Text(whitespace.to_owned()));
        }
    }
}
