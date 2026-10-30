use crate::import::schema::parse_u32;

/// Every element below `call` in document order, each with the `componentid`
/// that the element itself or its nearest `<entry>` ancestor declares.
///
/// Walks with an explicit stack: a call component may nest any elements, and
/// the design's entry-depth limit does not bound elements that are not
/// entries.
pub(super) fn call_elements<'a, 'input>(
    call: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = (roxmltree::Node<'a, 'input>, Option<u32>)> {
    let mut stack = Vec::new();
    push_children(&mut stack, call, None);
    std::iter::from_fn(move || {
        let (node, component) = stack.pop()?;
        push_children(&mut stack, node, component);
        Some((node, component))
    })
}

fn push_children<'a, 'input>(
    stack: &mut Vec<(roxmltree::Node<'a, 'input>, Option<u32>)>,
    parent: roxmltree::Node<'a, 'input>,
    inherited: Option<u32>,
) {
    let start = stack.len();
    stack.extend(
        parent
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|child| {
                let component = if child.has_tag_name("entry") {
                    parse_u32(child.attribute("componentid")).or(inherited)
                } else {
                    inherited
                };
                (child, component)
            }),
    );
    // Pop the first child first.
    stack[start..].reverse();
}
