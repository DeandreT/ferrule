//! A design or schema nested far deeper than any real one must return a
//! result. The XML parser and the XSD importer once recursed per level and
//! aborted the whole process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ir::{SchemaKind, SchemaNode};
use mfd::{ImportProfile, MAX_MFD_ENTRY_DEPTH, MfdError, MfdPackage, import_package};

const LEVELS: usize = 200_000;

fn nested(open: &str, close: &str) -> String {
    let mut text = open.repeat(LEVELS);
    text.push_str(&close.repeat(LEVELS));
    text
}

#[test]
fn deeply_nested_design_is_refused() {
    let unclosed = "<a>".repeat(LEVELS);
    let closed = format!("<mapping>{}</mapping>", nested("<a>", "</a>"));
    for design in [unclosed, closed] {
        let package = MfdPackage::new("design.mfd", [("design.mfd", design.as_bytes())]).unwrap();
        assert!(import_package(&package).is_err());
    }
}

/// A long chain of plain elements inside a design component costs one step per
/// element in the root-view inventory, so the import finishes promptly. The
/// old per-element ancestor search was quadratic and would not.
#[test]
fn deep_component_chain_finishes_promptly() {
    const CHAIN: usize = 60_000;
    let design = format!(
        "<mapping version=\"22\"><component name=\"defaultmap\"><structure><children><component name=\"c\" library=\"xml\" uid=\"1\">{}</component></children><graph directed=\"1\"/></structure></component></mapping>",
        nested_chain(CHAIN)
    );
    let (sender, receiver) = mpsc::channel();
    // Debug frames are large; a big stack keeps any per-level recursion
    // elsewhere from masking the timing being tested.
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let package =
                MfdPackage::new("design.mfd", [("design.mfd", design.as_bytes())]).unwrap();
            let _ = sender.send(import_package(&package).map(|_| ()));
        })
        .unwrap();
    // The design has no target, so a refusal is expected; any result is fine.
    match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(_) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("the import did not finish in time"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the import panicked"),
    }
}

fn nested_chain(levels: usize) -> String {
    let mut text = "<n>".repeat(levels);
    text.push_str(&"</n>".repeat(levels));
    text
}

/// A packaged source schema whose named-type chain is far past the XSD
/// import depth limit falls back to the design's entry tree with a warning
/// instead of overflowing the stack.
#[test]
fn packaged_schema_past_the_xsd_depth_limit_falls_back_with_a_warning() {
    const TYPES: usize = 6_000;
    let mut schema = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog" type="t0"/>"#,
    );
    for index in 0..TYPES {
        schema.push_str(&format!(
            r#"<xs:complexType name="t{index}"><xs:sequence><xs:element name="Book" type="t{}"/></xs:sequence></xs:complexType>"#,
            index + 1
        ));
    }
    schema.push_str(&format!(
        r#"<xs:complexType name="t{TYPES}"><xs:sequence><xs:element name="Title" type="xs:string"/></xs:sequence></xs:complexType></xs:schema>"#
    ));
    let imported = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || {
            let package = MfdPackage::new(
                "books.mfd",
                [
                    ("books.mfd", include_bytes!("fixtures/books.mfd").as_slice()),
                    ("books-source.xsd", schema.as_bytes()),
                    (
                        "books-target.xsd",
                        include_bytes!("fixtures/books-target.xsd").as_slice(),
                    ),
                ],
            )
            .unwrap();
            import_package(&package)
        })
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("books-source.xsd")
                && warning.contains("nesting and reference depth limit")),
        "{:?}",
        imported.warnings
    );
}

/// A design whose source schema reaches its declarations through a long
/// chain of include files on disk imports them instead of overflowing the
/// stack.
#[test]
fn design_schema_behind_a_long_include_chain_imports() {
    const FILES: usize = 10_000;
    let directory =
        std::env::temp_dir().join(format!("ferrule_mfd_include_chain_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let xs = r#"xmlns:xs="http://www.w3.org/2001/XMLSchema""#;
    for index in 1..FILES {
        let body = if index + 1 < FILES {
            format!(r#"<xs:include schemaLocation="chain{}.xsd"/>"#, index + 1)
        } else {
            r#"<xs:complexType name="CatalogType"><xs:sequence><xs:element name="Book" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Title" type="xs:string"/></xs:sequence><xs:attribute name="isbn" type="xs:string"/><xs:attribute name="pages" type="xs:int"/></xs:complexType></xs:element></xs:sequence></xs:complexType>"#
                .to_string()
        };
        std::fs::write(
            directory.join(format!("chain{index}.xsd")),
            format!("<xs:schema {xs}>{body}</xs:schema>"),
        )
        .unwrap();
    }
    std::fs::write(
        directory.join("books-source.xsd"),
        format!(
            r#"<xs:schema {xs} elementFormDefault="qualified"><xs:include schemaLocation="chain1.xsd"/><xs:element name="Catalog" type="CatalogType"/></xs:schema>"#
        ),
    )
    .unwrap();
    std::fs::write(
        directory.join("books.mfd"),
        include_bytes!("fixtures/books.mfd"),
    )
    .unwrap();
    std::fs::write(
        directory.join("books-target.xsd"),
        include_bytes!("fixtures/books-target.xsd"),
    )
    .unwrap();
    let design = directory.join("books.mfd");
    let imported = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || mfd::import(&design))
        .unwrap()
        .join()
        .unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    let imported = imported.unwrap();
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.contains("books-source.xsd")),
        "{:?}",
        imported.warnings
    );
    let book = imported.project.source.child("Book").unwrap();
    assert!(book.repeating && book.child("Title").is_some(), "{book:#?}");
}

/// The entry trees the importers walk: typed `ferrule-kind` groups and plain
/// entries in an XML source, and object properties in a JSON target.
#[derive(Clone, Copy, Debug)]
enum EntryTree {
    Typed,
    PlainXml,
    Json,
}

impl EntryTree {
    const ALL: [Self; 3] = [Self::Typed, Self::PlainXml, Self::Json];

    /// A design whose deep component nests exactly `levels` `<entry>`
    /// elements, counting its wrappers.
    fn design(self, levels: usize) -> String {
        let (wrappers, open, close, leaf) = match self {
            Self::Typed => (
                vec!["FileInstance", "document"],
                r#"<entry name="n" ferrule-kind="group" ferrule-repeating="0">"#,
                "</entry>",
                r#"<entry name="Value" outkey="2" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>"#,
            ),
            Self::PlainXml => (
                vec!["FileInstance", "document"],
                r#"<entry name="n">"#,
                "</entry>",
                r#"<entry name="Value" outkey="2"/>"#,
            ),
            Self::Json => (
                vec!["FileInstance", "document", "root"],
                r#"<entry name="object"><entry name="n" type="json-property">"#,
                "</entry></entry>",
                r#"<entry name="string" inpkey="4"/>"#,
            ),
        };
        let per_level = open.matches("<entry").count();
        let mut remaining = levels - wrappers.len() - 1;
        // An odd count for the two-entry JSON levels gets one more wrapper.
        let pad = !remaining.is_multiple_of(per_level);
        remaining -= usize::from(pad);
        let mut tree = String::new();
        for wrapper in wrappers.iter().chain(pad.then_some(&"pad")) {
            tree.push_str(&format!(r#"<entry name="{wrapper}">"#));
        }
        tree.push_str(&open.repeat(remaining / per_level));
        tree.push_str(leaf);
        tree.push_str(&close.repeat(remaining / per_level));
        tree.push_str(&"</entry>".repeat(wrappers.len() + usize::from(pad)));

        let shallow_source = r#"<entry name="FileInstance"><entry name="document"><entry name="Value" outkey="2"/></entry></entry>"#;
        let shallow_target = r#"<entry name="FileInstance"><entry name="document"><entry name="Value" inpkey="4"/></entry></entry>"#;
        let (source, target, target_library) = match self {
            Self::Typed | Self::PlainXml => {
                (tree.as_str(), shallow_target, r#"library="xml" kind="14""#)
            }
            Self::Json => (shallow_source, tree.as_str(), r#"library="json" kind="31""#),
        };
        format!(
            r#"<mapping version="22"><component name="defaultmap" uid="1"><structure><children><component name="Input" library="xml" uid="2" kind="14"><data><root>{source}</root><document schema="missing-source.xsd" inputinstance="input.xml"/></data></component><component name="Output" {target_library} uid="3"><properties XSLTDefaultOutput="1"/><data><root>{target}</root></data></component></children><graph directed="1"><vertices><vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex></vertices></graph></structure></component></mapping>"#
        )
    }

    /// The imported schema depth of [`Self::design`] at `levels`.
    fn schema_depth(self, levels: usize) -> usize {
        match self {
            // The `FileInstance` and `document` wrappers are not schema levels.
            Self::Typed | Self::PlainXml => levels - 2,
            // The `root` group, then one per object property.
            Self::Json => 1 + (levels - 4) / 2,
        }
    }
}

fn on_main_thread_stack<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap()
}

fn assert_entry_depth_refusal<T>(result: Result<T, MfdError>) {
    match result {
        Err(MfdError::UnsupportedImport(reason)) => assert!(
            reason.contains(&format!("more than {MAX_MFD_ENTRY_DEPTH} levels deep")),
            "{reason}"
        ),
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("a design past the entry depth limit imported"),
    }
}

fn schema_depth(schema: &SchemaNode) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(schema, 1)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        if let SchemaKind::Group { children, .. } = &node.kind {
            stack.extend(children.iter().map(|child| (child, depth + 1)));
        }
    }
    deepest
}

/// Entry trees far deeper than the limit are refused before any component
/// is read, through file, pipeline and package import, instead of
/// overflowing the stack in the entry-tree walkers.
#[test]
fn entry_trees_past_the_depth_limit_are_refused() {
    on_main_thread_stack(|| {
        let directory = TempDir::new();
        for tree in EntryTree::ALL {
            let path = directory.0.join(format!("{tree:?}.mfd"));
            std::fs::write(&path, tree.design(LEVELS)).unwrap();
            assert_entry_depth_refusal(mfd::import(&path));
            assert_entry_depth_refusal(mfd::import_pipeline(&path));
        }
        // Package entries are limited to 4 MiB, so the packaged tree uses
        // bare `<entry>` elements.
        let design = format!(
            "<mapping><component><data><root>{}</root></data></component></mapping>",
            nested("<entry>", "</entry>")
        );
        let package = MfdPackage::new("design.mfd", [("design.mfd", design.as_bytes())]).unwrap();
        assert_entry_depth_refusal(import_package(&package));
    });
}

/// A tree at the limit imports and validates on an 8 MiB stack even in an
/// unoptimized build; one more level is refused.
#[test]
fn entry_trees_at_the_depth_limit_import() {
    on_main_thread_stack(|| {
        let directory = TempDir::new();
        for tree in EntryTree::ALL {
            let path = directory.0.join(format!("{tree:?}.mfd"));
            std::fs::write(&path, tree.design(MAX_MFD_ENTRY_DEPTH)).unwrap();
            let imported = mfd::import(&path).unwrap_or_else(|error| panic!("{tree:?}: {error}"));
            let project = &imported.project;
            let deep = match tree {
                EntryTree::Typed | EntryTree::PlainXml => &project.source,
                EntryTree::Json => &project.target,
            };
            assert_eq!(
                schema_depth(deep),
                tree.schema_depth(MAX_MFD_ENTRY_DEPTH),
                "{tree:?}"
            );
            let _ = engine::validate(project);
            let _ = mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                ImportProfile::Executable,
            );

            std::fs::write(&path, tree.design(MAX_MFD_ENTRY_DEPTH + 1)).unwrap();
            assert_entry_depth_refusal(mfd::import(&path));
        }
    });
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_deep_entries_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
