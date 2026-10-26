use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use format_edi_config::{self as config, EdiConfigByteReader};

const ENVELOPE: &str = r#"<Config><Format standard="X12"/><Include href="Fields.Segment"/><Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded"><Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded"><Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/><Segment ref="GE" minOccurs="0"/></Group><Segment ref="IEA" minOccurs="0"/></Group></Group></Config>"#;
const MESSAGE: &str = r#"<Config><Format standard="X12"/><Include href="Fields.Segment"/><Message><MessageType>850</MessageType><Group name="Message_850" maxOccurs="unbounded"><Segment ref="ST"/><Segment ref="BEG"/><Segment ref="SE"/></Group></Message></Config>"#;
const FIELDS: &str = r#"<Config><Elements><Data name="F1" type="string"/><Data name="F143" type="string"/><Data name="F999" type="integer"/><Segment name="ISA"><Data ref="F1"/></Segment><Segment name="GS"><Data ref="F1"/></Segment><Segment name="ST"><Data ref="F143"/></Segment><Segment name="BEG"><Data ref="F999"/></Segment><Segment name="SE"><Data ref="F1"/></Segment><Segment name="GE"><Data ref="F1"/></Segment><Segment name="IEA"><Data ref="F1"/></Segment></Elements></Config>"#;

struct Reader {
    files: RefCell<BTreeMap<PathBuf, Arc<[u8]>>>,
    calls: RefCell<Vec<PathBuf>>,
}
impl EdiConfigByteReader for Reader {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.calls.borrow_mut().push(path.to_path_buf());
        let bytes = self.files.borrow().get(path).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "identity not supplied")
        })?;
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), Arc::from(&b"changed after read"[..]));
        Ok(bytes)
    }
}

fn source(root: &Path, envelope: &str, message_name: &str) -> Rc<Reader> {
    Rc::new(Reader {
        files: RefCell::new(BTreeMap::from([
            (root.join("Envelope.Config"), Arc::from(envelope.as_bytes())),
            (root.join("Fields.Segment"), Arc::from(FIELDS.as_bytes())),
            (root.join(message_name), Arc::from(MESSAGE.as_bytes())),
        ])),
        calls: RefCell::default(),
    })
}

#[test]
fn repeated_edi_reads_consume_the_same_supplied_bytes_without_path_reopens()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-edi-byte-identities");
    let source = source(&root, ENVELOPE, "850.Config");
    let config = config::import_config_with_byte_reader(
        &root.join("Envelope.Config"),
        &root,
        &["850".into()],
        source.clone(),
    )?;
    assert!(
        config
            .schema
            .child("Interchange")
            .and_then(|n| n.child("Group"))
            .and_then(|n| n.child("Message_850"))
            .is_some()
    );
    assert_eq!(source.calls.borrow().len(), 3);
    assert_eq!(
        source
            .calls
            .borrow()
            .iter()
            .filter(|p| p.ends_with("Envelope.Config"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn supplied_edi_does_not_discover_message_files_by_content()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-edi-byte-identities");
    let source = source(&root, ENVELOPE, "trap.Config");
    assert!(
        config::import_config_with_byte_reader(
            &root.join("Envelope.Config"),
            &root,
            &["850".into()],
            source.clone()
        )
        .is_err()
    );
    assert!(
        source
            .calls
            .borrow()
            .iter()
            .all(|p| !p.ends_with("trap.Config"))
    );
    Ok(())
}

#[test]
fn escaping_edi_include_is_refused_before_host_read() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-edi-byte-identities");
    let envelope = ENVELOPE.replace("Fields.Segment", "../outside.Segment");
    let source = source(&root, &envelope, "850.Config");
    assert!(matches!(
        config::import_config_with_byte_reader(
            &root.join("Envelope.Config"),
            &root,
            &["850".into()],
            source.clone()
        ),
        Err(config::ConfigError::Invalid(message)) if message.contains("escapes its authorized root")
    ));
    assert_eq!(&*source.calls.borrow(), &[root.join("Envelope.Config")]);
    Ok(())
}

#[test]
fn supplied_limits_keep_typed_failures_and_validation_order()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?.join("authored-edi-byte-identities");
    let source = source(&root, ENVELOPE, "850.Config");
    assert!(matches!(
        config::import_config_with_byte_reader(
            &root.join("Envelope.Config"),
            Path::new("not-absolute"),
            &vec!["850".into(); 33],
            source.clone(),
        ),
        Err(config::ConfigError::Limit("selected message count"))
    ));
    assert!(source.calls.borrow().is_empty());

    source.files.borrow_mut().insert(
        root.join("Envelope.Config"),
        Arc::from(vec![b' '; 8 * 1024 * 1024 + 1]),
    );
    assert!(matches!(
        config::import_config_with_byte_reader(
            &root.join("Envelope.Config"),
            &root,
            &["850".into()],
            source.clone(),
        ),
        Err(config::ConfigError::Limit("total input size"))
    ));
    assert_eq!(&*source.calls.borrow(), &[root.join("Envelope.Config")]);
    Ok(())
}
