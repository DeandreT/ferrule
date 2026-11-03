use codegen_runtime::{
    ExecutionContext, FilterMapLimits, Instance, RuntimeParameters, SourceContractReason as R,
    SourceContractViolation, Value, repeated, scalar,
};
use guard_host::*;
#[path = "document_json.rs"]
mod document_json;
#[path = "document_xml.rs"]
mod document_xml;
#[path = "unicode.rs"]
mod unicode;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

static CASES: AtomicUsize = AtomicUsize::new(0);
static LOADS: AtomicUsize = AtomicUsize::new(0);
struct Loader;
impl codegen_runtime::DynamicSourceLoader for Loader {
    fn load(&self, _: &str, _: &str) -> Result<Instance, String> {
        LOADS.fetch_add(1, Ordering::SeqCst);
        panic!("loader ran")
    }
}
impl codegen_runtime::DynamicJsonSourceLoader for Loader {
    fn load(&self, _: &str, _: &str) -> Result<Vec<u8>, String> {
        LOADS.fetch_add(1, Ordering::SeqCst);
        panic!("JSON loader ran")
    }
}
impl codegen_runtime::PrimarySourceHost for Loader {
    type Error = std::io::Error;
    fn select(
        &self,
        _: &str,
        _: codegen_runtime::PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error> {
        LOADS.fetch_add(1, Ordering::SeqCst);
        panic!("primary selection ran")
    }
    fn load(&self, _: &str) -> Result<codegen_runtime::SourceData, Self::Error> {
        LOADS.fetch_add(1, Ordering::SeqCst);
        panic!("primary load ran")
    }
}
fn reject<T, E: Error + 'static>(result: Result<T, E>, expected: R) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("guard was bypassed"),
    };
    let mut current: &(dyn Error + 'static) = &error;
    loop {
        if let Some(error) = current.downcast_ref::<SourceContractViolation>() {
            assert_eq!(error.reason, expected);
            assert!(error.path.iter().all(|name| !name.contains("secret")));
            CASES.fetch_add(1, Ordering::SeqCst);
            return;
        }
        current = current
            .source()
            .unwrap_or_else(|| panic!("no typed violation: {error}"));
    }
}
fn reject_control<T, E: Error + 'static>(
    result: Result<codegen_runtime::FilterMapExecution<T, E>, E>,
    expected: R,
) {
    match result {
        Ok(run) => reject(run.outcome, expected),
        Err(error) => reject::<(), _>(Err(error), expected),
    }
}
fn s(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}
fn group<const N: usize>(fields: [(&str, Instance); N]) -> Instance {
    codegen_runtime::group(fields.map(|(name, value)| (name.to_owned(), value)))
}
fn input() -> Instance {
    group([
        ("UnicodeSource", s("B")),
        ("AsciiSource", s("A")),
        ("Text", s("ROOT")),
        (
            "Row",
            repeated([
                group([
                    ("Value", s("OUTER1")),
                    (
                        "Cell",
                        repeated([
                            group([("Value", s("INNER1"))]),
                            group([("Value", s("INNER2"))]),
                        ]),
                    ),
                ]),
                group([("Value", s("OUTER2")), ("Cell", repeated([]))]),
            ]),
        ),
        ("Cell", repeated([group([("Value", s("ROOT-CELL"))])])),
    ])
}
fn expected() -> Instance {
    group([
        ("Root", s("ROOT")),
        ("UnicodeA", s("A")),
        ("UnicodeB", s("B")),
        (
            "Rows",
            repeated([
                group([
                    ("Root", s("ROOT")),
                    ("Result", s("OUTER1")),
                    (
                        "Child",
                        repeated([
                            group([("Result", s("OUTER1"))]),
                            group([("Result", s("OUTER1"))]),
                        ]),
                    ),
                ]),
                group([
                    ("Root", s("ROOT")),
                    ("Result", s("OUTER2")),
                    ("Child", repeated([])),
                ]),
            ]),
        ),
    ])
}
const JSON: &str = r#"{"Text":"ROOT","AsciiSource":"A","UnicodeSource":"B","Row":[{"Value":"OUTER1","Cell":[{"Value":"INNER1"},{"Value":"INNER2"}]},{"Value":"OUTER2","Cell":[]}],"Cell":[{"Value":"ROOT-CELL"}]}"#;
const XML: &str = "<Input><Text>ROOT</Text><AsciiSource>A</AsciiSource><UnicodeSource>B</UnicodeSource><Row><Value>OUTER1</Value><Cell><Value>INNER1</Value></Cell><Cell><Value>INNER2</Value></Cell></Row><Row><Value>OUTER2</Value></Row><Cell><Value>ROOT-CELL</Value></Cell></Input>";

fn main() {
    let unicode_source = group([("X\u{10000}", s("B")), ("X\u{ff21}", s("A"))]);
    assert_eq!(
        unicode::execute(&unicode_source).unwrap(),
        group([("A", s("A")), ("B", s("B"))])
    );
    reject(
        unicode::execute(&group([("X\u{ff21}", s("A")), ("X\u{ff21}", s("A"))])),
        R::DuplicateField,
    );
    reject(
        unicode::execute(&group([("X\u{ff22}", s("secret"))])),
        R::UnexpectedField,
    );
    CASES.fetch_add(1, Ordering::SeqCst);
    let valid = input();
    for _ in 0..3 {
        assert_eq!(execute(&valid).unwrap(), expected());
        let outputs = execute_outputs(&valid).unwrap();
        assert_eq!(outputs.primary, expected());
        assert_eq!(outputs.extras.len(), 1);
        assert_eq!(outputs.extras[0].name, "audit");
        assert_eq!(outputs.extras[0].instance, group([("Root", s("ROOT"))]));
        match execute_selected_target(&valid, TargetSelection::Named("audit")).unwrap() {
            SelectedTargetOutput::Named(value) => {
                assert_eq!(value.instance, group([("Root", s("ROOT"))]))
            }
            _ => panic!(),
        }
        CASES.fetch_add(3, Ordering::SeqCst);
    }
    let mut sparse = input();
    let Instance::Group(root) = &mut sparse else {
        panic!()
    };
    let Instance::Repeated(rows) = &mut root.iter_mut().find(|(name, _)| name == "Row").unwrap().1
    else {
        panic!()
    };
    let Instance::Group(row) = &mut rows[0] else {
        panic!()
    };
    row.retain(|(name, _)| name != "Cell");
    reject(execute(&sparse), R::MissingRepeatedSlot);
    let mut hidden = input();
    let Instance::Group(root) = &mut hidden else {
        panic!()
    };
    let Instance::Repeated(rows) = &mut root.iter_mut().find(|(name, _)| name == "Row").unwrap().1
    else {
        panic!()
    };
    let Instance::Group(row) = &mut rows[0] else {
        panic!()
    };
    row.push(("Text".into(), s("ROOT")));
    reject(execute(&hidden), R::UnexpectedField);
    for value in [
        scalar(Value::Null),
        repeated([]),
        Instance::MappedSequence(Vec::new()),
        Instance::DocumentSet(Vec::new()),
    ] {
        reject(execute(&value), R::WrongShape);
    }
    for value in [
        Value::json_null(),
        Value::Int(1),
        Value::Float(f64::NAN),
        Value::decimal(codegen_runtime::Decimal96::parse_exact("1").unwrap()),
    ] {
        let mut source = input();
        let Instance::Group(root) = &mut source else {
            panic!()
        };
        root.iter_mut().find(|(name, _)| name == "Text").unwrap().1 = scalar(value);
        reject(execute(&source), R::WrongScalarDomain);
    }
    let mut duplicate = input();
    let Instance::Group(root) = &mut duplicate else {
        panic!()
    };
    root.push(("AsciiSource".into(), s("A")));
    reject(execute(&duplicate), R::DuplicateField);
    let mut unknown = input();
    let Instance::Group(root) = &mut unknown else {
        panic!()
    };
    root.push(("X\u{ff22}".into(), s("secret")));
    reject(execute(&unknown), R::UnexpectedField);

    let loader = Loader;
    let mut params = RuntimeParameters::new();
    params
        .insert("undeclared", Value::String("secret".into()))
        .unwrap();
    let execution = ExecutionContext::new(std::path::Path::new("mapping")).with_parameters(&params);
    let empty = ExecutionContext::new(std::path::Path::new("mapping"));
    reject(
        document_xml::evaluate_primary_filename(&execution),
        R::UnexpectedParameter,
    );
    reject(evaluate_output_filename(&execution), R::UnexpectedParameter);
    reject(
        document_json::evaluate_primary_filename(&execution),
        R::UnexpectedParameter,
    );
    reject(
        evaluate_primary_filename(&execution),
        R::UnexpectedParameter,
    );
    reject(
        document_xml::execute_primary_xml_document_bytes(&execution, &loader, 1),
        R::UnexpectedParameter,
    );
    reject(
        document_json::execute_primary_json_document_bytes(&execution, &loader, 1),
        R::UnexpectedParameter,
    );
    assert_eq!(
        document_xml::evaluate_primary_filename(&empty).unwrap(),
        "input.xml"
    );
    assert_eq!(evaluate_output_filename(&empty).unwrap(), "output.xml");
    let extras = [
        NamedInput {
            name: "unknown",
            instance: &valid,
        },
        NamedInput {
            name: "unknown",
            instance: &valid,
        },
    ];
    for extras in [&extras[..1], &extras[..]] {
        reject(execute_with_sources(&valid, extras), R::UnexpectedBinding);
        reject(
            execute_outputs_with_sources(&valid, extras),
            R::UnexpectedBinding,
        );
        reject(
            execute_with_sources_and_context(&valid, extras, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_outputs_with_sources_and_context(&valid, extras, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_with_sources_and_dynamic_source_loader(&valid, extras, &loader),
            R::UnexpectedBinding,
        );
        reject(
            execute_outputs_with_sources_and_dynamic_source_loader(&valid, extras, &loader),
            R::UnexpectedBinding,
        );
        reject(
            execute_with_sources_context_and_dynamic_source_loader(&valid, extras, &empty, &loader),
            R::UnexpectedBinding,
        );
        reject(
            execute_outputs_with_sources_context_and_dynamic_source_loader(
                &valid, extras, &empty, &loader,
            ),
            R::UnexpectedBinding,
        );
        reject(
            execute_selected_target_with_host(
                &valid,
                extras,
                None,
                Some(&loader),
                TargetSelection::Primary,
            ),
            R::UnexpectedBinding,
        );
        reject_control(
            execute_outputs_with_filter_map_controls(
                &valid,
                extras,
                None,
                Some(&loader),
                FilterMapLimits::default(),
                None,
            ),
            R::UnexpectedBinding,
        );
        reject_control(
            execute_selected_target_with_filter_map_controls(
                &valid,
                extras,
                None,
                Some(&loader),
                TargetSelection::Named("audit"),
                FilterMapLimits::default(),
                None,
            ),
            R::UnexpectedBinding,
        );
    }
    reject(
        execute_with_context(&valid, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_outputs_with_context(&valid, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_with_sources_and_context(&valid, &[], &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_outputs_with_sources_and_context(&valid, &[], &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_with_sources_context_and_dynamic_source_loader(&valid, &[], &execution, &loader),
        R::UnexpectedParameter,
    );
    reject(
        execute_outputs_with_sources_context_and_dynamic_source_loader(
            &valid,
            &[],
            &execution,
            &loader,
        ),
        R::UnexpectedParameter,
    );
    reject(
        execute_selected_target_with_host(
            &valid,
            &[],
            Some(&execution),
            Some(&loader),
            TargetSelection::Named("audit"),
        ),
        R::UnexpectedParameter,
    );
    reject_control(
        execute_outputs_with_filter_map_controls(
            &valid,
            &[],
            Some(&execution),
            Some(&loader),
            FilterMapLimits::default(),
            None,
        ),
        R::UnexpectedParameter,
    );
    reject_control(
        execute_selected_target_with_filter_map_controls(
            &valid,
            &[],
            Some(&execution),
            Some(&loader),
            TargetSelection::Primary,
            FilterMapLimits::default(),
            None,
        ),
        R::UnexpectedParameter,
    );
    reject(
        execute_with_dynamic_source_loader(&hidden, &loader),
        R::UnexpectedField,
    );
    reject(
        execute_outputs_with_dynamic_source_loader(&hidden, &loader),
        R::UnexpectedField,
    );

    let json_extras = [NamedJsonInput {
        name: "secret",
        document: "not-json",
    }; 2];
    let json_bytes_extras = [NamedJsonBytesInput {
        name: "secret",
        document: b"not-json",
    }; 2];
    for n in [1, 2] {
        let inputs = &json_extras[..n];
        let bytes = &json_bytes_extras[..n];
        reject(
            execute_json_with_sources(JSON, inputs),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_outputs_with_sources(JSON, inputs),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_with_sources_and_context(JSON, inputs, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_outputs_with_sources_and_context(JSON, inputs, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_bytes_with_sources(JSON.as_bytes(), bytes),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_bytes_outputs_with_sources(JSON.as_bytes(), bytes),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_bytes_with_sources_and_context(JSON.as_bytes(), bytes, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_bytes_outputs_with_sources_and_context(JSON.as_bytes(), bytes, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_selected_target_with_host(
                JSON,
                inputs,
                None,
                Some(&loader),
                TargetSelection::Primary,
            ),
            R::UnexpectedBinding,
        );
        reject(
            execute_json_bytes_selected_target_with_host(
                JSON.as_bytes(),
                bytes,
                None,
                Some(&loader),
                TargetSelection::Named("audit"),
            ),
            R::UnexpectedBinding,
        );
        reject_control(
            execute_json_outputs_with_filter_map_controls(
                JSON,
                inputs,
                None,
                FilterMapLimits::default(),
                None,
            ),
            R::UnexpectedBinding,
        );
        reject_control(
            execute_json_bytes_outputs_with_filter_map_controls(
                JSON.as_bytes(),
                bytes,
                None,
                FilterMapLimits::default(),
                None,
            ),
            R::UnexpectedBinding,
        );
    }
    reject(
        execute_json_with_context(JSON, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_json_outputs_with_context(JSON, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_json_bytes_with_context(JSON.as_bytes(), &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_json_bytes_outputs_with_context(JSON.as_bytes(), &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_json_selected_target_with_host(
            JSON,
            &[],
            Some(&execution),
            Some(&loader),
            TargetSelection::Primary,
        ),
        R::UnexpectedParameter,
    );
    reject(
        execute_json_bytes_selected_target_with_host(
            JSON.as_bytes(),
            &[],
            Some(&execution),
            Some(&loader),
            TargetSelection::Named("audit"),
        ),
        R::UnexpectedParameter,
    );
    reject_control(
        execute_json_selected_target_with_filter_map_controls(
            JSON,
            &[],
            Some(&execution),
            None,
            TargetSelection::Primary,
            FilterMapLimits::default(),
            None,
        ),
        R::UnexpectedParameter,
    );
    reject_control(
        execute_json_bytes_selected_target_with_filter_map_controls(
            JSON.as_bytes(),
            &[],
            Some(&execution),
            None,
            TargetSelection::Primary,
            FilterMapLimits::default(),
            None,
        ),
        R::UnexpectedParameter,
    );

    let xml_extras = [NamedXmlInput {
        name: "secret",
        document: "not-xml",
    }; 2];
    let xml_bytes_extras = [NamedXmlBytesInput {
        name: "secret",
        document: b"not-xml",
    }; 2];
    for n in [1, 2] {
        let inputs = &xml_extras[..n];
        let bytes = &xml_bytes_extras[..n];
        reject(execute_xml_with_sources(XML, inputs), R::UnexpectedBinding);
        reject(
            execute_xml_outputs_with_sources(XML, inputs),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_with_sources_and_context(XML, inputs, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_outputs_with_sources_and_context(XML, inputs, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_bytes_with_sources(XML.as_bytes(), bytes),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_bytes_outputs_with_sources(XML.as_bytes(), bytes),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_bytes_with_sources_and_context(XML.as_bytes(), bytes, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_bytes_outputs_with_sources_and_context(XML.as_bytes(), bytes, &empty),
            R::UnexpectedBinding,
        );
        reject(
            execute_xml_bytes_selected_target_with_host(
                XML.as_bytes(),
                bytes,
                None,
                TargetSelection::Named("audit"),
            ),
            R::UnexpectedBinding,
        );
    }
    reject(
        execute_xml_with_context(XML, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_xml_outputs_with_context(XML, &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_xml_bytes_with_context(XML.as_bytes(), &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_xml_bytes_outputs_with_context(XML.as_bytes(), &execution),
        R::UnexpectedParameter,
    );
    reject(
        execute_xml_bytes_selected_target_with_host(
            XML.as_bytes(),
            &[],
            Some(&execution),
            TargetSelection::Primary,
        ),
        R::UnexpectedParameter,
    );
    assert!(execute_json(JSON).unwrap().contains("OUTER1"));
    assert!(execute_xml(XML).unwrap().contains("OUTER1"));
    assert_eq!(LOADS.load(Ordering::SeqCst), 0);
    println!(
        "guard-host-ok cases={} loads=0",
        CASES.load(Ordering::SeqCst)
    );
}
