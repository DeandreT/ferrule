use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_isbn_service_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Books"><xs:complexType><xs:sequence><xs:element name="Book" maxOccurs="unbounded"><xs:complexType><xs:attribute name="ISBN10" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Books"><xs:complexType><xs:sequence><xs:element name="Book" maxOccurs="unbounded"><xs:complexType><xs:attribute name="ISBN13" type="xs:string"/><xs:attribute name="EAN13" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Books"><entry name="Book" outkey="10"><entry name="ISBN10" type="attribute" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Books"/></data></component>
  <component name="convertToISBN13" library="IsbnConverterService" kind="20"><data><root><entry name="request"><entry name="isbn10" inpkey="20"/></entry></root><root><entry name="response" outkey="21"/></root></data></component>
  <component name="convertToEAN" library="IsbnConverterService" kind="20"><data><root><entry name="request"><entry name="isbn10" inpkey="22"/></entry></root><root><entry name="response" outkey="23"/></root></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Books"><entry name="Book" inpkey="30"><entry name="ISBN13" type="attribute" inpkey="31"/><entry name="EAN13" type="attribute" inpkey="32"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Books"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn isbn_service_calls_lower_to_local_checked_conversion() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let input = Instance::Group(vec![(
        "Book".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "ISBN10".into(),
            Instance::Scalar(Value::String("0-7645-4964-2".into())),
        )])]),
    )]);
    let output = engine::run(&imported.project, &input)?;
    let Some(book) = output
        .field("Book")
        .and_then(Instance::as_repeated)
        .and_then(|books| books.first())
    else {
        panic!("converted output must contain one book");
    };
    for field in ["ISBN13", "EAN13"] {
        assert_eq!(
            book.field(field).and_then(Instance::as_scalar),
            Some(&Value::String("9780764549649".into()))
        );
    }
    Ok(())
}
