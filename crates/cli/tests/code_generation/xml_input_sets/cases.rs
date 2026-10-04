use super::*;

pub(super) const CURRENT: &str = "2026-10-04T00:00:00Z";
pub(super) const PRIMARY: &str = include_str!("../fixtures/xml_input_sets/primary.xml");
pub(super) const RATES: &str = include_str!("../fixtures/xml_input_sets/rates.xml");
pub(super) const LABELS: &str = include_str!("../fixtures/xml_input_sets/labels.xml");
pub(super) const VARIANTS: [&str; 10] = [
    "base",
    "context",
    "named-output",
    "phase-order",
    "policies",
    "reverse-policies",
    "reversed-declarations",
    "zero",
    "single",
    "unreachable",
];
pub(super) const PUBLIC_APIS: [&str; 16] = [
    "xml-outputs-sources",
    "xml-outputs-sources-context",
    "bytes-outputs-sources",
    "bytes-outputs-sources-context",
    "xml-sources",
    "xml-sources-context",
    "bytes-sources",
    "bytes-sources-context",
    "xml-outputs",
    "xml-outputs-context",
    "bytes-outputs",
    "bytes-outputs-context",
    "xml",
    "xml-context",
    "bytes",
    "bytes-context",
];

#[derive(Clone)]
pub(super) struct Input {
    pub(super) name: &'static str,
    pub(super) schema: &'static str,
    pub(super) bytes: Vec<u8>,
}
#[derive(Clone)]
pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) primary: Vec<u8>,
    pub(super) inputs: Vec<Input>,
    pub(super) diagnostics: bool,
}
impl Case {
    fn nominal(name: &'static str) -> Self {
        Self {
            name,
            primary: PRIMARY.as_bytes().to_vec(),
            inputs: vec![
                Input {
                    name: "rates",
                    schema: "rates",
                    bytes: RATES.as_bytes().to_vec(),
                },
                Input {
                    name: "labels",
                    schema: "labels",
                    bytes: LABELS.as_bytes().to_vec(),
                },
            ],
            diagnostics: true,
        }
    }
    fn replace(&mut self, owner: &str, from: &str, to: &str) {
        let bytes = if owner == "primary" {
            &mut self.primary
        } else {
            &mut self
                .inputs
                .iter_mut()
                .find(|input| input.schema == owner)
                .expect("fixture owner")
                .bytes
        };
        let old = std::str::from_utf8(bytes).expect("fixture UTF8");
        assert!(old.contains(from), "exact fixture mutation");
        *bytes = old.replace(from, to).into_bytes();
    }
    pub(super) fn json(&self) -> Json {
        json!({"case":self.name,"primary":self.primary,"inputs":self.inputs.iter().map(|input|json!({"name":input.name,"schema":input.schema,"bytes":input.bytes})).collect::<Vec<_>>(),"diagnostics":self.diagnostics})
    }
    pub(super) fn text_api(&self, with_sources: bool) -> bool {
        std::str::from_utf8(&self.primary).is_ok()
            && (!with_sources
                || self
                    .inputs
                    .iter()
                    .all(|input| std::str::from_utf8(&input.bytes).is_ok()))
    }
}
pub(super) fn cases(variant: &str) -> Vec<Case> {
    let names: &[&'static str] = match variant {
        "base" => &[
            "nominal",
            "reverse-inputs",
            "namespace-alias",
            "unicode",
            "missing-first-group",
            "missing-second-group",
            "nil-adjustment",
            "empty-rates",
            "empty-labels",
            "missing-factor",
            "primary-bad-number",
            "rates-bad-number",
            "rates-wrong-namespace",
            "labels-malformed",
            "two-malformed-forward",
            "two-malformed-reverse",
            "primary-malformed",
            "missing-rates",
            "missing-labels",
            "duplicate-rates",
            "unexpected-name",
            "wrong-case",
            "whitespace-name",
            "primary-invalid-utf8",
            "rates-invalid-utf8",
            "labels-invalid-utf8",
            "swapped-documents",
            "count-over",
        ],
        "context" => &["nominal", "need-context"],
        "named-output" => &["nonintegral-factor"],
        "phase-order" => &["competing-failures"],
        "reversed-declarations" => &["nominal", "two-malformed-reverse"],
        "zero" => &["nominal", "primary-bad-number", "primary-invalid-utf8"],
        "unreachable" => &["nominal", "missing-spare"],
        _ => &["nominal"],
    };
    names.iter().map(|&name| {
        let mut case=Case::nominal(name);
        match name {
            "reverse-inputs"=>case.inputs.reverse(),
            "namespace-alias"=>{case.replace("rates","r:","p:");case.replace("rates","xmlns:r","xmlns:p");},
            "unicode"=>{case.replace("rates","Code=\"A\"","Code=\"A😀&amp;\"");case.replace("labels","Ω","Ω😀&amp;");},
            "missing-first-group"=>case.replace("rates","<r:Details><r:Adjustment>2</r:Adjustment></r:Details>",""),
            "missing-second-group"=>case.replace("rates","<r:Details><r:Adjustment>6.5</r:Adjustment></r:Details>",""),
            "nil-adjustment"=>case.replace("rates","<r:Adjustment>2</r:Adjustment>","<r:Adjustment xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:nil=\"true\"/>"),
            "empty-rates"=>case.inputs[0].bytes=b"<Rates xmlns=\"urn:rates\" Revision=\"3\"><Factor>4</Factor></Rates>".to_vec(),
            "empty-labels"=>case.inputs[1].bytes=b"<Labels/>".to_vec(),
            "missing-factor"=>case.replace("rates","<r:Factor>4</r:Factor>",""),
            "primary-bad-number"=>case.replace("primary","2.5","bad"),
            "rates-bad-number"=>case.replace("rates","1.25","bad"),
            "rates-wrong-namespace"=>case.replace("rates","urn:rates","urn:wrong"),
            "labels-malformed"=>case.inputs[1].bytes=b"<Labels>".to_vec(),
            "two-malformed-forward"|"two-malformed-reverse"=>{case.inputs[0].bytes=b"<Rates>".to_vec();case.inputs[1].bytes=b"<Labels>".to_vec();if name.ends_with("reverse") {case.inputs.reverse();}},
            "primary-malformed"=>{case.primary=b"<Input>".to_vec();case.inputs[0].bytes=b"<Rates>".to_vec();case.inputs[1].bytes=b"<Labels>".to_vec();},
            "missing-rates"=>{case.primary=b"<Input>".to_vec();case.inputs.remove(0);},
            "missing-labels"=>{case.primary=b"<Input>".to_vec();case.inputs.remove(1);},
            "duplicate-rates"=>{case.primary=b"<Input>".to_vec();case.inputs[1]=case.inputs[0].clone();},
            "unexpected-name"=>{case.primary=b"<Input>".to_vec();case.inputs[1].name="unknown";},
            "wrong-case"=>case.inputs[0].name="Rates",
            "whitespace-name"=>case.inputs[0].name=" rates",
            "primary-invalid-utf8"=>case.primary=b"<Input><Value>\xff</Value></Input>".to_vec(),
            "rates-invalid-utf8"=>case.inputs[0].bytes=b"<r:Rates xmlns:r=\"urn:rates\"><r:Factor>\xff</r:Factor></r:Rates>".to_vec(),
            "labels-invalid-utf8"=>case.inputs[1].bytes=b"<Labels><Prefix>\xff</Prefix></Labels>".to_vec(),
            "swapped-documents"=>{let rates=case.inputs[0].bytes.clone();case.inputs[0].bytes=case.inputs[1].bytes.clone();case.inputs[1].bytes=rates;},
            "count-over"=>{case.primary=b"<Input>".to_vec();case.inputs=vec![Input {name:"unknown",schema:"rates",bytes:Vec::new()};4096];case.diagnostics=false;},
            "need-context"=>case.replace("primary","false","true"),
            "nonintegral-factor"=>case.replace("rates","<r:Factor>4</r:Factor>","<r:Factor>4.5</r:Factor>"),
            "competing-failures"=>{case.replace("primary","2.5","2.625");case.replace("primary","false","true");},
            "nominal"|"missing-spare"=>{},
            other=>panic!("unknown case {other}"),
        }
        if variant=="zero" {case.inputs.clear();}
        if variant=="unreachable" && name!="missing-spare" {case.inputs.push(Input {name:"spare",schema:"spare",bytes:b"<Spare/>".to_vec()});}
        case
    }).collect()
}
pub(super) fn mapping(variant: &str) -> TestResult<Project> {
    let mut value: Json =
        serde_json::from_str(include_str!("../fixtures/xml_input_sets/project.json"))?;
    match variant {
        "base"=>{},
        "single"=>value["extra_targets"]=json!([]),
        "reversed-declarations"=>value["extra_sources"].as_array_mut().expect("declarations").reverse(),
        "zero"=>{
            value["extra_sources"]=json!([]);value["extra_targets"]=json!([]);
            value["target"]["kind"]["children"]=json!([value["target"]["kind"]["children"][0].clone()]);
            value["root"]["bindings"]=json!([{"target_field":"Product","node":1}]);value["root"]["children"]=json!([]);
            value["graph"]["nodes"]=json!({"1":{"kind":"source_field","path":["Value"]}});
        },
        "unreachable"=>value["extra_sources"].as_array_mut().expect("sources").push(json!({"name":"spare","path":"missing/spare.xml","schema":{"name":"Spare","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[]}},"options":{"xml_document":true}})),
        "context"|"phase-order"=>{
            value["extra_targets"][0]["schema"]["kind"]["children"].as_array_mut().expect("Audit fields").push(json!({"name":"RunTime","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"string"}}));
            value["extra_targets"][0]["root"]["bindings"].as_array_mut().expect("Audit bindings").push(json!({"target_field":"RunTime","node":16}));
            if variant=="phase-order" {value["target"]["kind"]["children"][0]["kind"]["ty"]=json!("int");}
        },
        "named-output"=>{
            value["extra_targets"][0]["schema"]["kind"]["children"].as_array_mut().expect("Audit fields").push(json!({"name":"FactorInt","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}));
            value["extra_targets"][0]["root"]["bindings"].as_array_mut().expect("Audit bindings").push(json!({"target_field":"FactorInt","node":2}));
        },
        "policies"|"reverse-policies"=>{
            value["target_options"]["xml_schema_hints"]=json!({"no_namespace_location":"result.xsd"});
            let audit=&mut value["extra_targets"][0];audit["schema"]["xml_namespace"]=json!({"kind":"qualified","uri":"urn:audit"});
            for field in audit["schema"]["kind"]["children"].as_array_mut().expect("Audit fields") {field["xml_namespace"]=json!({"kind":"qualified","uri":"urn:audit"});}
            audit["options"]["xml_schema_hints"]=json!({"locations":[{"namespace":"urn:audit","location":"audit.xsd"}]});
            value["graph"]["nodes"]["18"]=json!({"kind":"const","value":"receipt"});
            value["extra_targets"].as_array_mut().expect("targets").push(json!({"name":"receipt","path":"receipt.xml","schema":{"name":"Receipt","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Label","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"string"}},{"name":"Count","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}]}},"options":{"xml_document":true,"xml_schema_hints":{"no_namespace_location":"receipt.xsd"}},"root":{"target_field":"Receipt","bindings":[{"target_field":"Label","node":18},{"target_field":"Count","node":8}],"children":[]}}));
            if variant=="reverse-policies" {value["extra_targets"].as_array_mut().expect("targets").reverse();}
        },
        other=>panic!("unknown variant {other}"),
    }
    Ok(serde_json::from_value(value)?)
}
fn owner(name: Option<&str>, project: &Project) -> Json {
    match name {
        None => Json::Null,
        Some("primary") => json!({"kind":"Primary","index":null,"name":null}),
        Some(name) => {
            json!({"kind":"Named","index":project.extra_sources.iter().position(|source|source.name==name).expect("declaration owner"),"name":name})
        }
    }
}
pub(super) fn expected(
    variant: &str,
    case: &str,
    api: &str,
    project: &Project,
) -> (&'static str, Json, Json, Option<Json>) {
    if !api.contains("sources") && !project.extra_sources.is_empty() {
        return (
            "Mapping",
            Json::Null,
            Json::Null,
            Some(json!({"kind":"MissingNamedSource","name":project.extra_sources[0].name})),
        );
    }
    let input = match case {
        "primary-bad-number" | "primary-malformed" | "primary-invalid-utf8" => Some("primary"),
        "rates-bad-number"
        | "rates-wrong-namespace"
        | "rates-invalid-utf8"
        | "swapped-documents" => Some("rates"),
        "labels-malformed" | "labels-invalid-utf8" => Some("labels"),
        "two-malformed-forward" | "two-malformed-reverse" => {
            Some(if variant == "reversed-declarations" {
                "labels"
            } else {
                "rates"
            })
        }
        _ => None,
    };
    if input.is_some() {
        return (
            if case.ends_with("invalid-utf8") {
                "Utf8"
            } else {
                "Input"
            },
            owner(input, project),
            Json::Null,
            None,
        );
    }
    let names = match case {
        "missing-rates" => Some(("MissingNamedSource", "rates")),
        "missing-labels" => Some(("MissingNamedSource", "labels")),
        "missing-spare" => Some(("MissingNamedSource", "spare")),
        "duplicate-rates" => Some(("DuplicateNamedSource", "rates")),
        "unexpected-name" => Some(("UnexpectedNamedSource", "unknown")),
        "wrong-case" => Some(("UnexpectedNamedSource", "Rates")),
        "whitespace-name" => Some(("UnexpectedNamedSource", " rates")),
        _ => None,
    };
    if let Some((kind, name)) = names {
        return (
            "Mapping",
            Json::Null,
            Json::Null,
            Some(json!({"kind":kind,"name":name})),
        );
    }
    if case == "count-over" {
        return ("Input", Json::Null, Json::Null, None);
    }
    if case == "missing-factor"
        || (matches!(case, "need-context" | "competing-failures") && !api.ends_with("context"))
    {
        return ("Mapping", Json::Null, Json::Null, None);
    }
    if variant == "named-output" {
        return (
            "Output",
            Json::Null,
            json!({"kind":"Named","index":0,"name":"audit"}),
            None,
        );
    }
    if variant == "phase-order" {
        return (
            "Output",
            Json::Null,
            json!({"kind":"Primary","index":null,"name":null}),
            None,
        );
    }
    ("ok", Json::Null, Json::Null, None)
}
fn attribute(name: &str, namespace: &str, value: &str) -> Json {
    json!({"name":name,"namespace":namespace,"value":value})
}
fn element(
    name: &str,
    namespace: &str,
    text: Option<&str>,
    attributes: Vec<Json>,
    children: Vec<Json>,
) -> Json {
    json!({"name":name,"namespace":namespace,"text":text,"attributes":attributes,"children":children})
}
pub(super) fn physical(variant: &str, case: &str, name: Option<&str>) -> Json {
    let policy = variant.ends_with("policies");
    let xsi = "http://www.w3.org/2001/XMLSchema-instance";
    let count = if case == "empty-rates" { "0" } else { "2" };
    match name {
        None => {
            let attrs = if policy {
                vec![attribute("noNamespaceSchemaLocation", xsi, "result.xsd")]
            } else {
                vec![]
            };
            let mut children = vec![element(
                "Product",
                "",
                Some(if variant == "zero" { "2.5" } else { "10" }),
                vec![],
                vec![],
            )];
            if variant != "zero" {
                children.push(element(
                    "Message",
                    "",
                    Some(if case == "unicode" {
                        "Ω😀&:done"
                    } else if case == "empty-labels" {
                        ":done"
                    } else {
                        "Ω:done"
                    }),
                    vec![],
                    vec![],
                ));
                children.push(element("RateCount", "", Some(count), vec![], vec![]));
                if case != "empty-rates" {
                    for (index, (code, amount, adjustment)) in [
                        (if case == "unicode" { "A😀&" } else { "A" }, "1.25", "2"),
                        ("B", "7.5", "6.5"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let mut fields = vec![element("Amount", "", Some(amount), vec![], vec![])];
                        if !((case == "missing-first-group" && index == 0)
                            || (case == "missing-second-group" && index == 1))
                        {
                            fields.push(if case == "nil-adjustment" && index == 0 {
                                element(
                                    "Adjustment",
                                    "",
                                    Some(""),
                                    vec![attribute("nil", xsi, "true")],
                                    vec![],
                                )
                            } else {
                                element("Adjustment", "", Some(adjustment), vec![], vec![])
                            });
                        }
                        children.push(element(
                            "Rate",
                            "",
                            None,
                            vec![attribute("Code", "", code)],
                            fields,
                        ));
                    }
                }
            }
            element("Result", "", None, attrs, children)
        }
        Some("audit") => {
            let ns = if policy { "urn:audit" } else { "" };
            let attrs = if policy {
                vec![attribute("schemaLocation", xsi, "urn:audit audit.xsd")]
            } else {
                vec![]
            };
            let mut children = vec![element("RateCount", ns, Some(count), vec![], vec![])];
            if case != "empty-rates" {
                children.push(element(
                    "FirstCode",
                    ns,
                    Some(if case == "unicode" { "A😀&" } else { "A" }),
                    vec![],
                    vec![],
                ));
            }
            children.push(element("Revision", ns, Some("3"), vec![], vec![]));
            if variant == "context" {
                children.push(element(
                    "RunTime",
                    "",
                    Some(if case == "need-context" {
                        CURRENT
                    } else {
                        "unused"
                    }),
                    vec![],
                    vec![],
                ));
            }
            element("Audit", ns, None, attrs, children)
        }
        Some("receipt") => element(
            "Receipt",
            "",
            None,
            vec![attribute("noNamespaceSchemaLocation", xsi, "receipt.xsd")],
            vec![
                element("Label", "", Some("receipt"), vec![], vec![]),
                element("Count", "", Some(count), vec![], vec![]),
            ],
        ),
        other => panic!("unexpected target {other:?}"),
    }
}
pub(super) fn field<'a>(instance: &'a Json, name: &str) -> Option<&'a Json> {
    instance["fields"]
        .as_array()
        .expect("all group fields")
        .iter()
        .find(|field| field["name"] == name)
        .map(|field| &field["instance"])
}
pub(super) fn assert_source_facts(value: &Json, role: &str, case: &str) {
    assert_eq!(value["xml_type_origin"], json!({"kind":"Unknown"}));
    if role == "primary" {
        assert_eq!(
            field(value, "Value"),
            Some(
                &json!({"kind":"Scalar","tag":"Float","bits_hex":if case=="competing-failures" {"4005000000000000"} else {"4004000000000000"}})
            )
        );
        assert_eq!(
            field(value, "NeedContext"),
            Some(
                &json!({"kind":"Scalar","tag":"Bool","value":matches!(case,"need-context"|"competing-failures")})
            )
        );
    }
    if role == "labels" {
        assert_eq!(
            field(value, "Prefix"),
            Some(&if case == "empty-labels" {
                json!({"kind":"Scalar","tag":"Null"})
            } else {
                json!({"kind":"Scalar","tag":"String","value":if case=="unicode" {"Ω😀&"} else {"Ω"}})
            })
        );
    }
    if role == "rates" {
        assert_eq!(
            field(value, "Revision"),
            Some(&json!({"kind":"Scalar","tag":"Int","value":3}))
        );
        if case == "missing-factor" {
            assert_eq!(
                field(value, "Factor"),
                Some(&json!({"kind":"Scalar","tag":"Null"}))
            );
        }
        if !matches!(case, "missing-factor") {
            assert_eq!(
                field(value, "Factor"),
                Some(
                    &json!({"kind":"Scalar","tag":"Float","bits_hex":if case=="nonintegral-factor" {"4012000000000000"} else {"4010000000000000"}})
                )
            );
        }
        let items = field(value, "Item").expect("repeated rates")["items"]
            .as_array()
            .expect("all rates");
        assert_eq!(items.len(), if case == "empty-rates" { 0 } else { 2 });
        for (index, item) in items.iter().enumerate() {
            assert_eq!(
                field(item, "Amount"),
                Some(
                    &json!({"kind":"Scalar","tag":"Float","bits_hex":if index==0 {"3ff4000000000000"} else {"401e000000000000"}})
                )
            );
            if (case == "missing-first-group" && index == 0)
                || (case == "missing-second-group" && index == 1)
            {
                assert!(field(item, "Details").is_none());
            } else {
                let details = field(item, "Details").expect("present details");
                assert_eq!(
                    field(details, "Adjustment"),
                    Some(&if case == "nil-adjustment" && index == 0 {
                        json!({"kind":"Scalar","tag":"XmlNil"})
                    } else {
                        json!({"kind":"Scalar","tag":"Float","bits_hex":if index==0 {"4000000000000000"} else {"401a000000000000"}})
                    })
                );
            }
        }
    }
}
