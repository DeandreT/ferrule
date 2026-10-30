//! Extra occurrences of single-occurrence declared elements that only an
//! element wildcard beside them can match.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{
    Instance, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NAMESPACE_URI_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
    XmlWildcardProcessContents,
};

const TARGET: &str = "urn:ferrule:wildcard-surplus:t";

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-wildcard-surplus-{}-{}",
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

fn import(schema: &str, root: &str) -> Result<SchemaNode, Box<dyn Error>> {
    Ok(import_set(schema, &[], root)?)
}

fn import_set(
    schema: &str,
    dependencies: &[(&str, &str)],
    root: &str,
) -> Result<SchemaNode, XmlFormatError> {
    let directory = TempDirectory::new()?;
    for (name, contents) in dependencies {
        std::fs::write(directory.0.join(name), contents)?;
    }
    let path = directory.0.join("root.xsd");
    std::fs::write(&path, schema)?;
    xsd::import_root(&path, Some(root))
}

fn assert_export_reimports(schema: &SchemaNode, root: &str) -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let exported = xsd::export_set(schema, "normalized.xsd")?;
    let path = directory.0.join("normalized.xsd");
    std::fs::write(&path, exported.root)?;
    for dependency in exported.dependencies {
        std::fs::write(directory.0.join(&dependency.filename), dependency.contents)?;
    }
    assert_eq!(&xsd::import_root(&path, Some(root))?, schema);
    Ok(())
}

fn round_trip(schema: &SchemaNode, input: &str) -> Result<Instance, Box<dyn Error>> {
    let instance = from_str(input, schema)?;
    let output = to_string(schema, &instance)?;
    assert_eq!(from_str(&output, schema)?, instance, "{output}");
    Ok(instance)
}

/// `Root` holds `Head`, a wildcard with the given attributes, then `trailer`.
fn unqualified_schema(globals: &str, wildcard: &str, trailer: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  {globals}
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any {wildcard}/>
    {trailer}
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
    )
}

const REPEATING_LAX: &str =
    r###"namespace="##any" processContents="lax" minOccurs="0" maxOccurs="unbounded""###;

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn generic_items(instance: &Instance) -> Result<&[Instance], Box<dyn Error>> {
    Ok(instance
        .field(XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing generic element items")?)
}

fn assert_generic(item: &Instance, name: &str, value: &str) {
    assert_eq!(item.field(XML_LOCAL_NAME_FIELD), Some(&text(name)));
    assert_eq!(
        item.field(XML_NAMESPACE_URI_FIELD),
        Some(&Instance::Scalar(Value::Null))
    );
    assert_eq!(item.field(XML_TEXT_FIELD), Some(&text(value)));
}

fn with_field(instance: &Instance, name: &str, value: Instance) -> Instance {
    let mut instance = instance.clone();
    if let Instance::Group(fields) = &mut instance
        && let Some((_, field)) = fields.iter_mut().find(|(field, _)| field == name)
    {
        *field = value;
    }
    instance
}

#[test]
fn unordered_groups_keep_extra_occurrences_as_wildcard_items() -> Result<(), Box<dyn Error>> {
    let schema = import(&unqualified_schema("", REPEATING_LAX, ""), "Root")?;
    assert!(
        schema.xml_repeating_choices.is_empty(),
        "the shape must not retain element order"
    );

    let input = "<Root><Head>1</Head><Head>2</Head><Other>o</Other><Head>3</Head></Root>";
    let instance = from_str(input, &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    let items = generic_items(&instance)?;
    assert_eq!(items.len(), 3);
    assert_generic(&items[0], "Head", "2");
    assert_generic(&items[1], "Other", "o");
    assert_generic(&items[2], "Head", "3");

    let output = to_string(&schema, &instance)?;
    let positions = ["<Head>1", "<Head>2", "<Other>o", "<Head>3"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);
    Ok(())
}

#[test]
fn ordered_groups_keep_extra_occurrence_values_in_document_order() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema(
            r#"<xs:element name="Note" type="xs:string"/>"#,
            REPEATING_LAX,
            "",
        ),
        "Root",
    )?;
    assert!(
        !schema.xml_repeating_choices.is_empty(),
        "the shape must retain element order"
    );

    let input = "<Root><Head>1</Head><Note>n</Note><Head>2</Head><Other>o</Other></Root>";
    let instance = from_str(input, &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    let items = generic_items(&instance)?;
    assert_eq!(items.len(), 2);
    assert_generic(&items[0], "Head", "2");
    assert_generic(&items[1], "Other", "o");

    let stream = instance
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing ordered content")?;
    let names = stream
        .iter()
        .map(|item| item.field(XML_NODE_NAME_FIELD).cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["Head", "Note", "Head", "Other"].map(|name| Some(text(name)))
    );
    assert_eq!(
        stream[0].field(XML_MIXED_CONTENT_VALUE_FIELD),
        Some(&text("1"))
    );
    assert_eq!(
        stream[2].field(XML_MIXED_CONTENT_VALUE_FIELD),
        Some(&items[0])
    );

    let output = to_string(&schema, &instance)?;
    let positions = ["<Head>1", "<Note>n", "<Head>2", "<Other>o"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    assert!(!output.contains("<Head>1</Head><Head>1"), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);

    // An extra occurrence cannot precede the declared element's own item.
    let reordered = with_field(
        &with_field(&instance, "Head", Instance::Scalar(Value::Null)),
        XML_MIXED_CONTENT_FIELD,
        Instance::Repeated(stream[1..].to_vec()),
    );
    assert!(matches!(
        to_string(&schema, &reordered),
        Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField { name, .. })
            if name == "Head"
    ));
    Ok(())
}

#[test]
fn qualified_ordered_extra_occurrences_round_trip() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{TARGET}" elementFormDefault="qualified">
  <xs:element name="Note" type="xs:string"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any namespace="##targetNamespace" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###
        ),
        &format!("{{{TARGET}}}Root"),
    )?;
    assert!(!schema.xml_repeating_choices.is_empty());
    let input = format!(
        r#"<t:Root xmlns:t="{TARGET}"><t:Head>1</t:Head><t:Note>n</t:Note><t:Head>2</t:Head></t:Root>"#
    );
    let instance = from_str(&input, &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    let items = generic_items(&instance)?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].field(XML_NAMESPACE_URI_FIELD), Some(&text(TARGET)));
    assert_eq!(items[0].field(XML_TEXT_FIELD), Some(&text("2")));

    let output = to_string(&schema, &instance)?;
    let positions = ["Head>1<", "Note>n<", "Head>2<"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);
    Ok(())
}

#[test]
fn instances_without_extra_occurrences_read_as_before() -> Result<(), Box<dyn Error>> {
    for globals in ["", r#"<xs:element name="Note" type="xs:string"/>"#] {
        let schema = import(&unqualified_schema(globals, REPEATING_LAX, ""), "Root")?;
        let instance = from_str("<Root><Head>1</Head><Other>o</Other></Root>", &schema)?;
        assert_eq!(instance.field("Head"), Some(&text("1")));
        let items = generic_items(&instance)?;
        assert_eq!(items.len(), 1);
        assert_generic(&items[0], "Other", "o");
        assert_eq!(
            from_str(&to_string(&schema, &instance)?, &schema)?,
            instance
        );

        let instance = from_str("<Root><Head>1</Head></Root>", &schema)?;
        assert!(generic_items(&instance)?.is_empty());
        assert_eq!(
            from_str(&to_string(&schema, &instance)?, &schema)?,
            instance
        );

        // A generic item still cannot replace the declared element itself.
        let mut without_head = with_field(&instance, "Head", Instance::Scalar(Value::Null));
        if let Instance::Group(fields) = &mut without_head {
            fields.retain(|(field, _)| field != XML_MIXED_CONTENT_FIELD);
        }
        let constructed = with_field(
            &without_head,
            XML_ELEMENTS_FIELD,
            Instance::Repeated(vec![
                generic_items(&from_str(
                    "<Root><Head>1</Head><Head>2</Head></Root>",
                    &schema,
                )?)?[0]
                    .clone(),
            ]),
        );
        assert!(matches!(
            to_string(&schema, &constructed),
            Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField { name, namespace })
                if name == "Head" && namespace == "##local"
        ));
    }
    Ok(())
}

#[test]
fn single_occurrence_wildcards_hold_one_extra_occurrence() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema(
            "",
            r###"namespace="##any" processContents="lax" minOccurs="0""###,
            "",
        ),
        "Root",
    )?;
    let instance = from_str("<Root><Head>1</Head><Head>2</Head></Root>", &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    let item = instance
        .field(XML_ELEMENTS_FIELD)
        .ok_or("missing generic element item")?;
    assert_generic(item, "Head", "2");
    assert_eq!(
        from_str(&to_string(&schema, &instance)?, &schema)?,
        instance
    );

    assert!(matches!(
        from_str(
            "<Root><Head>1</Head><Head>2</Head><Head>3</Head></Root>",
            &schema
        ),
        Err(XmlFormatError::XmlWildcardCardinality)
    ));
    Ok(())
}

#[test]
fn extra_occurrences_outside_the_wildcard_namespace_are_refused() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{TARGET}" elementFormDefault="qualified">
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string"/>
    <xs:any namespace="##other" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###
        ),
        &format!("{{{TARGET}}}Root"),
    )?;
    let accepted = from_str(
        &format!(r#"<t:Root xmlns:t="{TARGET}"><t:Head>1</t:Head></t:Root>"#),
        &schema,
    )?;
    assert_eq!(accepted.field("Head"), Some(&text("1")));
    assert!(matches!(
        from_str(
            &format!(r#"<t:Root xmlns:t="{TARGET}"><t:Head>1</t:Head><t:Head>2</t:Head></t:Root>"#),
            &schema
        ),
        Err(XmlFormatError::XmlWildcardNamespaceMismatch { name, namespace })
            if name == "Head" && namespace == TARGET
    ));
    Ok(())
}

#[test]
fn unordered_extras_of_elements_after_the_wildcard_are_refused() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema(
            "",
            REPEATING_LAX,
            r#"<xs:element name="Tail" type="xs:string" minOccurs="0"/>"#,
        ),
        "Root",
    )?;
    assert!(schema.xml_repeating_choices.is_empty());

    // Extras of `Head`, which precedes the wildcard, write back after it.
    let instance = from_str(
        "<Root><Head>1</Head><Head>2</Head><Tail>t</Tail></Root>",
        &schema,
    )?;
    assert_generic(&generic_items(&instance)?[0], "Head", "2");
    assert_eq!(
        from_str(&to_string(&schema, &instance)?, &schema)?,
        instance
    );

    // A generic `Tail` would be written before the typed one and swap values.
    assert!(matches!(
        from_str(
            "<Root><Head>1</Head><Tail>1</Tail><Tail>2</Tail></Root>",
            &schema
        ),
        Err(XmlFormatError::UnrepresentableXmlWildcardSurplusElement { name, namespace, .. })
            if name == "Tail" && namespace == "##local"
    ));
    let mut generic_tail = generic_items(&instance)?[0].clone();
    if let Instance::Group(fields) = &mut generic_tail
        && let Some((_, name)) = fields
            .iter_mut()
            .find(|(field, _)| field == XML_LOCAL_NAME_FIELD)
    {
        *name = text("Tail");
    }
    let constructed = with_field(
        &instance,
        XML_ELEMENTS_FIELD,
        Instance::Repeated(vec![generic_tail]),
    );
    assert!(matches!(
        to_string(&schema, &constructed),
        Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField { name, .. })
            if name == "Tail"
    ));
    Ok(())
}

#[test]
fn strict_generic_groups_refuse_extra_occurrences() -> Result<(), Box<dyn Error>> {
    let lax = import(&unqualified_schema("", REPEATING_LAX, ""), "Root")?;
    let lax_instance = from_str("<Root><Head>1</Head><Head>2</Head></Root>", &lax)?;
    let mut schema = lax;
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        return Err("Root is not a group".into());
    };
    let wildcard = children
        .iter_mut()
        .find(|child| child.name == XML_ELEMENTS_FIELD)
        .ok_or("missing wildcard")?;
    wildcard.xml_wildcard_process_contents = XmlWildcardProcessContents::Strict;

    assert!(matches!(
        from_str("<Root><Head>1</Head><Head>2</Head></Root>", &schema),
        Err(XmlFormatError::UnrepresentableXmlWildcardSurplusElement { name, .. }) if name == "Head"
    ));
    assert!(matches!(
        to_string(&schema, &lax_instance),
        Err(XmlFormatError::UnrepresentableXmlWildcardSurplusElement { name, .. }) if name == "Head"
    ));
    assert!(from_str("<Root><Head>1</Head></Root>", &schema).is_ok());
    Ok(())
}

#[test]
fn generated_xml_inputs_refuse_wildcard_schemas() -> Result<(), Box<dyn Error>> {
    let schema = import(&unqualified_schema("", REPEATING_LAX, ""), "Root")?;
    assert!(!ir::xml_structured_document_input_is_supported(&schema));
    assert!(!ir::xml_root_view_read_policy_is_supported(&schema));
    Ok(())
}

const A: &str = "urn:ferrule:wildcard-surplus:a";
const B: &str = "urn:ferrule:wildcard-surplus:b";

fn element_names(schema: &SchemaNode) -> Vec<&str> {
    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
        return Vec::new();
    };
    children
        .iter()
        .filter(|child| !child.attribute && !child.text)
        .map(|child| child.name.as_str())
        .collect()
}

/// A global `Head` referenced by `Root` beside a wildcard with the given
/// attributes.
fn referenced_schema(wildcard: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Head" type="xs:string"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element ref="Head"/>
    <xs:any {wildcard}/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
    )
}

#[test]
fn lax_projections_yield_to_referenced_declarations() -> Result<(), Box<dyn Error>> {
    let schema = import(&referenced_schema(REPEATING_LAX), "Root")?;
    assert_eq!(element_names(&schema), ["Head", XML_ELEMENTS_FIELD]);
    assert!(schema.xml_repeating_choices.is_empty());
    assert_export_reimports(&schema, "Root")?;

    let instance = round_trip(&schema, "<Root><Head>1</Head></Root>")?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    assert!(generic_items(&instance)?.is_empty());

    let instance = round_trip(&schema, "<Root><Head>1</Head><Head>2</Head></Root>")?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    let items = generic_items(&instance)?;
    assert_eq!(items.len(), 1);
    assert_generic(&items[0], "Head", "2");
    Ok(())
}

#[test]
fn single_lax_projections_leave_referenced_declarations_outside_their_choice()
-> Result<(), Box<dyn Error>> {
    let schema = import(
        &referenced_schema(r###"namespace="##any" processContents="lax" minOccurs="0""###),
        "Root",
    )?;
    assert!(
        schema
            .xml_repeating_choices
            .iter()
            .all(|choice| !choice.members.iter().any(|member| member == "Head")),
        "{:?}",
        schema.xml_repeating_choices
    );
    assert_export_reimports(&schema, "Root")?;
    let instance = round_trip(&schema, "<Root><Head>1</Head><Other>o</Other></Root>")?;
    let item = instance
        .field(XML_ELEMENTS_FIELD)
        .ok_or("missing generic element item")?;
    assert_generic(item, "Other", "o");
    let instance = round_trip(&schema, "<Root><Head>1</Head><Head>2</Head></Root>")?;
    let item = instance
        .field(XML_ELEMENTS_FIELD)
        .ok_or("missing generic element item")?;
    assert_generic(item, "Head", "2");
    Ok(())
}

#[test]
fn strict_projections_beside_referenced_declarations_are_refused() -> Result<(), Box<dyn Error>> {
    let declarations = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{A}" elementFormDefault="qualified">
  <xs:element name="Head" type="xs:string"/>
</xs:schema>"#
    );
    let schema = |reference: &str| {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    {reference}
    <xs:any namespace="{A}" processContents="strict" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        )
    };
    assert!(matches!(
        import_set(
            &schema(r#"<xs:element ref="a:Head"/>"#),
            &[("a.xsd", &declarations)],
            "Root"
        ),
        Err(XmlFormatError::UnsupportedXmlWildcard { reason })
            if reason.contains("declared sibling")
    ));

    // Without a declared sibling the strict projection keeps its typed port.
    let schema = import_set(
        &schema(r#"<xs:element name="Local" type="xs:string"/>"#),
        &[("a.xsd", &declarations)],
        "Root",
    )?;
    assert_eq!(element_names(&schema), ["Local", "Head"]);
    let instance = round_trip(
        &schema,
        &format!(
            r#"<Root xmlns:a="{A}"><Local>l</Local><a:Head>1</a:Head><a:Head>2</a:Head></Root>"#
        ),
    )?;
    assert_eq!(
        instance.field("Head"),
        Some(&Instance::Repeated(vec![text("1"), text("2")]))
    );
    Ok(())
}

#[test]
fn complex_extra_occurrences_round_trip_through_the_generic_tree() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema("", REPEATING_LAX, "").replace(
            r#"<xs:element name="Head" type="xs:string"/>"#,
            r#"<xs:element name="Head"><xs:complexType><xs:sequence>
      <xs:element name="Part" type="xs:string" maxOccurs="unbounded"/>
    </xs:sequence><xs:attribute name="id" type="xs:string"/></xs:complexType></xs:element>"#,
        ),
        "Root",
    )?;
    let instance = round_trip(
        &schema,
        r#"<Root><Head id="1"><Part>a</Part></Head><Head id="2" extra="x"><Part>b</Part><Part>c</Part></Head></Root>"#,
    )?;
    assert_eq!(
        instance.field("Head").and_then(|head| head.field("id")),
        Some(&text("1"))
    );
    let item = &generic_items(&instance)?[0];
    assert_eq!(item.field(XML_LOCAL_NAME_FIELD), Some(&text("Head")));
    let attributes = item
        .field(ir::XML_ATTRIBUTES_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing generic attributes")?;
    assert_eq!(attributes.len(), 2);
    let parts = generic_items(item)?;
    assert_eq!(parts.len(), 2);
    assert_generic(&parts[1], "Part", "c");
    Ok(())
}

#[test]
fn mixed_parents_keep_text_around_extra_occurrences() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema("", REPEATING_LAX, "").replace(
            "<xs:complexType><xs:sequence>",
            r#"<xs:complexType mixed="true"><xs:sequence>"#,
        ),
        "Root",
    )?;
    let instance = round_trip(&schema, "<Root>a<Head>1</Head>b<Head>2</Head>c</Root>")?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    assert_generic(&generic_items(&instance)?[0], "Head", "2");
    let output = to_string(&schema, &instance)?;
    assert!(
        output.contains("a<Head>1</Head>b<Head>2</Head>c"),
        "{output}"
    );
    Ok(())
}

#[test]
fn extra_occurrences_of_namespace_alternatives_keep_their_namespace() -> Result<(), Box<dyn Error>>
{
    let declarations = |namespace: &str| {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{namespace}" elementFormDefault="qualified">
  <xs:element name="Note" type="xs:string"/>
</xs:schema>"#
        )
    };
    let schema = import_set(
        &format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}" xmlns:b="{B}" targetNamespace="{TARGET}" elementFormDefault="qualified">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:import namespace="{B}" schemaLocation="b.xsd"/>
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:choice><xs:element ref="a:Note"/><xs:element ref="b:Note"/></xs:choice>
    <xs:any namespace="##other" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###
        ),
        &[("a.xsd", &declarations(A)), ("b.xsd", &declarations(B))],
        &format!("{{{TARGET}}}Root"),
    )?;
    let note = schema.child("Note").ok_or("missing Note")?;
    assert!(!note.repeating);
    assert!(!note.xml_name_alternatives.is_empty());
    assert_eq!(element_names(&schema), ["Note", XML_ELEMENTS_FIELD]);

    let instance = round_trip(
        &schema,
        &format!(
            r#"<t:Root xmlns:t="{TARGET}" xmlns:a="{A}" xmlns:b="{B}"><b:Note>1</b:Note><a:Note>2</a:Note></t:Root>"#
        ),
    )?;
    assert_eq!(instance.field("Note"), Some(&text("1")));
    let item = &generic_items(&instance)?[0];
    assert_eq!(item.field(XML_LOCAL_NAME_FIELD), Some(&text("Note")));
    assert_eq!(item.field(XML_NAMESPACE_URI_FIELD), Some(&text(A)));
    Ok(())
}

#[test]
fn nil_first_occurrences_keep_later_extras() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema("", REPEATING_LAX, "").replace(
            r#"<xs:element name="Head" type="xs:string"/>"#,
            r#"<xs:element name="Head" type="xs:string" nillable="true"/>"#,
        ),
        "Root",
    )?;
    let instance = round_trip(
        &schema,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Head xsi:nil="true"/><Head>2</Head></Root>"#,
    )?;
    assert_eq!(
        instance.field("Head"),
        Some(&Instance::Scalar(Value::xml_nil()))
    );
    assert_generic(&generic_items(&instance)?[0], "Head", "2");
    Ok(())
}

#[test]
fn declarations_without_namespace_identity_match_qualified_items_by_name()
-> Result<(), Box<dyn Error>> {
    let schema = import(&unqualified_schema("", REPEATING_LAX, ""), "Root")?;
    assert!(
        schema
            .child("Head")
            .is_some_and(|head| head.xml_namespace.is_none())
    );
    let instance = round_trip(
        &schema,
        r#"<Root><Head>1</Head><x:Head xmlns:x="urn:x">2</x:Head></Root>"#,
    )?;
    let item = &generic_items(&instance)?[0];
    assert_eq!(item.field(XML_NAMESPACE_URI_FIELD), Some(&text("urn:x")));

    // Read alone, `x:Head` fills the typed field, so the writer refuses a
    // generic `x:Head` while that field is empty.
    let constructed = with_field(
        &with_field(&instance, "Head", Instance::Scalar(Value::Null)),
        XML_ELEMENTS_FIELD,
        Instance::Repeated(vec![item.clone()]),
    );
    assert!(matches!(
        to_string(&schema, &constructed),
        Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField { name, namespace })
            if name == "Head" && namespace == "urn:x"
    ));
    Ok(())
}

#[test]
fn ordered_items_repeating_a_typed_value_are_refused() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &unqualified_schema(
            r#"<xs:element name="Note" type="xs:string"/>"#,
            REPEATING_LAX,
            "",
        ),
        "Root",
    )?;
    let instance = from_str("<Root><Head>1</Head></Root>", &schema)?;
    let stream = instance
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing ordered content")?;
    let mut second = stream[0].clone();
    if let Instance::Group(fields) = &mut second
        && let Some((_, value)) = fields
            .iter_mut()
            .find(|(field, _)| field == XML_MIXED_CONTENT_VALUE_FIELD)
    {
        *value = text("2");
    }
    let constructed = with_field(
        &instance,
        XML_MIXED_CONTENT_FIELD,
        Instance::Repeated(vec![stream[0].clone(), second]),
    );
    assert!(matches!(
        to_string(&schema, &constructed),
        Err(XmlFormatError::UnrepresentableXmlWildcardSurplusElement { name, reason, .. })
            if name == "Head" && reason.contains("generic element value")
    ));
    Ok(())
}

/// Global string elements in namespace `A`.
fn a_declarations(names: &[&str]) -> String {
    let elements = names
        .iter()
        .map(|name| format!(r#"<xs:element name="{name}" type="xs:string"/>"#))
        .collect::<String>();
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{A}" elementFormDefault="qualified">{elements}</xs:schema>"#
    )
}

/// `Root` with the given content over `a.xsd` declaring `names`.
fn import_over_a(content: &str, names: &[&str]) -> Result<SchemaNode, XmlFormatError> {
    import_set(
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:element name="Root"><xs:complexType>{content}</xs:complexType></xs:element>
</xs:schema>"#
        ),
        &[("a.xsd", &a_declarations(names))],
        "Root",
    )
}

fn is_strict_sibling_refusal(result: &Result<SchemaNode, XmlFormatError>) -> bool {
    matches!(result, Err(XmlFormatError::UnsupportedXmlWildcard { reason })
        if reason.contains("declared sibling"))
}

#[test]
fn single_strict_wildcards_absorbed_by_referenced_declarations_are_refused() {
    for occurrence in ["", r#" minOccurs="0""#] {
        let result = import_over_a(
            &format!(
                r#"<xs:sequence><xs:element ref="a:Head"/><xs:any namespace="{A}" processContents="strict"{occurrence}/></xs:sequence>"#
            ),
            &["Head"],
        );
        assert!(
            is_strict_sibling_refusal(&result),
            "{occurrence}: {result:?}"
        );
    }
}

#[test]
fn strict_wildcards_that_never_match_a_declared_sibling_drop_its_port() -> Result<(), Box<dyn Error>>
{
    let any = format!(
        r#"<xs:any namespace="{A}" processContents="strict" minOccurs="0" maxOccurs="unbounded"/>"#
    );
    // The declaration takes precedence where both could match.
    for content in [
        format!(r#"<xs:sequence>{any}<xs:element ref="a:Head"/></xs:sequence>"#),
        format!(r#"<xs:choice><xs:element ref="a:Head"/>{any}</xs:choice>"#),
        format!(
            r#"<xs:sequence><xs:element ref="a:Head" maxOccurs="unbounded"/><xs:sequence minOccurs="0">{any}</xs:sequence></xs:sequence>"#
        ),
    ] {
        let schema = import_over_a(&content, &["Head"])?;
        assert_eq!(element_names(&schema), ["Head"], "{content}");
        assert!(schema.child("Head").is_some_and(
            |head| head.xml_wildcard_process_contents == XmlWildcardProcessContents::Skip
        ));
    }
    Ok(())
}

#[test]
fn repeating_referenced_declarations_keep_element_order_beside_lax_wildcards()
-> Result<(), Box<dyn Error>> {
    let schema = import(
        &referenced_schema(REPEATING_LAX).replace(
            r#"<xs:element ref="Head"/>"#,
            r#"<xs:element ref="Head" minOccurs="0" maxOccurs="unbounded"/>"#,
        ),
        "Root",
    )?;
    assert_eq!(element_names(&schema), ["Head", XML_ELEMENTS_FIELD]);
    assert!(!schema.xml_repeating_choices.is_empty());
    let instance = round_trip(&schema, "<Root><Head>1</Head><X/><Head>2</Head></Root>")?;
    assert_eq!(
        instance.field("Head"),
        Some(&Instance::Repeated(vec![text("1"), text("2")]))
    );
    let output = to_string(&schema, &instance)?;
    let positions = ["<Head>1", "<X", "<Head>2"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    Ok(())
}

#[test]
fn repeating_referenced_declarations_keep_element_order_beside_strict_wildcards()
-> Result<(), Box<dyn Error>> {
    let schema = import_over_a(
        &format!(
            r#"<xs:sequence><xs:element ref="a:Head" minOccurs="0" maxOccurs="unbounded"/><xs:any namespace="{A}" processContents="strict" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"#
        ),
        &["Head", "Note"],
    )?;
    assert_eq!(element_names(&schema), ["Head", "Note"]);
    let input = format!(
        r#"<Root xmlns:a="{A}"><a:Head>1</a:Head><a:Note>n</a:Note><a:Head>2</a:Head></Root>"#
    );
    let instance = round_trip(&schema, &input)?;
    let output = to_string(&schema, &instance)?;
    let positions = ["1</Head>", "n</Note>", "2</Head>"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    Ok(())
}

/// `Doc` extends `Base(Item*, Total)` with the given wildcard content.
fn extension_schema(wildcard: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Item" type="xs:string"/>
  <xs:element name="Total" type="xs:string"/>
  <xs:complexType name="Base"><xs:sequence>
    <xs:element ref="Item" maxOccurs="unbounded"/>
    <xs:element ref="Total"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="Open"><xs:complexContent><xs:extension base="Base">
    <xs:sequence>{wildcard}</xs:sequence>
  </xs:extension></xs:complexContent></xs:complexType>
  <xs:element name="Doc" type="Open"/>
</xs:schema>"#
    )
}

#[test]
fn extension_wildcards_yield_to_the_base_declarations() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &extension_schema(
            r###"<xs:any namespace="##any" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>"###,
        ),
        "Doc",
    )?;
    assert_eq!(
        element_names(&schema),
        ["Item", "Total", XML_ELEMENTS_FIELD]
    );
    let instance = round_trip(&schema, "<Doc><Item>a</Item><Total>t</Total></Doc>")?;
    let output = to_string(&schema, &instance)?;
    assert!(output.find("<Item>") < output.find("<Total>"), "{output}");

    let instance = round_trip(
        &schema,
        "<Doc><Item>a</Item><Total>t</Total><Item>b</Item><Total>u</Total></Doc>",
    )?;
    assert_eq!(
        instance.field("Item"),
        Some(&Instance::Repeated(vec![text("a"), text("b")]))
    );
    assert_eq!(instance.field("Total"), Some(&text("t")));
    assert_generic(&generic_items(&instance)?[0], "Total", "u");
    // The base `Item` is not adjacent to the wildcard, so no choice retains
    // its interleaving with wildcard items; the group writes in schema order.
    assert!(schema.xml_repeating_choices.is_empty());
    let output = to_string(&schema, &instance)?;
    let positions = ["Item>a<", "Item>b<", "Total>t<", "Total>u<"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    assert_export_reimports(&schema, "Doc")?;
    Ok(())
}

#[test]
fn strict_extension_wildcards_resolve_against_the_base() -> Result<(), Box<dyn Error>> {
    let schema = |base: &str| {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:complexType name="Base"><xs:sequence>{base}</xs:sequence></xs:complexType>
  <xs:complexType name="Open"><xs:complexContent><xs:extension base="Base"><xs:sequence>
    <xs:any namespace="{A}" processContents="strict" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:extension></xs:complexContent></xs:complexType>
  <xs:element name="Doc" type="Open"/>
</xs:schema>"#
        )
    };
    let declarations = a_declarations(&["Item", "Total"]);
    let repeating = import_set(
        &schema(r#"<xs:element ref="a:Item" maxOccurs="unbounded"/>"#),
        &[("a.xsd", &declarations)],
        "Doc",
    )?;
    assert_eq!(element_names(&repeating), ["Item", "Total"]);
    round_trip(
        &repeating,
        &format!(r#"<Doc xmlns:a="{A}"><a:Item>1</a:Item><a:Item>2</a:Item></Doc>"#),
    )?;

    let single = import_set(
        &schema(r#"<xs:element ref="a:Item" maxOccurs="unbounded"/><xs:element ref="a:Total"/>"#),
        &[("a.xsd", &declarations)],
        "Doc",
    );
    assert!(is_strict_sibling_refusal(&single), "{single:?}");
    Ok(())
}

#[test]
fn unqualified_and_namespace_free_declarations_share_one_port() -> Result<(), Box<dyn Error>> {
    let schema = import(
        &referenced_schema(REPEATING_LAX).replace(
            r#"<xs:element ref="Head"/>"#,
            r#"<xs:element name="Head" type="xs:string" form="unqualified"/>"#,
        ),
        "Root",
    )?;
    assert_eq!(element_names(&schema), ["Head", XML_ELEMENTS_FIELD]);
    let instance = round_trip(&schema, "<Root><Head>1</Head><Head>2</Head></Root>")?;
    assert_generic(&generic_items(&instance)?[0], "Head", "2");
    Ok(())
}

#[test]
fn declared_extension_elements_resolve_base_wildcard_ports() -> Result<(), Box<dyn Error>> {
    let schema = |root: &str| {
        import(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Head" type="xs:string"/>
  <xs:complexType name="Base"><xs:sequence>
    <xs:any namespace="##any" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="Open"><xs:complexContent><xs:extension base="Base">
    <xs:sequence><xs:element ref="Head"/></xs:sequence>
  </xs:extension></xs:complexContent></xs:complexType>
  <xs:element name="Doc" type="Open"/>
</xs:schema>"###,
            root,
        )
    };
    // The declared `Head` after the wildcard takes precedence over it.
    let doc = schema("Doc")?;
    assert_eq!(element_names(&doc), [XML_ELEMENTS_FIELD, "Head"]);
    assert!(doc.xml_repeating_choices.is_empty());
    let instance = round_trip(&doc, "<Doc><Head>1</Head></Doc>")?;
    assert_eq!(instance.field("Head"), Some(&text("1")));
    assert!(generic_items(&instance)?.is_empty());
    let instance = round_trip(&doc, "<Doc><X>x</X><Head>1</Head></Doc>")?;
    assert_generic(&generic_items(&instance)?[0], "X", "x");
    Ok(())
}

#[test]
fn same_local_projected_names_merge_with_declared_alternatives() -> Result<(), Box<dyn Error>> {
    let declarations = |namespace: &str| {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{namespace}" elementFormDefault="qualified">
  <xs:element name="Note" type="xs:string"/>
</xs:schema>"#
        )
    };
    let schema = import_set(
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}" targetNamespace="{TARGET}" elementFormDefault="qualified">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:import namespace="{B}" schemaLocation="b.xsd"/>
  <xs:element name="Root"><xs:complexType>
    <xs:choice maxOccurs="unbounded">
      <xs:element ref="a:Note"/>
      <xs:any namespace="{B}" processContents="lax"/>
    </xs:choice>
  </xs:complexType></xs:element>
</xs:schema>"#
        ),
        &[("a.xsd", &declarations(A)), ("b.xsd", &declarations(B))],
        &format!("{{{TARGET}}}Root"),
    )?;
    assert_eq!(element_names(&schema), ["Note", XML_ELEMENTS_FIELD]);
    let note = schema.child("Note").ok_or("missing Note")?;
    assert!(note.repeating);
    assert_eq!(
        note.xml_namespace.as_ref().and_then(ir::XmlNamespace::uri),
        Some(A)
    );
    assert_eq!(
        note.xml_name_alternatives
            .iter()
            .map(ir::XmlNamespace::uri)
            .collect::<Vec<_>>(),
        [Some(B)]
    );
    assert_eq!(
        note.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Skip
    );
    let instance = round_trip(
        &schema,
        &format!(
            r#"<t:Root xmlns:t="{TARGET}" xmlns:a="{A}" xmlns:b="{B}"><a:Note>1</a:Note><b:Note>2</b:Note><b:Other>o</b:Other></t:Root>"#
        ),
    )?;
    assert_eq!(
        instance.field("Note"),
        Some(&Instance::Repeated(vec![text("1"), text("2")]))
    );
    Ok(())
}

#[test]
fn base_choices_keep_their_members_beside_extension_wildcards() -> Result<(), Box<dyn Error>> {
    let schema = import(
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Item" type="xs:string"/>
  <xs:element name="Total" type="xs:string"/>
  <xs:complexType name="Base"><xs:choice maxOccurs="unbounded">
    <xs:element ref="Item"/><xs:element ref="Total"/>
  </xs:choice></xs:complexType>
  <xs:complexType name="Open"><xs:complexContent><xs:extension base="Base">
    <xs:sequence>
      <xs:any namespace="##any" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence>
  </xs:extension></xs:complexContent></xs:complexType>
  <xs:element name="Doc" type="Open"/>
</xs:schema>"###,
        "Doc",
    )?;
    assert_eq!(
        element_names(&schema),
        ["Item", "Total", XML_ELEMENTS_FIELD]
    );
    assert!(schema.xml_repeating_choices_are_valid());
    assert_eq!(
        schema
            .xml_repeating_choices
            .iter()
            .map(|choice| choice.members.clone())
            .collect::<Vec<_>>(),
        [vec!["Item".to_string(), "Total".to_string()]]
    );
    let instance = round_trip(
        &schema,
        "<Doc><Item>a</Item><Total>t</Total><Item>b</Item><X>x</X></Doc>",
    )?;
    let output = to_string(&schema, &instance)?;
    let positions = ["Item>a<", "Total>t<", "Item>b<", "X>x<"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    Ok(())
}

/// `Root` in `TARGET` with the given content over global `Note`s in `A`
/// and `B`.
fn import_notes(content: &str) -> Result<SchemaNode, XmlFormatError> {
    let declarations = |namespace: &str| {
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="{namespace}" elementFormDefault="qualified"><xs:element name="Note" type="xs:string"/></xs:schema>"#
        )
    };
    import_set(
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="{A}" xmlns:b="{B}" targetNamespace="{TARGET}" elementFormDefault="qualified">
  <xs:import namespace="{A}" schemaLocation="a.xsd"/>
  <xs:import namespace="{B}" schemaLocation="b.xsd"/>
  <xs:element name="Root"><xs:complexType>{content}</xs:complexType></xs:element>
</xs:schema>"#
        ),
        &[("a.xsd", &declarations(A)), ("b.xsd", &declarations(B))],
        &format!("{{{TARGET}}}Root"),
    )
}

fn notes_input(notes: &str) -> String {
    format!(r#"<t:Root xmlns:t="{TARGET}" xmlns:a="{A}" xmlns:b="{B}">{notes}</t:Root>"#)
}

fn note_namespaces(schema: &SchemaNode) -> Vec<Option<&str>> {
    schema.child("Note").map_or_else(Vec::new, |note| {
        note.xml_namespace
            .iter()
            .chain(&note.xml_name_alternatives)
            .map(ir::XmlNamespace::uri)
            .collect()
    })
}

#[test]
fn strict_sequence_ports_partly_covered_by_a_declaration_are_refused() {
    let result = import_notes(
        r###"<xs:sequence><xs:element ref="a:Note" maxOccurs="unbounded"/><xs:any namespace="##other" processContents="strict" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###,
    );
    assert!(
        matches!(&result, Err(XmlFormatError::UnsupportedXmlWildcard { reason })
            if reason.contains("only some of its namespaces")),
        "{result:?}"
    );
}

#[test]
fn lax_sequence_ports_partly_covered_by_a_declaration_keep_other_namespaces_generic()
-> Result<(), Box<dyn Error>> {
    let schema = import_notes(
        r###"<xs:sequence><xs:element ref="a:Note" maxOccurs="unbounded"/><xs:any namespace="##other" processContents="lax" minOccurs="0" maxOccurs="unbounded"/></xs:sequence>"###,
    )?;
    assert_eq!(element_names(&schema), ["Note", XML_ELEMENTS_FIELD]);
    assert_eq!(note_namespaces(&schema), [Some(A)]);
    let instance = round_trip(
        &schema,
        &notes_input("<a:Note>1</a:Note><b:Note>2</b:Note><a:Note>3</a:Note>"),
    )?;
    assert_eq!(
        instance.field("Note"),
        Some(&Instance::Repeated(vec![text("1"), text("3")]))
    );
    let item = &generic_items(&instance)?[0];
    assert_eq!(item.field(XML_NAMESPACE_URI_FIELD), Some(&text(B)));
    assert_eq!(item.field(XML_TEXT_FIELD), Some(&text("2")));
    let output = to_string(&schema, &instance)?;
    let positions = ["1</Note>", "2</Note>", "3</Note>"].map(|tag| output.find(tag));
    assert!(positions.iter().all(Option::is_some), "{output}");
    assert!(positions.is_sorted(), "{output}");
    Ok(())
}

#[test]
fn choice_ports_partly_covered_by_a_declaration_merge_their_other_namespaces()
-> Result<(), Box<dyn Error>> {
    for process in ["strict", "lax"] {
        let schema = import_notes(&format!(
            r###"<xs:choice maxOccurs="unbounded"><xs:element ref="a:Note"/><xs:any namespace="##other" processContents="{process}"/></xs:choice>"###
        ))?;
        assert_eq!(note_namespaces(&schema), [Some(A), Some(B)], "{process}");
        let expected = if process == "lax" {
            vec!["Note", XML_ELEMENTS_FIELD]
        } else {
            vec!["Note"]
        };
        assert_eq!(element_names(&schema), expected, "{process}");
        let instance = round_trip(
            &schema,
            &notes_input("<a:Note>1</a:Note><b:Note>2</b:Note>"),
        )?;
        assert_eq!(
            instance.field("Note"),
            Some(&Instance::Repeated(vec![text("1"), text("2")])),
            "{process}"
        );
    }
    Ok(())
}

#[test]
fn ports_fully_covered_by_declared_alternatives_yield() -> Result<(), Box<dyn Error>> {
    for process in ["strict", "lax"] {
        let schema = import_notes(&format!(
            r###"<xs:sequence>
  <xs:choice maxOccurs="unbounded"><xs:element ref="a:Note"/><xs:element ref="b:Note"/></xs:choice>
  <xs:any namespace="##other" processContents="{process}" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence>"###
        ))?;
        assert_eq!(note_namespaces(&schema), [Some(A), Some(B)], "{process}");
        assert_eq!(
            element_names(&schema)
                .iter()
                .filter(|name| **name == "Note")
                .count(),
            1,
            "{process}"
        );
        let instance = round_trip(
            &schema,
            &notes_input("<b:Note>1</b:Note><a:Note>2</a:Note>"),
        )?;
        assert_eq!(
            instance.field("Note"),
            Some(&Instance::Repeated(vec![text("1"), text("2")])),
            "{process}"
        );
    }
    // A declaration after the wildcard covering only `A` leaves `B` to it.
    let after = |process: &str| {
        import_notes(&format!(
            r###"<xs:sequence><xs:any namespace="##other" processContents="{process}" minOccurs="0" maxOccurs="unbounded"/><xs:element ref="a:Note"/></xs:sequence>"###
        ))
    };
    let strict = after("strict");
    assert!(
        matches!(&strict, Err(XmlFormatError::UnsupportedXmlWildcard { reason })
            if reason.contains("only some of its namespaces")),
        "{strict:?}"
    );
    let lax = after("lax")?;
    assert_eq!(note_namespaces(&lax), [Some(A)]);
    let instance = round_trip(&lax, &notes_input("<b:Note>1</b:Note><a:Note>2</a:Note>"))?;
    assert_eq!(instance.field("Note"), Some(&text("2")));
    assert_eq!(
        generic_items(&instance)?[0].field(XML_NAMESPACE_URI_FIELD),
        Some(&text(B))
    );
    Ok(())
}
