use super::*;

pub(super) fn fixture() -> Fixture {
    let fixture = invoked_qualifier_fixture();
    fixture.change(
        r#"<vertex vertexkey="188"><edges><edge vertexkey="602"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="602"/></edges></vertex>"#,
    );
    fixture.change(r#"<entry name="LIN">"#, r#"<entry name="LIN" inpkey="84">"#);
    fixture.change(
        r#"<vertex vertexkey="603"><edges><edge vertexkey="85"/></edges></vertex>"#,
        r#"<vertex vertexkey="603"><edges><edge vertexkey="84"/></edges></vertex><vertex vertexkey="604"><edges><edge vertexkey="85"/></edges></vertex>"#,
    );
    let path = fixture.0.join("850.Config");
    let config = std::fs::read_to_string(&path).unwrap().replace(
        r#"<Segment ref="LIN"/>"#,
        r#"<Segment ref="LIN" maxOccurs="unbounded"/>"#,
    );
    std::fs::write(path, config).unwrap();
    fixture
}

#[test]
fn nullable_qualifier_controls_one_group_per_detail_not_one_per_value() {
    for empty_qualifier in [false, true] {
        let fixture = fixture();
        if empty_qualifier {
            fixture.change(
                r#"value="QUAL" datatype="string""#,
                r#"value="" datatype="string""#,
            );
        }
        let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
        .unwrap();
        let mut parameters = engine::RuntimeParameters::new();
        parameters
            .insert(
                &imported.invocation.key_parameter,
                Value::String("P".into()),
            )
            .unwrap();
        let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
        let grouped = mfd::ImportedGroupedJsonEdiDocument {
            document: imported.document,
            source: imported.source,
            writer_minimums: imported.writer_minimums,
        };
        for (json, expected) in [
            (
                r#"{"rows":[{"name":"a","qty":"FIRST","qty":"SECOND"}]}"#,
                vec![vec!["FIRST"]],
            ),
            (
                r#"{"rows":[{"name":"a","qty":"","qty":"SECOND"}]}"#,
                vec![vec!["SECOND"]],
            ),
            (
                r#"{"rows":[{"name":"a","qty":" ","qty":"SECOND"}]}"#,
                vec![vec![" "]],
            ),
            (
                r#"{"rows":[{"name":"a","qty":"","qty":""}]}"#,
                vec![vec![""]],
            ),
            (
                r#"{"rows":[{"name":"a","qty":null,"qty":"SECOND"}]}"#,
                vec![vec!["SECOND"]],
            ),
            (
                r#"{"rows":[{"name":"a","qty":"FIRST","qty":null}]}"#,
                vec![vec!["FIRST"]],
            ),
            (
                r#"{"rows":[{"name":"a"},{"name":"a","qty":"SECOND","qty":"THIRD"}]}"#,
                vec![vec![], vec!["SECOND"]],
            ),
        ] {
            let source = input(&grouped, &[json]);
            let Instance::DocumentSet(documents) =
                engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
            else {
                panic!("documents")
            };
            assert_eq!(documents.len(), 1);
            let interchange = &documents[0]
                .value()
                .field("Interchange")
                .unwrap()
                .as_repeated()
                .unwrap()[0];
            let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message = group.field("Message_850").unwrap();
            let message = message.as_repeated().map_or(message, |items| &items[0]);
            let actual = message
                .field("Lines")
                .unwrap()
                .as_repeated()
                .unwrap()
                .iter()
                .map(|line| {
                    line.field("LIN")
                        .and_then(Instance::as_repeated)
                        .unwrap_or(&[])
                        .iter()
                        .map(|lin| {
                            let Some(Instance::Scalar(Value::String(text))) = lin.field("F1")
                            else {
                                panic!("field")
                            };
                            text.as_str()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{json}");
        }
        assert!(codegen::lower_document(&grouped.document).is_ok());
    }
}
