//! Layout of declared mixed content. The reader keeps every text node of a
//! mixed parent, so the writer adds no indentation inside one; element-only
//! content keeps the writer's indentation.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{dtd, from_str, to_string, xsd};
use ir::{
    Instance, SchemaNode, Value, XML_MIXED_CONTENT_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};

const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xml-declared-mixed-layout-{}-{}",
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

fn import_xsd(content: &str, mixed: bool) -> Result<SchemaNode, Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.0.join("root.xsd");
    std::fs::write(
        &path,
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root"><xs:complexType mixed="{mixed}">
    {content}
  </xs:complexType></xs:element>
</xs:schema>"#
        ),
    )?;
    Ok(xsd::import_root(&path, Some("Root"))?)
}

/// `Root` holds repeating typed `Head` elements, then an optional
/// element-only `Tail` group.
fn sequence_schema(mixed: bool) -> Result<SchemaNode, Box<dyn Error>> {
    import_xsd(
        r#"<xs:sequence>
      <xs:element name="Head" type="xs:string" maxOccurs="unbounded"/>
      <xs:element name="Tail" minOccurs="0"><xs:complexType><xs:sequence>
        <xs:element name="Part" type="xs:string" maxOccurs="unbounded"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence>"#,
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

/// Requires `input` to be written back exactly after the XML declaration.
fn assert_written_back(schema: &SchemaNode, input: &str) -> Result<Instance, Box<dyn Error>> {
    let (instance, output) = round_trip(schema, input)?;
    assert_eq!(output, format!("{DECLARATION}{input}"));
    Ok(instance)
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

#[test]
fn declared_mixed_content_starting_with_an_element_round_trips() -> Result<(), Box<dyn Error>> {
    let schema = sequence_schema(true)?;
    let instance = assert_written_back(&schema, "<Root><Head>1</Head>b<Head>2</Head></Root>")?;
    assert_eq!(stream_texts(&instance), ["b"]);
    let instance = assert_written_back(&schema, "<Root><Head>1</Head></Root>")?;
    assert!(stream_texts(&instance).is_empty());
    Ok(())
}

#[test]
fn declared_mixed_content_with_adjacent_elements_round_trips() -> Result<(), Box<dyn Error>> {
    let schema = sequence_schema(true)?;
    let instance = assert_written_back(
        &schema,
        "<Root>a<Head>1</Head><Head>2</Head><Head>3</Head>c</Root>",
    )?;
    assert_eq!(stream_texts(&instance), ["a", "c"]);
    let instance = assert_written_back(&schema, "<Root><Head>1</Head><Head>2</Head></Root>")?;
    assert!(stream_texts(&instance).is_empty());
    Ok(())
}

#[test]
fn element_only_children_of_a_mixed_parent_gain_no_indentation() -> Result<(), Box<dyn Error>> {
    // Each element item keeps its string value, so indentation inside `Tail`
    // would come back as text in the parent's stream.
    let schema = sequence_schema(true)?;
    let instance = assert_written_back(
        &schema,
        "<Root><Head>1</Head><Tail><Part>p</Part><Part>q</Part></Tail></Root>",
    )?;
    assert!(stream_texts(&instance).is_empty());
    Ok(())
}

#[test]
fn declared_mixed_text_and_whitespace_are_written_back_exactly() -> Result<(), Box<dyn Error>> {
    let schema = sequence_schema(true)?;
    let input = "<Root>\n  <Head>1</Head>  \t<Head>2</Head>\n  b <Head>3</Head>\n</Root>";
    let instance = assert_written_back(&schema, input)?;
    assert_eq!(stream_texts(&instance), ["\n  ", "  \t", "\n  b ", "\n"]);
    Ok(())
}

#[test]
fn dtd_mixed_content_round_trips() -> Result<(), Box<dyn Error>> {
    let schema = dtd::import_root_str(
        "<!ELEMENT Root (#PCDATA | Bold | Italic)*>
         <!ELEMENT Bold (#PCDATA)>
         <!ELEMENT Italic (#PCDATA)>",
        Some("Root"),
    )?;
    let instance = assert_written_back(
        &schema,
        "<Root><Bold>x</Bold><Italic>y</Italic> z <Bold>w</Bold></Root>",
    )?;
    assert_eq!(stream_texts(&instance), [" z "]);
    Ok(())
}

#[test]
fn dtd_any_content_round_trips() -> Result<(), Box<dyn Error>> {
    let schema = dtd::import_root_str("<!ELEMENT Root ANY>", Some("Root"))?;
    let instance = assert_written_back(
        &schema,
        r#"<Root><Local a="1">x</Local><Other><Leaf>y</Leaf></Other>tail<Local></Local></Root>"#,
    )?;
    assert_eq!(stream_texts(&instance), ["tail"]);
    let instance = assert_written_back(
        &schema,
        "<Root> <Local>x</Local>\n\t<Local>y</Local></Root>",
    )?;
    assert_eq!(stream_texts(&instance), [" ", "\n\t"]);
    Ok(())
}

#[test]
fn element_only_typed_content_keeps_its_indentation() -> Result<(), Box<dyn Error>> {
    let schema = sequence_schema(false)?;
    let (_, output) = round_trip(
        &schema,
        "<Root><Head>1</Head><Head>2</Head><Tail><Part>p</Part><Part>q</Part></Tail></Root>",
    )?;
    assert_eq!(
        output,
        format!(
            "{DECLARATION}<Root>\n  <Head>1</Head>\n  <Head>2</Head>\n  <Tail>\n    <Part>p</Part>\n    <Part>q</Part>\n  </Tail>\n</Root>"
        )
    );

    // A repeating choice retains element order without text.
    let schema = import_xsd(
        r#"<xs:choice maxOccurs="unbounded">
      <xs:element name="A" type="xs:string"/>
      <xs:element name="B" type="xs:string"/>
    </xs:choice>"#,
        false,
    )?;
    let (instance, output) = round_trip(&schema, "<Root><A>1</A><B>2</B><A>3</A></Root>")?;
    assert!(instance.field(XML_MIXED_CONTENT_FIELD).is_some());
    assert_eq!(
        output,
        format!("{DECLARATION}<Root>\n  <A>1</A>\n  <B>2</B>\n  <A>3</A></Root>")
    );

    // An element-only parent stays indented around a mixed child.
    let schema = import_xsd(
        r#"<xs:sequence>
      <xs:element name="Head" type="xs:string"/>
      <xs:element name="Note"><xs:complexType mixed="true"><xs:sequence>
        <xs:element name="B" type="xs:string" maxOccurs="unbounded"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence>"#,
        false,
    )?;
    let (_, output) = round_trip(
        &schema,
        "<Root><Head>1</Head><Note><B>x</B>y<B>z</B><B>w</B></Note></Root>",
    )?;
    assert_eq!(
        output,
        format!(
            "{DECLARATION}<Root>\n  <Head>1</Head>\n  <Note><B>x</B>y<B>z</B><B>w</B></Note>\n</Root>"
        )
    );
    Ok(())
}

/// `instance` without the ordered content stream of the element at `path`,
/// as a mapping builds it from typed fields alone.
fn typed_fields(mut instance: Instance, path: &[&str]) -> Instance {
    let mut group = &mut instance;
    for name in path {
        let Instance::Group(fields) = group else {
            panic!("`{name}` has no parent group");
        };
        group = fields
            .iter_mut()
            .find_map(|(field, value)| (field.as_str() == *name).then_some(value))
            .unwrap_or_else(|| panic!("no field `{name}`"));
    }
    let Instance::Group(fields) = group else {
        panic!("the typed element is not a group");
    };
    fields.retain(|(name, _)| name != XML_MIXED_CONTENT_FIELD);
    instance
}

/// Writes the typed form of `input` and requires `expected` after the XML
/// declaration. Re-reading it on the first pass gives the same typed fields.
fn assert_typed_written(
    schema: &SchemaNode,
    input: &str,
    path: &[&str],
    expected: &str,
) -> Result<Instance, Box<dyn Error>> {
    let typed = typed_fields(from_str(input, schema)?, path);
    let output = to_string(schema, &typed)?;
    assert_eq!(output, format!("{DECLARATION}{expected}"));
    let reread = from_str(&output, schema)?;
    assert_eq!(typed_fields(reread.clone(), path), typed, "{output}");
    Ok(reread)
}

#[test]
fn typed_mixed_parent_gets_no_added_layout() -> Result<(), Box<dyn Error>> {
    let schema = sequence_schema(true)?;
    let elements =
        "<Root><Head>1</Head><Head>2</Head><Tail><Part>p</Part><Part>q</Part></Tail></Root>";
    let reread = assert_typed_written(&schema, elements, &[], elements)?;
    assert!(stream_texts(&reread).is_empty());

    // Typed text is written before the typed children, and is the only text
    // that reads back.
    let reread = assert_typed_written(
        &schema,
        "<Root>a<Head>1</Head><Head>2</Head></Root>",
        &[],
        "<Root>a<Head>1</Head><Head>2</Head></Root>",
    )?;
    assert_eq!(stream_texts(&reread), ["a"]);
    Ok(())
}

#[test]
fn element_only_parent_stays_indented_around_a_typed_mixed_child() -> Result<(), Box<dyn Error>> {
    let schema = import_xsd(
        r#"<xs:sequence>
      <xs:element name="Head" type="xs:string"/>
      <xs:element name="Note"><xs:complexType mixed="true"><xs:sequence>
        <xs:element name="B" type="xs:string" maxOccurs="unbounded"/>
        <xs:element name="Tail" minOccurs="0"><xs:complexType><xs:sequence>
          <xs:element name="Part" type="xs:string" maxOccurs="unbounded"/>
        </xs:sequence></xs:complexType></xs:element>
      </xs:sequence></xs:complexType></xs:element>
      <xs:element name="End" type="xs:string"/>
    </xs:sequence>"#,
        false,
    )?;
    let reread = assert_typed_written(
        &schema,
        "<Root><Head>1</Head><Note><B>x</B><B>z</B><Tail><Part>p</Part></Tail></Note><End>e</End></Root>",
        &["Note"],
        "<Root>\n  <Head>1</Head>\n  <Note><B>x</B><B>z</B><Tail><Part>p</Part></Tail></Note>\n  <End>e</End>\n</Root>",
    )?;
    let note = reread.field("Note").ok_or("no Note")?;
    assert!(stream_texts(note).is_empty());
    Ok(())
}

#[test]
fn typed_dtd_mixed_and_any_parents_get_no_added_layout() -> Result<(), Box<dyn Error>> {
    let schema = dtd::import_root_str(
        "<!ELEMENT Root (#PCDATA | Bold | Italic)*>
         <!ELEMENT Bold (#PCDATA)>
         <!ELEMENT Italic (#PCDATA)>",
        Some("Root"),
    )?;
    let reread = assert_typed_written(
        &schema,
        "<Root><Bold>x</Bold><Italic>y</Italic><Bold>w</Bold></Root>",
        &[],
        "<Root><Bold>x</Bold><Bold>w</Bold><Italic>y</Italic></Root>",
    )?;
    assert!(stream_texts(&reread).is_empty());

    let schema = dtd::import_root_str("<!ELEMENT Root ANY>", Some("Root"))?;
    let elements =
        r#"<Root><Local a="1">x</Local><Other><Leaf>y</Leaf></Other><Local></Local></Root>"#;
    let reread = assert_typed_written(&schema, elements, &[], elements)?;
    assert!(stream_texts(&reread).is_empty());
    Ok(())
}
