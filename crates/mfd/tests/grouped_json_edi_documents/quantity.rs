use super::*;

pub(super) fn fixture() -> Fixture {
    let fixture = object_invoked_fixture();
    std::fs::write(fixture.0.join("source.json"), r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"qty":{"type":"integer"},"uom":{"type":"string"},"units":{"type":"integer"}}}}}}"#).unwrap();
    fixture.change(r#"<entry name="qty" type="json-property"><entry name="number" outkey="22"/></entry>"#,
        r#"<entry name="qty" type="json-property"><entry name="number" outkey="22"/></entry><entry name="uom" type="json-property"><entry name="string" outkey="23"/></entry><entry name="units" type="json-property"><entry name="number" outkey="24"/></entry>"#);
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        "",
    );
    fixture.change(
        r#"<vertex vertexkey="152"><edges><edge vertexkey="86"/></edges></vertex>"#,
        "",
    );
    fixture.change("</children>", r#"<component name="constant" library="core" kind="2" uid="60"><targets><datapoint pos="0" key="200"/></targets><data><constant datatype="string" value="CS"/></data></component>
<component name="equal" library="core" kind="5" uid="61"><sources><datapoint pos="0" key="201"/><datapoint pos="1" key="202"/></sources><targets><datapoint pos="0" key="203"/></targets></component>
<component name="divide" library="core" kind="5" uid="62"><sources><datapoint pos="0" key="204"/><datapoint pos="1" key="205"/></sources><targets><datapoint pos="0" key="206"/></targets></component>
<component name="floor" library="core" kind="5" uid="63"><sources><datapoint pos="0" key="207"/></sources><targets><datapoint pos="0" key="208"/></targets></component>
<component name="if-else" library="core" kind="4" uid="64"><sources><datapoint pos="0" key="209"/><datapoint pos="1" key="210"/><datapoint pos="2" key="211"/></sources><targets><datapoint pos="0" key="212"/></targets></component>
<component name="sum" library="core" kind="5" uid="65"><sources><datapoint/><datapoint pos="1" key="214"/></sources><targets><datapoint pos="0" key="215"/></targets></component></children>"#);
    fixture.change("</vertices>", r#"<vertex vertexkey="23"><edges><edge vertexkey="201"/></edges></vertex><vertex vertexkey="200"><edges><edge vertexkey="202"/></edges></vertex><vertex vertexkey="203"><edges><edge vertexkey="209"/></edges></vertex><vertex vertexkey="22"><edges><edge vertexkey="204"/><edge vertexkey="211"/></edges></vertex><vertex vertexkey="24"><edges><edge vertexkey="205"/></edges></vertex><vertex vertexkey="206"><edges><edge vertexkey="207"/></edges></vertex><vertex vertexkey="208"><edges><edge vertexkey="210"/></edges></vertex><vertex vertexkey="212"><edges><edge vertexkey="85"/><edge vertexkey="214"/></edges></vertex><vertex vertexkey="215"><edges><edge vertexkey="86"/></edges></vertex></vertices>"#);
    fixture
}

#[test]
fn integer_quantity_keeps_cartesian_last_before_outer_selection_and_sum() {
    let fixture = fixture();
    let imported = mfd::import_object_invoked_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let prepared = codegen_runtime::PreparedJsonParameterInvocations::prepare(
        &imported.invocation,
        &Value::String(r#"{"projectCode":"P","fileName":"a","control":"100"}"#.into()),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let invocation = prepared.invocations().next().unwrap();
    let parameter = codegen_runtime::project_json_source_value(
        &imported.invocation.projection,
        invocation.parameter_object(),
        Default::default(),
    )
    .unwrap()
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    let (key, value) = invocation.key_binding();
    parameters.insert(key, value).unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let parameter_name = imported.parameter_source_name.clone();
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    for (fields, quantity, total) in [
        (r#""uom":"CS","qty":7.9,"units":3.9"#, Some("2"), "2"),
        (r#""uom":"CS","qty":-7,"units":3"#, Some("-3"), "-3"),
        (
            r#""uom":"CS","qty":2,"qty":9,"units":3,"units":2"#,
            Some("4"),
            "4",
        ),
        (
            r#""uom":"CS","uom":"EA","qty":9,"units":3"#,
            Some("3"),
            "12",
        ),
        (
            r#""uom":"EA","uom":"CS","qty":9,"units":3"#,
            Some("9"),
            "12",
        ),
        (r#""uom":"CS","qty":9"#, None, "0"),
        (r#""qty":9,"units":3"#, None, "0"),
        (r#""uom":"CS","qty":"9","units":3"#, None, "0"),
        (r#""uom":"EA","qty":9,"units":0"#, Some("9"), "9"),
        (
            r#""uom":"CS","qty":71305346262837903834189555302,"units":9"#,
            Some("7922816251426433759354395034"),
            "7922816251426433759354395034",
        ),
    ] {
        let text = format!(r#"{{"rows":[{{"name":"a",{fields}}}]}}"#);
        let source = input(&grouped, &[&text]);
        let result = engine::run_with_sources_and_context(
            grouped.document.project(),
            &source,
            vec![(parameter_name.clone(), parameter.clone())],
            &context,
        )
        .unwrap();
        let Instance::DocumentSet(documents) = result else {
            panic!("documents");
        };
        let interchange = &documents[0]
            .value()
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()[0];
        let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
        let message = group.field("Message_850").unwrap();
        let message = message.as_repeated().map_or(message, |items| &items[0]);
        let line = &message.field("Lines").unwrap().as_repeated().unwrap()[0];
        let lin = line.field("LIN").unwrap();
        let actual = lin
            .field("F1")
            .map(|value| match value {
                Instance::Scalar(Value::String(text)) => text.as_str(),
                _ => panic!("text quantity"),
            })
            .filter(|text| !text.is_empty());
        assert_eq!(actual, quantity, "{fields}");
        assert_eq!(
            lin.field("F2"),
            Some(&Instance::Scalar(Value::decimal(
                ir::Decimal96::parse_exact(total).unwrap()
            ))),
            "{fields}"
        );
    }
    for fields in [
        r#""uom":"CS","qty":9,"units":0"#,
        r#""uom":"CS","qty":9,"units":3,"units":0"#,
        r#""uom":"EA","uom":"CS","qty":9,"units":0"#,
    ] {
        let text = format!(r#"{{"rows":[{{"name":"a",{fields}}}]}}"#);
        let source = input(&grouped, &[&text]);
        assert!(
            engine::run_with_sources_and_context(
                grouped.document.project(),
                &source,
                vec![(parameter_name.clone(), parameter.clone())],
                &context
            )
            .is_err(),
            "{fields}"
        );
    }
    assert!(codegen::lower_document(&grouped.document).is_ok());
}
