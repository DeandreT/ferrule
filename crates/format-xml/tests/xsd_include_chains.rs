//! Schema documents reached through long `xs:include` and `xs:import` chains
//! import like short ones. Following the links once recursed per document,
//! and a chain of a few thousand files overflowed the stack. Each check runs on
//! a thread with the 8 MiB stack of a default main thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use format_xml::{XmlFormatError, xsd};
use ir::{ScalarType, SchemaKind, SchemaNode};

const LONG_CHAIN: usize = 10_000;
const XS: &str = r#"xmlns:xs="http://www.w3.org/2001/XMLSchema""#;

/// Removes the per-test schema directory on drop, including after a panic.
struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_include_chain_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

/// The declarations every include chain ends in: a named type, a type
/// derived from it, and a global element. Their references are unprefixed,
/// so they adopt the namespace of the schema that includes them.
const CHAIN_END: &str = r#"
  <xs:complexType name="Line"><xs:sequence>
    <xs:element name="Sku" type="xs:string"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="SpecialLine"><xs:complexContent>
    <xs:extension base="Line"><xs:sequence>
      <xs:element name="Rush" type="xs:boolean"/>
    </xs:sequence></xs:extension>
  </xs:complexContent></xs:complexType>
  <xs:element name="Note" type="xs:string"/>"#;

/// A member of the root's substitution group, at the end of every chain.
const REMARK: &str =
    r#"<xs:element name="Remark" type="SpecialLine" substitutionGroup="Comment"/>"#;

/// Writes `s0.xsd` through `s{files - 1}.xsd`, each including the next,
/// with the shared declarations in the last. Only the root declares the
/// target namespace, so every other file is a chameleon include. With
/// `back_edge`, the last file also includes the root, closing a cycle.
fn include_chain(fixture: &Fixture, files: usize, back_edge: bool) -> PathBuf {
    for index in 1..files {
        let body = if index + 1 < files {
            format!(r#"<xs:include schemaLocation="s{}.xsd"/>"#, index + 1)
        } else if back_edge {
            format!(r#"<xs:include schemaLocation="s0.xsd"/>{CHAIN_END}{REMARK}"#)
        } else {
            format!("{CHAIN_END}{REMARK}")
        };
        fixture.write(
            &format!("s{index}.xsd"),
            &format!(r#"<xs:schema {XS} xmlns:c="urn:chain">{body}</xs:schema>"#),
        );
    }
    fixture.write(
        "s0.xsd",
        &root_schema(r#"<xs:include schemaLocation="s1.xsd"/>"#),
    )
}

fn root_schema(links: &str) -> String {
    format!(
        r#"<xs:schema {XS} xmlns:c="urn:chain" targetNamespace="urn:chain" elementFormDefault="qualified">
  {links}
  <xs:element name="Comment" type="c:Line" abstract="true"/>
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Item" type="c:Line" maxOccurs="unbounded"/>
    <xs:element ref="c:Note"/>
    <xs:element ref="c:Comment"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
    )
}

/// Runs `import` on a thread with a default main thread's stack.
fn on_main_thread_stack<T: Send + 'static>(import: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(import)
        .unwrap()
        .join()
        .expect("the import overflowed its stack or panicked")
}

/// Every way to import the chain's root element and its named type.
fn import_all(root: &Path) -> Vec<Result<SchemaNode, XmlFormatError>> {
    let directory = root.parent().unwrap();
    vec![
        xsd::import_root(root, Some("Input")),
        xsd::import_root_with_resource_root(root, Some("Input"), directory),
        xsd::import_type(root, "{urn:chain}Line"),
        xsd::import_type_with_resource_root(root, "{urn:chain}SpecialLine", directory),
    ]
}

fn alternatives(node: &SchemaNode) -> Vec<&str> {
    match &node.kind {
        SchemaKind::Group { alternatives, .. } => alternatives
            .iter()
            .map(|alternative| alternative.name.as_str())
            .collect(),
        _ => Vec::new(),
    }
}

/// The root element resolved the type, the derived type, the global element
/// and the substitution group member declared at the far end of the chain.
fn assert_imports_the_chain_end(schema: &SchemaNode, member: &str) {
    let item = schema.child("Item").unwrap();
    assert!(item.repeating && item.child("Sku").is_some(), "{item:#?}");
    assert_eq!(
        alternatives(item),
        ["{urn:chain}Line", "{urn:chain}SpecialLine"]
    );
    assert!(
        matches!(
            schema.child("Note").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ),
        "{schema:#?}"
    );
    assert_eq!(alternatives(schema.child("Comment").unwrap()), [member]);
}

#[test]
fn a_ten_thousand_file_include_chain_imports_like_a_short_one() {
    for back_edge in [false, true] {
        let short = Fixture::new();
        let short_root = include_chain(&short, 2, back_edge);
        let expected = import_all(&short_root);

        let long = Fixture::new();
        let long_root = include_chain(&long, LONG_CHAIN, back_edge);
        let actual = on_main_thread_stack(move || import_all(&long_root));

        assert_eq!(
            format!("{actual:?}"),
            format!("{expected:?}"),
            "back edge: {back_edge}"
        );
        assert_imports_the_chain_end(actual[0].as_ref().unwrap(), "{urn:chain}Remark");
        assert!(actual.iter().all(Result::is_ok), "{actual:?}");
    }
}

#[test]
fn a_ten_thousand_file_import_chain_imports_like_a_short_one() {
    // Each file imports the next file's distinct namespace. Only the
    // substitution and derived-type indexes follow such a chain to its end.
    fn import_chain(fixture: &Fixture, files: usize) -> PathBuf {
        // The last namespace is the same for every length.
        let namespace = |index: usize| {
            if index + 1 == files {
                "urn:end".to_string()
            } else {
                format!("urn:n{index}")
            }
        };
        for index in 1..files {
            let body = if index + 1 < files {
                format!(
                    r#"<xs:import namespace="{}" schemaLocation="s{}.xsd"/>"#,
                    namespace(index + 1),
                    index + 1
                )
            } else {
                // Closes a cycle back to the root's namespace, where the
                // member's type is declared.
                r#"<xs:import namespace="urn:chain" schemaLocation="s0.xsd"/>
                <xs:element name="Remark" type="c:SpecialLine" substitutionGroup="c:Comment"/>"#
                    .to_string()
            };
            fixture.write(
                &format!("s{index}.xsd"),
                &format!(
                    r#"<xs:schema {XS} xmlns:c="urn:chain" targetNamespace="{}">{body}</xs:schema>"#,
                    namespace(index)
                ),
            );
        }
        fixture.write(
            "s0.xsd",
            &root_schema(&format!(
                r#"<xs:import namespace="{}" schemaLocation="s1.xsd"/>{CHAIN_END}"#,
                namespace(1)
            )),
        )
    }

    let short = Fixture::new();
    let expected = import_all(&import_chain(&short, 2));
    let long = Fixture::new();
    let long_root = import_chain(&long, LONG_CHAIN);
    let actual = on_main_thread_stack(move || import_all(&long_root));

    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert!(actual.iter().all(Result::is_ok), "{actual:?}");
    assert_imports_the_chain_end(actual[0].as_ref().unwrap(), "{urn:end}Remark");
}

#[test]
fn shared_and_cyclic_includes_resolve_once() {
    // The root reaches `shared.xsd` through two files, and `shared.xsd`
    // includes the root again.
    let fixture = Fixture::new();
    let root = fixture.write(
        "root.xsd",
        &root_schema(
            r#"<xs:include schemaLocation="left.xsd"/><xs:include schemaLocation="right.xsd"/>"#,
        ),
    );
    for side in ["left", "right"] {
        fixture.write(
            &format!("{side}.xsd"),
            &format!(r#"<xs:schema {XS}><xs:include schemaLocation="shared.xsd"/></xs:schema>"#),
        );
    }
    fixture.write(
        "shared.xsd",
        &format!(
            r#"<xs:schema {XS} xmlns:c="urn:chain"><xs:include schemaLocation="root.xsd"/>{CHAIN_END}{REMARK}</xs:schema>"#
        ),
    );

    let imported = on_main_thread_stack(move || import_all(&root));
    assert!(imported.iter().all(Result::is_ok), "{imported:?}");
    assert_imports_the_chain_end(imported[0].as_ref().unwrap(), "{urn:chain}Remark");
    let line = imported[2].as_ref().unwrap();
    assert_eq!(line.name, "Line");
    assert!(line.child("Sku").is_some(), "{line:#?}");
    let special = imported[3].as_ref().unwrap();
    assert!(special.child("Sku").is_some() && special.child("Rush").is_some());
}
