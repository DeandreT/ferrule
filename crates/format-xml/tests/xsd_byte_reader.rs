use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use format_xml::xsd::{self, XmlSchemaByteReader};
use ir::{ScalarType, SchemaKind};

const ROOT: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:b="urn:bytes" targetNamespace="urn:bytes"><xs:include schemaLocation="types.xsd"/><xs:element name="Packet" type="b:PacketType"/></xs:schema>"#;
const TYPES: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:bytes"><xs:complexType name="PacketType"><xs:sequence><xs:element name="Quantity" type="xs:int"/></xs:sequence></xs:complexType></xs:schema>"#;

struct Reader {
    files: RefCell<BTreeMap<PathBuf, Arc<[u8]>>>,
    calls: RefCell<Vec<PathBuf>>,
    mutate_after_read: bool,
}

impl XmlSchemaByteReader for Reader {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.calls.borrow_mut().push(path.to_path_buf());
        let bytes = self.files.borrow().get(path).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "unapproved identity")
        })?;
        if self.mutate_after_read {
            // A reopening reader would observe this, not the returned Arc.
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), Arc::from(&b"replacement, not XML"[..]));
        }
        Ok(bytes)
    }
}

fn source(root: &Path, main: &str, mutate: bool) -> Rc<Reader> {
    Rc::new(Reader {
        files: RefCell::new(BTreeMap::from([
            (root.join("input.xsd"), Arc::from(main.as_bytes())),
            (root.join("types.xsd"), Arc::from(TYPES.as_bytes())),
        ])),
        calls: RefCell::default(),
        mutate_after_read: mutate,
    })
}

#[test]
fn immutable_root_and_include_are_the_only_parser_inputs() -> Result<(), Box<dyn std::error::Error>>
{
    // There is deliberately no filesystem catalog at this identity. The public
    // reader API must not canonicalize/open it behind the supplied source.
    let root = std::env::current_dir()?.join("authored-byte-reader-only-root");
    let source = source(&root, ROOT, true);
    let schema = xsd::import_root_with_byte_reader(
        &root.join("input.xsd"),
        Some("Packet"),
        &root,
        source.clone(),
    )?;
    assert_eq!(
        schema.child("Quantity").map(|n| &n.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );
    assert_eq!(source.calls.borrow().len(), 2);
    assert_eq!(
        source
            .calls
            .borrow()
            .iter()
            .filter(|p| p.ends_with("types.xsd"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn escaped_dependency_never_reaches_the_host_reader_and_is_sticky()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-byte-reader-only-root");
    let text = ROOT.replace("types.xsd", "../outside.xsd");
    let source = source(&root, &text, false);
    let result = xsd::import_root_with_byte_reader(
        &root.join("input.xsd"),
        Some("Packet"),
        &root,
        source.clone(),
    );
    assert!(result.is_err());
    assert_eq!(&*source.calls.borrow(), &[root.join("input.xsd")]);
    Ok(())
}

#[test]
fn named_type_and_base_queries_have_supplied_reader_entrypoints()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-byte-reader-only-root");
    let source = source(&root, ROOT, false);
    let schema = xsd::import_type_with_byte_reader(
        &root.join("input.xsd"),
        "{urn:bytes}PacketType",
        &root,
        source.clone(),
    )?;
    assert_eq!(
        schema.child("Quantity").map(|n| &n.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );
    let base = xsd::import_type_base_with_byte_reader(
        &root.join("input.xsd"),
        "{urn:bytes}PacketType",
        &root,
        source,
    )?;
    assert_eq!(base, None);
    Ok(())
}

#[test]
fn raw_utf16_decoding_is_separate_from_resource_loading() -> Result<(), Box<dyn std::error::Error>>
{
    let mut raw = vec![0xff, 0xfe];
    for unit in ROOT.encode_utf16() {
        raw.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(xsd::decode_schema_bytes(&raw, raw.len() as u64)?, ROOT);
    assert!(xsd::decode_schema_bytes(&raw, raw.len() as u64 - 1).is_err());
    Ok(())
}
