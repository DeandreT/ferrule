//! Text and layout inside generic `element()` content. The content of an
//! undeclared element is unconstrained character data, so the reader keeps
//! every text node and the writer adds no indentation of its own.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{from_str, to_string, xsd};
use ir::{
    Instance, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_MIXED_CONTENT_FIELD, XML_NODE_NAME_FIELD,
    XML_TEXT_FIELD,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xml-generic-whitespace-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn import_content(content: &str, mixed: bool) -> Result<SchemaNode, Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.0.join("root.xsd");
    std::fs::write(
        &path,
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root"><xs:complexType mixed="{mixed}"><xs:sequence>
    {content}
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )?;
    Ok(xsd::import_root(&path, Some("Root"))?)
}

/// `Root` holds a typed `Head`, a repeating lax wildcard, then an optional
/// typed `Tail` group, with the root content optionally declared mixed.
fn import(mixed: bool) -> Result<SchemaNode, Box<dyn Error>> {
    import_content(
        r#"<xs:element name="Head" type="xs:string"/>
    <xs:any processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
    <xs:element name="Tail" minOccurs="0"><xs:complexType><xs:sequence>
      <xs:element name="Part" type="xs:string" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType></xs:element>"#,
        mixed,
    )
}

/// Reads `input`, writes it, and requires the first re-read to be equal.
fn round_trip(schema: &SchemaNode, input: &str) -> Result<(Instance, String), Box<dyn Error>> {
    let instance = from_str(input, schema)?;
    let output = to_string(schema, &instance)?;
    assert_eq!(from_str(&output, schema)?, instance, "{output}");
    Ok((instance, output))
}

fn generic_item(instance: &Instance) -> Result<&Instance, Box<dyn Error>> {
    Ok(instance
        .field(XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .and_then(<[Instance]>::first)
        .ok_or("missing generic element item")?)
}

/// The text items of an element's ordered content stream.
fn stream_texts(instance: &Instance) -> Vec<&str> {
    instance
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .filter(|item| {
            item.field(XML_NODE_NAME_FIELD) == Some(&Instance::Scalar(Value::String(String::new())))
        })
        .filter_map(|item| match item.field(XML_TEXT_FIELD) {
            Some(Instance::Scalar(Value::String(text))) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn text_field(instance: &Instance) -> Option<&Value> {
    instance.field(XML_TEXT_FIELD).and_then(Instance::as_scalar)
}

#[test]
fn element_only_generic_content_round_trips_on_the_first_pass() -> Result<(), Box<dyn Error>> {
    let extra =
        r#"<Extra a="1"><Child b="2"><Leaf>v</Leaf><Leaf></Leaf></Child><Child>w</Child></Extra>"#;
    let schema = import(false)?;
    let (instance, output) = round_trip(
        &schema,
        &format!("<Root><Head>h</Head>{extra}<Tail><Part>p</Part><Part>q</Part></Tail></Root>"),
    )?;
    assert!(stream_texts(generic_item(&instance)?).is_empty());
    // Typed content keeps its indentation; generic content gains none.
    assert_eq!(
        output,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Root>
  <Head>h</Head>
  {extra}
  <Tail>
    <Part>p</Part>
    <Part>q</Part>
  </Tail>
</Root>"#
        )
    );

    let schema = import_content(
        r#"<xs:any processContents="skip" minOccurs="0" maxOccurs="unbounded"/>"#,
        false,
    )?;
    let (instance, output) = round_trip(&schema, &format!("<Root>{extra}{extra}</Root>"))?;
    assert!(stream_texts(generic_item(&instance)?).is_empty());
    assert!(
        output.contains(&format!("\n  {extra}\n  {extra}\n")),
        "{output}"
    );
    Ok(())
}

#[test]
fn indentation_read_from_generic_content_is_written_back_exactly() -> Result<(), Box<dyn Error>> {
    let schema = import(false)?;
    let layout = "<Extra>\n    <Child>v</Child>\n    <Child>w</Child>\n  </Extra>";
    let (instance, output) = round_trip(&schema, &format!("<Root><Head>h</Head>{layout}</Root>"))?;
    assert_eq!(
        stream_texts(generic_item(&instance)?),
        ["\n    ", "\n    ", "\n  "]
    );
    assert!(output.contains(layout), "{output}");
    assert_eq!(to_string(&schema, &from_str(&output, &schema)?)?, output);
    Ok(())
}

#[test]
fn generic_mixed_content_keeps_its_text_exactly() -> Result<(), Box<dyn Error>> {
    let schema = import(false)?;
    let content = "<Extra>a <B>b</B><C/>  c\n\t<D>d <E>e</E></D> </Extra>";
    let (instance, output) = round_trip(&schema, &format!("<Root><Head>h</Head>{content}</Root>"))?;
    let item = generic_item(&instance)?;
    assert_eq!(stream_texts(item), ["a ", "  c\n\t", " "]);
    assert!(
        output.contains("<Extra>a <B>b</B><C></C>  c\n\t<D>d <E>e</E></D> </Extra>"),
        "{output}"
    );
    Ok(())
}

#[test]
fn whitespace_only_generic_content_follows_the_text_field_rule() -> Result<(), Box<dyn Error>> {
    let schema = import(false)?;
    // Spaces without a line break are text.
    let (instance, output) = round_trip(&schema, "<Root><Head>h</Head><Extra> </Extra></Root>")?;
    let item = generic_item(&instance)?;
    assert_eq!(text_field(item), Some(&Value::String(" ".into())));
    assert!(output.contains("<Extra> </Extra>"), "{output}");
    // Whitespace with a line break and no child element is layout.
    let (instance, output) = round_trip(&schema, "<Root><Head>h</Head><Extra>\n  </Extra></Root>")?;
    let item = generic_item(&instance)?;
    assert_eq!(text_field(item), Some(&Value::String(String::new())));
    assert!(output.contains("<Extra></Extra>"), "{output}");
    Ok(())
}

#[test]
fn declared_mixed_parents_keep_their_layout() -> Result<(), Box<dyn Error>> {
    let schema = import(true)?;
    let (instance, output) = round_trip(
        &schema,
        "<Root>a<Head>h</Head>b<Extra x=\"1\"><Child>v</Child></Extra>c</Root>",
    )?;
    assert_eq!(stream_texts(&instance), ["a", "b", "c"]);
    assert_eq!(
        output,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root>a<Head>h</Head>b<Extra x=\"1\"><Child>v</Child></Extra>c</Root>"
    );
    Ok(())
}
