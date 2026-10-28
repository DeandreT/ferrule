//! Per-call cost attribution for the generated JSON5, XML and X12 endpoints.
//!
//! Run
//! `cargo test -p codegen-runtime --release --test endpoint_codec_preparation -- --nocapture`
//! to see the tables; `docs/performance/endpoint-codec-preparation.md`
//! records the observations and how to read them.

mod support;

use codegen_runtime::{
    Instance, MAX_EMBEDDED_X12_SCHEMA_BYTES, MAX_EMBEDDED_XML_SCHEMA_BYTES, map_json5_bytes_with,
    map_json5_with, parse_structured_xml, parse_structured_xml_bytes, parse_x12, parse_x12_bytes,
    parse_xml, serialize_x12, serialize_x12_bytes, serialize_xml, serialize_xml_document,
};
use ir::{ScalarType, SchemaNode};
use support::allocation::{Cost, header, measure, retained, row};

const SMALL: u32 = 200;
const LARGE: u32 = 3;

/// A self-authored closed JSON5 object: 48 scalars and two nested objects.
fn json5_schema(name: &str) -> SchemaNode {
    let mut children = vec![
        SchemaNode::scalar("Id", ScalarType::String),
        SchemaNode::scalar("Quantity", ScalarType::Int),
        SchemaNode::scalar("Total", ScalarType::Float),
        SchemaNode::scalar("Paid", ScalarType::Bool),
        SchemaNode::scalar("Note", ScalarType::String),
        SchemaNode::group(
            "Customer",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("City", ScalarType::String),
            ],
        ),
        SchemaNode::group(
            "Shipping",
            vec![
                SchemaNode::scalar("Carrier", ScalarType::String),
                SchemaNode::scalar("Days", ScalarType::Int),
            ],
        ),
    ];
    children.extend(
        (0..43).map(|index| SchemaNode::scalar(format!("Attribute{index:02}"), ScalarType::String)),
    );
    SchemaNode::group(name, children)
}

fn json5_document(note_bytes: usize) -> String {
    format!(
        "// order\n{{\n  Id: 'ORD-000042',\n  Quantity: 0x10,\n  Total: +1234.5,\n  Paid: true,\n  Note: '{}',\n  Customer: {{ Name: 'Example Customer', City: \"Example City\", }},\n  Shipping: {{ Carrier: 'Post', Days: 3 }},\n  Attribute00: 'alpha', Attribute01: 'beta',\n}}\n",
        "n".repeat(note_bytes)
    )
}

/// The JSON order schema without patterns or ranges, which the XML
/// boundaries do not own.
fn order_schema() -> SchemaNode {
    let line = SchemaNode::group(
        "Line",
        vec![
            SchemaNode::scalar("Sku", ScalarType::String),
            SchemaNode::scalar("Quantity", ScalarType::Int),
            SchemaNode::scalar("Price", ScalarType::Float),
            SchemaNode::scalar("Note", ScalarType::String),
        ],
    )
    .repeating();
    let mut children = vec![
        SchemaNode::scalar("Id", ScalarType::String),
        SchemaNode::scalar("Customer", ScalarType::String),
        SchemaNode::scalar("Currency", ScalarType::String),
        SchemaNode::scalar("Total", ScalarType::Float),
        line,
    ];
    children.extend(
        (0..48).map(|index| SchemaNode::scalar(format!("Attribute{index:02}"), ScalarType::String)),
    );
    SchemaNode::group("Order", children)
}

fn order_instance(schema: &SchemaNode, lines: usize) -> Instance {
    let lines = (0..lines)
        .map(|index| {
            serde_json::json!({
                "Sku": format!("AB-{:04}", index % 10_000),
                "Quantity": index % 999 + 1,
                "Price": 1.25 + (index % 100) as f64,
                "Note": format!("line {index} of the order"),
            })
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({
        "Id": "ORD-000042",
        "Customer": "Example Customer",
        "Currency": "EUR",
        "Total": 1234.5,
        "Line": lines,
        "Attribute00": "alpha",
        "Attribute01": "beta",
    })
    .to_string();
    let Ok(instance) = format_json::from_str(&document, schema) else {
        panic!("order fixture parses");
    };
    instance
}

/// A flat root with two `xsi:type` alternatives, the observed primary XML profile.
fn observed_descriptor() -> String {
    let schema: SchemaNode = ok(
        "observed XML schema is valid",
        serde_json::from_str(
            r#"{
          "name":"Envelope","xml_namespace":{"kind":"unqualified"},"repeating":false,
          "xml_type_alternatives":true,"xml_default_type":"Base",
          "kind":{"kind":"group","children":[
            {"name":"Token","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}},
            {"name":"Supplement","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}
          ],"alternatives":[{"name":"Base","members":["Token"]},{"name":"Extended","members":["Token","Supplement"]}]}
        }"#,
        ),
    );
    ok(
        "observed XML schema encodes",
        codegen_schema::encode(&schema, MAX_EMBEDDED_XML_SCHEMA_BYTES),
    )
}

fn fields(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| SchemaNode::scalar(format!("E{index:02}"), ScalarType::String))
            .collect(),
    )
}

fn x12_descriptor() -> String {
    let schema = SchemaNode::group(
        "Envelope",
        vec![
            fields("ISA", 16),
            fields("GS", 8),
            fields("ST", 2),
            fields("NTE", 2).repeating(),
            fields("SE", 2),
            fields("GE", 2),
            fields("IEA", 2),
        ],
    );
    let Ok(schema) = codegen_schema::encode(&schema, MAX_EMBEDDED_X12_SCHEMA_BYTES) else {
        panic!("X12 fixture schema encodes");
    };
    serde_json::json!({
        "schema": schema,
        "version": "004010",
        "separators": null,
        "lenient_segments": false,
        "constraints": [],
        "implied_decimals": [],
        "lexical_formats": [],
        "autocomplete": null,
    })
    .to_string()
}

fn x12_document(notes: usize) -> String {
    let mut text = String::from(
        "ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*U*00401*000000123*0*T*:~\nGS*PO*ALPHA*BETA*20300615*0945*7*X*004010~\nST*850*0007~\n",
    );
    for index in 0..notes {
        text.push_str(&format!("NTE*note {index}*free form text of the note~\n"));
    }
    text.push_str(&format!(
        "SE*{}*0007~\nGE*1*7~\nIEA*1*000000123~\n",
        notes + 2
    ));
    text
}

fn ok<T, E: std::fmt::Debug>(label: &str, result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{label}: {error:?}"),
    }
}

#[test]
fn json5_endpoint_per_call_cost_attribution() {
    let source = json5_schema("Source");
    let target = json5_schema("Target");
    let source_descriptor = ok(
        "JSON5 source encodes",
        codegen_schema::json5_profile::encode_schema(&source),
    );
    let target_descriptor = ok(
        "JSON5 target encodes",
        codegen_schema::json5_profile::encode_schema(&target),
    );
    let small = json5_document(32);
    let large = json5_document(1_000_000);
    // The mapping rebuilds the input as the target; its cost is reported
    // separately so that the boundary cost can be read without it.
    let mapping = |input: &Instance| Ok(input.clone());
    let output = ok(
        "large JSON5 maps",
        map_json5_with(&source_descriptor, &target_descriptor, &large, mapping),
    );
    println!(
        "JSON5: descriptors {} + {} bytes; small document {} bytes; large document {} bytes; large output {} bytes",
        source_descriptor.len(),
        target_descriptor.len(),
        small.len(),
        large.len(),
        output.len()
    );
    header();
    let decode = measure(SMALL, || {
        codegen_schema::json5_profile::decode_schema(&source_descriptor)
    });
    row(
        "one descriptor decode (`json5_profile::decode_schema`)",
        decode,
    );
    let (_, live) = retained(|| codegen_schema::json5_profile::decode_schema(&source_descriptor));
    println!("| live bytes retained by one decoded descriptor | | | {live} | |");
    let small_instance = ok(
        "small JSON5 normalizes",
        format_json::json5_boundary::normalize(&small)
            .map_err(|error| error.to_string())
            .and_then(|text| format_json::from_str(&text, &source).map_err(|e| e.to_string())),
    );
    row(
        "mapping clone, small",
        measure(SMALL, || small_instance.clone()),
    );
    let map_small = measure(SMALL, || {
        map_json5_with(&source_descriptor, &target_descriptor, &small, mapping)
    });
    row("`map_json5_with`, small", map_small);
    row(
        "`map_json5_bytes_with`, small",
        measure(SMALL, || {
            map_json5_bytes_with(
                &source_descriptor,
                &target_descriptor,
                small.as_bytes(),
                mapping,
            )
        }),
    );
    row(
        "`map_json5_with`, large",
        measure(LARGE, || {
            map_json5_with(&source_descriptor, &target_descriptor, &large, mapping)
        }),
    );
    row(
        "`map_json5_bytes_with`, large",
        measure(LARGE, || {
            map_json5_bytes_with(
                &source_descriptor,
                &target_descriptor,
                large.as_bytes(),
                mapping,
            )
        }),
    );
    let normalized = ok(
        "large JSON5 normalizes",
        format_json::json5_boundary::normalize(&large),
    );
    row(
        "normalized strict text of the large input",
        measure(LARGE, || format_json::json5_boundary::normalize(&large)),
    );
    row(
        "`format_json::from_str` of that text",
        measure(LARGE, || format_json::from_str(&normalized, &source)),
    );
    let large_instance = ok(
        "large normalized parses",
        format_json::from_str(&normalized, &source),
    );
    row(
        "mapping clone, large",
        measure(LARGE, || large_instance.clone()),
    );
    let value = ok(
        "large output normalizes",
        format_json::to_value(&target, &large_instance),
    );
    row(
        "owned normalized `serde_json::Value` of the large output",
        measure(LARGE, || format_json::to_value(&target, &large_instance)),
    );
    row(
        "pretty text from that value",
        measure(LARGE, || serde_json::to_string_pretty(&value)),
    );
    row(
        "measuring pass over that value (`io::sink`)",
        measure(LARGE, || {
            serde_json::to_writer_pretty(std::io::sink(), &value)
        }),
    );
    assert_preparation_shared("map_json5_with", map_small, decode);
}

#[test]
fn xml_endpoint_per_call_cost_attribution() {
    let schema = order_schema();
    let descriptor = ok(
        "XML schema encodes",
        codegen_schema::encode(&schema, MAX_EMBEDDED_XML_SCHEMA_BYTES),
    );
    let small_instance = order_instance(&schema, 3);
    let large_instance = order_instance(&schema, 20_000);
    let serialize =
        |instance: &Instance| serialize_xml_document(&descriptor, instance, true, true, None, None);
    let small = ok("small XML serializes", serialize(&small_instance));
    let large = ok("large XML serializes", serialize(&large_instance));
    assert_eq!(
        ok(
            "large XML parses",
            parse_structured_xml(&descriptor, &large)
        ),
        large_instance
    );
    println!(
        "XML: descriptor {} bytes; small document {} bytes; large document {} bytes",
        descriptor.len(),
        small.len(),
        large.len(),
    );
    header();
    let decode = measure(SMALL, || {
        codegen_schema::decode(&descriptor, MAX_EMBEDDED_XML_SCHEMA_BYTES)
    });
    row("descriptor decode (`codegen_schema::decode`)", decode);
    let (_, live) = retained(|| codegen_schema::decode(&descriptor, MAX_EMBEDDED_XML_SCHEMA_BYTES));
    println!("| live bytes retained by one decoded descriptor | | | {live} | |");
    let parse_small = measure(SMALL, || parse_structured_xml(&descriptor, &small));
    row("`parse_structured_xml`, small", parse_small);
    let envelope = observed_descriptor();
    let envelope_document = r#"<Envelope Token="A&amp;B"/>"#;
    let envelope_instance = ok(
        "observed XML parses",
        parse_xml(&envelope, envelope_document, true, true),
    );
    row(
        "`parse_xml` (observed flat root), small",
        measure(SMALL, || {
            parse_xml(&envelope, envelope_document, true, true)
        }),
    );
    let serialize_small = measure(SMALL, || serialize(&small_instance));
    row("`serialize_xml_document`, small", serialize_small);
    let hints = r#"{"no_namespace_location":"order.xsd"}"#;
    row(
        "`serialize_xml_document` with schema hints (observed flat root), small",
        measure(SMALL, || {
            serialize_xml_document(
                &envelope,
                &envelope_instance,
                false,
                false,
                None,
                Some(hints),
            )
        }),
    );
    let node_small = measure(SMALL, || {
        serialize_xml(1, &descriptor, &small_instance, false, false, None)
    });
    row("`serialize_xml` (expression node), small", node_small);
    row(
        "`parse_structured_xml`, large",
        measure(LARGE, || parse_structured_xml(&descriptor, &large)),
    );
    row(
        "`parse_structured_xml_bytes`, large",
        measure(LARGE, || {
            parse_structured_xml_bytes(&descriptor, large.as_bytes())
        }),
    );
    row(
        "`format_xml::from_str_structured`, large",
        measure(LARGE, || format_xml::from_str_structured(&large, &schema)),
    );
    row(
        "`serialize_xml_document`, large",
        measure(LARGE, || serialize(&large_instance)),
    );
    let options = format_xml::XmlWriteOptions {
        scalar_output: None,
        declaration: true,
        indent: true,
        default_namespace: None,
        schema_hints: None,
    };
    row(
        "`format_xml::to_string_with_options`, large",
        measure(LARGE, || {
            format_xml::to_string_with_options(&schema, &large_instance, &options)
        }),
    );
    for (label, cost) in [
        ("parse_structured_xml", parse_small),
        ("serialize_xml_document", serialize_small),
        ("serialize_xml", node_small),
    ] {
        assert_preparation_shared(label, cost, decode);
    }
}

#[test]
fn x12_endpoint_per_call_cost_attribution() {
    let descriptor = x12_descriptor();
    let small = x12_document(2);
    let large = x12_document(20_000);
    let small_instance = ok("small X12 parses", parse_x12(&descriptor, &small));
    let large_instance = ok("large X12 parses", parse_x12(&descriptor, &large));
    let output = ok(
        "large X12 serializes",
        serialize_x12(&descriptor, &large_instance, None),
    );
    assert_eq!(
        ok("large X12 output parses", parse_x12(&descriptor, &output)),
        large_instance
    );
    println!(
        "X12: descriptor {} bytes; small document {} bytes; large document {} bytes; large output {} bytes",
        descriptor.len(),
        small.len(),
        large.len(),
        output.len()
    );
    header();
    row(
        "`parse_x12`, small",
        measure(SMALL, || parse_x12(&descriptor, &small)),
    );
    row(
        "`serialize_x12`, small",
        measure(SMALL, || serialize_x12(&descriptor, &small_instance, None)),
    );
    row(
        "`parse_x12`, large",
        measure(LARGE, || parse_x12(&descriptor, &large)),
    );
    row(
        "`parse_x12_bytes`, large",
        measure(LARGE, || parse_x12_bytes(&descriptor, large.as_bytes())),
    );
    row(
        "`serialize_x12`, large",
        measure(LARGE, || serialize_x12(&descriptor, &large_instance, None)),
    );
    row(
        "`serialize_x12_bytes`, large",
        measure(LARGE, || {
            serialize_x12_bytes(&descriptor, &large_instance, None)
        }),
    );
    row(
        "instance clone, large",
        measure(LARGE, || large_instance.clone()),
    );
}

/// Repeated calls share the prepared descriptor: a small document costs
/// less than decoding the descriptor once. Counts are deterministic for one
/// toolchain; timings are informational only.
fn assert_preparation_shared(label: &str, call: Cost, decode: Cost) {
    assert!(
        call.allocations < decode.allocations && call.allocated < decode.allocated,
        "{label}: {call:?} versus one descriptor decode {decode:?}"
    );
}
