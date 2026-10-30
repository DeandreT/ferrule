//! Walks the `xs:include` and `xs:import` graph of schema documents.
//!
//! Each walk visits documents depth first in document order, exactly as
//! recursion over each document's links would, so the same documents are
//! resolved and read in the same order. The pending links live on the heap,
//! so a chain of thousands of include files cannot exhaust the stack.

use std::path::PathBuf;

use roxmltree::Node;

/// One `xs:include` or `xs:import`, owned so its document can be dropped
/// while the link waits on the walk stack.
pub(super) struct SchemaLink {
    pub include: bool,
    /// The `namespace` attribute of an import.
    pub namespace: Option<String>,
    pub location: Option<String>,
}

/// A visited schema document whose links remain to be followed.
pub(super) struct LinkedSchema {
    /// The path that link locations resolve against.
    pub path: PathBuf,
    /// The document's target namespace, or the one it adopted when included.
    pub effective_namespace: Option<String>,
    links: Vec<SchemaLink>,
}

impl LinkedSchema {
    pub(super) fn new(schema: &Node, path: PathBuf, effective_namespace: Option<&str>) -> Self {
        let links = schema
            .children()
            .filter(|node| {
                node.is_element() && matches!(node.tag_name().name(), "include" | "import")
            })
            .map(|link| SchemaLink {
                include: link.tag_name().name() == "include",
                namespace: link.attribute("namespace").map(str::to_string),
                location: link.attribute("schemaLocation").map(str::to_string),
            })
            .collect();
        Self {
            path,
            effective_namespace: effective_namespace.map(str::to_string),
            links,
        }
    }

    /// The namespace a document reached through `link` inherits: an include
    /// adopts this document's namespace, an import keeps its own.
    pub(super) fn inherited_namespace(&self, link: &SchemaLink) -> Option<&str> {
        link.include
            .then_some(self.effective_namespace.as_deref())
            .flatten()
    }
}

/// What following one link produced.
pub(super) enum Follow<T> {
    /// Nothing to descend into: the link was skipped, unreadable, or led to
    /// a document already visited.
    Skip,
    /// A newly visited document whose links come next.
    Descend(LinkedSchema),
    /// The walk's answer; no further link is followed.
    Found(T),
}

/// Follows every link below `root`, depth first in document order.
pub(super) fn walk<T>(
    root: LinkedSchema,
    mut follow: impl FnMut(&LinkedSchema, &SchemaLink) -> Follow<T>,
) -> Option<T> {
    let mut stack = vec![(root, 0usize)];
    while let Some((schema, next)) = stack.last_mut() {
        let Some(link) = schema.links.get(*next) else {
            stack.pop();
            continue;
        };
        *next += 1;
        match follow(schema, link) {
            Follow::Skip => {}
            Follow::Descend(dependency) => stack.push((dependency, 0)),
            Follow::Found(found) => return Some(found),
        }
    }
    None
}
