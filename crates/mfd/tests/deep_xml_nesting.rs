//! A design or schema nested far deeper than any real one must return a
//! result. The XML parser and the XSD importer once recursed per level and
//! aborted the whole process.

use std::sync::mpsc;
use std::time::Duration;

use mfd::{MfdPackage, import_package};

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
