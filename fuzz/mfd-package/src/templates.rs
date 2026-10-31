//! Self-authored starting packages that fuzz programs mutate.
//!
//! Each design maps one string field `Value` from a source document `Input`
//! to a target document `Output`, the smallest shape the importer lowers into
//! an executable project. The designs follow the element layout of
//! `mfd::export_package` output for the same projects.

/// One starting package: its design path and its entries, in order.
pub struct Template {
    pub name: &'static str,
    pub design: &'static str,
    pub entries: Vec<(String, Vec<u8>)>,
}

/// Every template; a program's first byte selects one modulo this length.
pub fn all() -> Vec<Template> {
    let xml_to_json = xml_to_json_design("design-source.xsd", "design-target.schema.json");
    vec![
        Template {
            name: "empty",
            design: "design.mfd",
            entries: Vec::new(),
        },
        Template {
            name: "xml_to_json",
            design: "design.mfd",
            entries: entries([
                ("design.mfd", xml_to_json.clone()),
                ("design-source.xsd", INPUT_XSD.into()),
                ("design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
        Template {
            name: "json_to_xml",
            design: "design.mfd",
            entries: entries([
                ("design.mfd", JSON_TO_XML_DESIGN.into()),
                ("design-source.schema.json", INPUT_JSON_SCHEMA.into()),
                ("design-target.xsd", OUTPUT_XSD.into()),
            ]),
        },
        Template {
            name: "dtd_source",
            design: "design.mfd",
            entries: entries([
                (
                    "design.mfd",
                    xml_to_json_design("source.dtd", "design-target.schema.json"),
                ),
                ("source.dtd", INPUT_DTD.into()),
                ("design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
        Template {
            name: "nested_design",
            design: "maps/design.mfd",
            entries: entries([
                ("maps/design.mfd", xml_to_json.clone()),
                ("maps/design-source.xsd", INPUT_XSD.into()),
                ("maps/design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
        Template {
            name: "xsd_include",
            design: "design.mfd",
            entries: entries([
                ("design.mfd", xml_to_json.clone()),
                ("design-source.xsd", INCLUDING_XSD.into()),
                ("types.xsd", INPUT_XSD.into()),
                ("design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
        Template {
            name: "json_ref",
            design: "design.mfd",
            entries: entries([
                ("design.mfd", JSON_TO_XML_DESIGN.into()),
                ("design-source.schema.json", REFERRING_JSON_SCHEMA.into()),
                ("types.schema.json", JSON_DEFINITIONS.into()),
                ("design-target.xsd", OUTPUT_XSD.into()),
            ]),
        },
        Template {
            name: "edi_idoc_source",
            design: "design.mfd",
            entries: entries([
                (
                    "design.mfd",
                    json_target_design(EDI_IDOC_SOURCE, "design-target.schema.json"),
                ),
                ("orders.config", EDI_CONFIG.into()),
                ("design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
        Template {
            name: "db_source",
            design: "design.mfd",
            entries: entries([
                (
                    "design.mfd",
                    json_target_design(DB_SOURCE, "design-target.schema.json"),
                ),
                ("orders.sqlite", "SQLite format 3\0".into()),
                ("design-target.schema.json", OUTPUT_JSON_SCHEMA.into()),
            ]),
        },
    ]
}

fn entries<const N: usize>(list: [(&str, String); N]) -> Vec<(String, Vec<u8>)> {
    list.into_iter()
        .map(|(path, text)| (path.to_owned(), text.into_bytes()))
        .collect()
}

fn xml_to_json_design(source_schema: &str, target_schema: &str) -> String {
    json_target_design(
        &format!(
            r#"<component name="Input" library="xml" uid="2" kind="14">
<data>
<root>
<header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header>
<entry name="FileInstance" ns="1">
<entry name="document" ns="1">
<entry name="Input" ferrule-kind="group" ferrule-repeating="0">
<entry name="Value" outkey="2" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
</entry>
</entry>
</entry>
</root>
<document schema="{source_schema}" instanceroot="{{}}Input" inputinstance="input.xml"/>
</data>
</component>"#
        ),
        target_schema,
    )
}

/// A design whose source component `source` (uid 2, output key 2) feeds the
/// JSON target of `xml_to_json`.
fn json_target_design(source: &str, target_schema: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22" ferrule-primary-source="2">
<resources/>
<component name="defaultmap" uid="1" editable="1">
<properties SelectedLanguage="builtin"/>
<structure>
<children>
{source}
<component name="Output" library="json" uid="3" kind="31">
<properties XSLTDefaultOutput="1"/>
<data>
<root>
<header><namespaces><namespace/></namespaces></header>
<entry name="FileInstance">
<entry name="document">
<entry name="root">
<entry name="object" inpkey="3">
<entry name="Value" type="json-property">
<entry name="string" inpkey="4"/>
</entry>
</entry>
</entry>
</entry>
</entry>
</root>
<json schema="{target_schema}" outputinstance="output.json"/>
</data>
</component>
</children>
<graph directed="1">
<edges/>
<vertices>
<vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex>
</vertices>
</graph>
</structure>
</component>
</mapping>
"#
    )
}

const JSON_TO_XML_DESIGN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22" ferrule-primary-source="2">
<resources/>
<component name="defaultmap" uid="1" editable="1">
<properties SelectedLanguage="builtin"/>
<structure>
<children>
<component name="Input" library="json" uid="2" kind="31">
<data>
<root>
<header><namespaces><namespace/></namespaces></header>
<entry name="FileInstance">
<entry name="document">
<entry name="root">
<entry name="object" outkey="1">
<entry name="Value" type="json-property">
<entry name="string" outkey="2"/>
</entry>
</entry>
</entry>
</entry>
</entry>
</root>
<json schema="design-source.schema.json" inputinstance="input.json"/>
</data>
</component>
<component name="Output" library="xml" uid="3" kind="14">
<properties XSLTDefaultOutput="1" AddSchemaLocation="0"/>
<data>
<root>
<header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header>
<entry name="FileInstance" ns="1">
<entry name="document" ns="1">
<entry name="Output" ferrule-kind="group" ferrule-repeating="0">
<entry name="Value" inpkey="4" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
</entry>
</entry>
</entry>
</root>
<document schema="design-target.xsd" instanceroot="{}Output" outputinstance="output.xml"/>
</data>
</component>
</children>
<graph directed="1">
<edges/>
<vertices>
<vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex>
</vertices>
</graph>
</structure>
</component>
</mapping>
"#;

/// An IDoc (`EDIFIXED`) source. Its configuration importer reads only from
/// the filesystem, so a package import must refuse it.
const EDI_IDOC_SOURCE: &str = r#"<component name="Input" library="text" uid="2" kind="16">
<data>
<root>
<entry name="FileInstance">
<entry name="document">
<entry name="ORDERS05">
<entry name="Value" outkey="2"/>
</entry>
</entry>
</entry>
</root>
<text type="edi" kind="EDIFIXED" config="orders.config" inputinstance="orders.idoc"><messages><message type="ORDERS05"/></messages></text>
</data>
</component>"#;

const EDI_CONFIG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Config Version="3"><Format standard="IDoc"/><Message Type="ORDERS05"/></Config>
"#;

/// A database source whose connection names a supplied file. Database
/// introspection needs a filesystem path, so a package import must refuse it.
const DB_SOURCE: &str = r#"<component name="Input" library="db" uid="2" kind="15">
<data>
<root>
<entry name="Orders" type="table">
<entry name="Value" outkey="2"/>
</entry>
</root>
<database ref="orders"/>
<database_connection name="orders" ConnectionString="orders.sqlite"/>
</data>
</component>"#;

const INPUT_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" elementFormDefault="unqualified">
  <xs:element name="Input">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Value" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#;

const OUTPUT_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" elementFormDefault="unqualified">
  <xs:element name="Output">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Value" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#;

const INCLUDING_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:include schemaLocation="types.xsd"/>
</xs:schema>
"#;

const INPUT_DTD: &str = "<!ELEMENT Input (Value)>\n<!ELEMENT Value (#PCDATA)>\n";

const INPUT_JSON_SCHEMA: &str = r#"{
  "title": "Input",
  "type": "object",
  "properties": {
    "Value": {
      "type": "string"
    }
  },
  "additionalProperties": false
}
"#;

const OUTPUT_JSON_SCHEMA: &str = r#"{
  "title": "Output",
  "type": "object",
  "properties": {
    "Value": {
      "type": "string"
    }
  },
  "additionalProperties": false
}
"#;

const REFERRING_JSON_SCHEMA: &str = r##"{
  "title": "Input",
  "type": "object",
  "properties": {
    "Value": {
      "$ref": "types.schema.json#/$defs/Value"
    }
  },
  "additionalProperties": false
}
"##;

const JSON_DEFINITIONS: &str = r##"{"$defs":{"Value":{"type":"string"},"Self":{"$ref":"#/$defs/Self"}}}
"##;
