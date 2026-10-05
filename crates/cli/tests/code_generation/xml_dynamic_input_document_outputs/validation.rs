use super::*;

pub(super) fn retained_bytes(buffer: &Json) -> Vec<u8> {
    let hex = buffer["hex"].as_str().expect("complete original bytes");
    assert_eq!(hex.len() % 2, 0);
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(buffer["size"].as_u64().unwrap(), bytes.len() as u64);
    assert_eq!(buffer["sha256"], sha256::hex(&bytes));
    bytes
}
fn canonical_buffer(value: &Json) -> Json {
    json!({"size":value["size"],"sha256":value["sha256"],"hex":value["hex"]})
}
fn check_all_buffers(value: &Json) {
    match value {
        Json::Object(fields) => {
            if fields.contains_key("hex")
                && fields.contains_key("size")
                && fields.contains_key("sha256")
            {
                let _ = retained_bytes(value);
            }
            for child in fields.values() {
                check_all_buffers(child);
            }
        }
        Json::Array(items) => {
            for child in items {
                check_all_buffers(child);
            }
        }
        _ => {}
    }
}
fn identity(row: &Json) -> (String, String, String, String) {
    (
        row["backend"].as_str().unwrap().into(),
        row["variant"].as_str().unwrap().into(),
        row["case"].as_str().unwrap().into(),
        row["method"].as_str().unwrap().into(),
    )
}
fn original_transport(value: &Json) -> Json {
    match value {
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .filter(|(name, _)| name.as_str() != "diagnostic_parse_outside_product")
                .map(|(name, value)| (name.clone(), original_transport(value)))
                .collect(),
        ),
        Json::Array(items) => json!(items.iter().map(original_transport).collect::<Vec<_>>()),
        scalar => scalar.clone(),
    }
}
pub(super) fn read_channels(original: &[u8], diagnostic: &[u8]) -> TestResult<Vec<Json>> {
    let lines = original
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let mut diagnostics = BTreeMap::new();
    for line in diagnostic
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row: Json = serde_json::from_slice(line)?;
        assert_eq!(row["schema"], 2);
        assert_eq!(row["record"], "diagnostic");
        assert_eq!(row["diagnostic_failure"], Json::Null);
        assert!(
            diagnostics
                .insert(identity(&row["identity"]), row)
                .is_none()
        );
    }
    assert_eq!(diagnostics.len(), lines.len());
    let mut rows = Vec::new();
    let mut keys = BTreeSet::new();
    for line in lines {
        let original: Json = serde_json::from_slice(line)?;
        assert_eq!(original["schema"], 2);
        assert_eq!(original["record"], "original");
        check_all_buffers(&original);
        let key = identity(&original);
        assert!(keys.insert(key.clone()));
        let diagnostic = diagnostics.remove(&key).unwrap();
        assert_eq!(retained_bytes(&diagnostic["original_record_buffer"]), line);
        let observed = &diagnostic["observation"];
        check_all_buffers(observed);
        for field in [
            "backend",
            "variant",
            "case",
            "method",
            "outcome",
            "project_original",
            "project_buffer",
            "context",
            "error",
            "native_original_stream_reference",
            "complete_typed_outputs",
        ] {
            assert_eq!(
                observed[field], original[field],
                "post-flush diagnostic changed original {field}"
            );
        }
        for field in ["inputs", "callbacks"] {
            assert_eq!(
                original_transport(&observed[field]),
                original[field],
                "post-flush diagnostic changed original {field}"
            );
        }
        if original["backend"] != "native" {
            let strip = |value: &Json| -> Json {
                json!({"primary":{"path":value["primary"]["path"],"buffer":canonical_buffer(&value["primary"]["buffer"])},"extras":value["extras"].as_array().unwrap().iter().map(|extra|json!({"name":extra["name"],"original_declaration_index":extra["original_declaration_index"],"documents":extra["documents"].as_array().unwrap().iter().map(|document|json!({"path":document["path"],"buffer":canonical_buffer(&document["buffer"])})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
            };
            if !original["outputs"].is_null() {
                assert_eq!(strip(&observed["outputs"]), strip(&original["outputs"]));
            } else {
                assert_eq!(observed["outputs"], Json::Null);
            }
        }
        assert_eq!(observed["diagnostic_recording_error"], Json::Null);
        assert_eq!(observed["original_value_recording_error"], Json::Null);
        rows.push(observed.clone());
    }
    assert!(diagnostics.is_empty());
    Ok(rows)
}
fn typed(value: &Json) -> Json {
    match value["kind"].as_str().unwrap() {
        "Scalar" => {
            if value["tag"] == "Float" {
                let bits = value["bits_hex"].as_str().unwrap();
                assert_eq!(bits.len(), 16);
                let _ = u64::from_str_radix(bits, 16).unwrap();
                json!({"kind":"Scalar","tag":"Float","bits_hex":bits})
            } else {
                let mut result = json!({"kind":"Scalar","tag":value["tag"]});
                if let Some(scalar) = value.get("value") {
                    result["value"] = scalar.clone();
                }
                result
            }
        }
        "Group" => {
            let origin = value["xml_type_origin"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>();
            json!({"kind":"Group","xml_type_origin":origin,"fields":value["fields"].as_array().unwrap().iter().map(|field|json!({"name":field["name"],"instance":typed(&field["instance"])})).collect::<Vec<_>>()})
        }
        "Repeated" | "MappedSequence" => {
            json!({"kind":value["kind"],"items":value["items"].as_array().unwrap().iter().map(typed).collect::<Vec<_>>()})
        }
        "DocumentSet" => {
            json!({"kind":"DocumentSet","documents":value["documents"].as_array().unwrap().iter().map(|member| {
            assert_eq!(member["resolved_source_path"], Json::Null);
            json!({"path":member["path"],"instance":typed(&member["instance"])})
        }).collect::<Vec<_>>()})
        }
        other => panic!("unknown complete snapshot kind {other}"),
    }
}
fn parsed(record: &Json) -> Json {
    assert_eq!(record["outcome"], "parsed");
    typed(&record["snapshot"])
}
fn physical(xml: &[u8], schema: &Json) -> Json {
    fn element(node: roxmltree::Node<'_, '_>, schema: &Json) -> Json {
        assert_eq!(node.tag_name().name(), schema["name"].as_str().unwrap());
        let namespace = match schema["xml_namespace"]["kind"].as_str().unwrap() {
            "qualified" => schema["xml_namespace"]["uri"].as_str().unwrap(),
            "unqualified" => "",
            other => panic!("unsupported authored namespace {other}"),
        };
        assert_eq!(node.tag_name().namespace().unwrap_or(""), namespace);
        let mut attributes = node.attributes().map(|attribute| json!({"name":attribute.name(),"namespace":attribute.namespace().unwrap_or(""),"value":attribute.value()})).collect::<Vec<_>>();
        attributes.sort_by_key(|attribute| {
            (
                attribute["namespace"].as_str().unwrap().to_owned(),
                attribute["name"].as_str().unwrap().to_owned(),
            )
        });
        let raw_text = node
            .children()
            .filter(|child| child.is_text())
            .filter_map(|child| child.text())
            .collect::<String>();
        let group = schema["kind"]["kind"] == "group";
        let text = if group {
            assert!(
                raw_text
                    .chars()
                    .all(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
            );
            "".into()
        } else {
            raw_text
        };
        let children = node
            .children()
            .filter(|child| child.is_element())
            .map(|child| {
                let fields = schema["kind"]["children"].as_array().unwrap();
                let field = fields
                    .iter()
                    .find(|field| {
                        field["attribute"] != true && field["name"] == child.tag_name().name()
                    })
                    .unwrap();
                element(child, field)
            })
            .collect::<Vec<_>>();
        json!({"name":node.tag_name().name(),"namespace":namespace,"attributes":attributes,"raw_text":text,"children":children})
    }
    element(
        roxmltree::Document::parse(std::str::from_utf8(xml).unwrap())
            .unwrap()
            .root_element(),
        schema,
    )
}
fn output_document(record: &Json, schema: &Json) -> Json {
    let bytes = retained_bytes(&record["buffer"]);
    assert_eq!(record["physical"]["outcome"], "parsed");
    json!({"typed":parsed(&record["typed"]),"physical":physical(&bytes, schema)})
}
fn outputs(row: &Json) -> Json {
    let project = &row["project_original"];
    let source = &row["outputs"];
    let declarations = project["extra_targets"].as_array().unwrap();
    assert_eq!(
        source["extras"].as_array().unwrap().len(),
        declarations.len()
    );
    let extras = source["extras"].as_array().unwrap().iter().zip(declarations).enumerate().map(|(index, (extra, declaration))| {
        assert_eq!(extra["original_declaration_index"], index); assert_eq!(extra["name"], declaration["name"]);
        json!({"name":extra["name"],"documents":extra["documents"].as_array().unwrap().iter().map(|document|json!({"path":document["path"],"value":output_document(document, &declaration["schema"])})).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();
    let result =
        json!({"primary":output_document(&source["primary"], &project["target"]),"extras":extras});
    if row["backend"] == "native" {
        assert_eq!(
            typed(&row["complete_typed_outputs"]["primary"]),
            result["primary"]["typed"]
        );
        assert_eq!(
            row["complete_typed_outputs"]["extras"]
                .as_array()
                .unwrap()
                .len(),
            declarations.len()
        );
        for (mapped, extra) in row["complete_typed_outputs"]["extras"]
            .as_array()
            .unwrap()
            .iter()
            .zip(result["extras"].as_array().unwrap())
        {
            assert_eq!(mapped["name"], extra["name"]);
            assert_eq!(mapped["instance"]["kind"], "DocumentSet");
            let members = mapped["instance"]["documents"].as_array().unwrap();
            assert_eq!(members.len(), extra["documents"].as_array().unwrap().len());
            for (mapped, serialized) in members.iter().zip(extra["documents"].as_array().unwrap()) {
                assert_eq!(mapped["path"], serialized["path"]);
                assert_eq!(typed(&mapped["instance"]), serialized["value"]["typed"]);
            }
        }
    }
    result
}
fn field<'a>(group: &'a Json, name: &str) -> &'a Json {
    assert_eq!(group["kind"], "Group");
    &group["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == name)
        .unwrap()["instance"]
}
fn hints(tree: &Json, options: &Json, mut attributes: Vec<Json>) {
    if let Some(locations) = options["xml_schema_hints"]["locations"].as_array() {
        attributes.push(json!({"name":"schemaLocation","namespace":"http://www.w3.org/2001/XMLSchema-instance","value":locations.iter().flat_map(|item|[item["namespace"].as_str().unwrap(),item["location"].as_str().unwrap()]).collect::<Vec<_>>().join(" ")}));
    }
    if let Some(location) = options["xml_schema_hints"]["no_namespace_location"].as_str() {
        attributes.push(json!({"name":"noNamespaceSchemaLocation","namespace":"http://www.w3.org/2001/XMLSchema-instance","value":location}));
    }
    attributes.sort_by_key(|attribute| {
        (
            attribute["namespace"].as_str().unwrap().to_owned(),
            attribute["name"].as_str().unwrap().to_owned(),
        )
    });
    assert_eq!(tree["attributes"], json!(attributes));
}
fn child_text<'a, 'input: 'a>(node: roxmltree::Node<'a, 'input>, name: &str) -> &'a str {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .unwrap()
        .text()
        .unwrap_or("")
}
fn authored_success(row: &Json, case: &Json, root: &Path, value: &Json) -> TestResult<()> {
    let text = std::fs::read_to_string(root.join(case["primary"].as_str().unwrap()))?;
    let primary = roxmltree::Document::parse(&text)?;
    let output_rows = primary
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "OutputRow")
        .collect::<Vec<_>>();
    let mut loaded = Vec::new();
    for node in primary
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "LoadRow")
    {
        let path = child_text(node, "InputFile");
        let document = case["loader_documents"]
            .get(path)
            .and_then(Json::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("documents/{path}"));
        let text = std::fs::read_to_string(root.join(document))?;
        let catalog = roxmltree::Document::parse(&text)?;
        for entry in catalog
            .root_element()
            .children()
            .filter(|node| node.is_element())
        {
            loaded.push((
                child_text(entry, "Code").parse::<i64>()?,
                child_text(entry, "Amount").parse::<f64>()?,
                child_text(entry, "Text").to_owned(),
            ));
        }
    }
    let rates = case["statics_transport"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["name"] == "rates")
        .unwrap();
    let text = std::fs::read_to_string(root.join(rates["document"].as_str().unwrap()))?;
    let factor =
        child_text(roxmltree::Document::parse(&text)?.root_element(), "Factor").parse::<f64>()?;
    let project = &row["project_original"];
    hints(
        &value["primary"]["physical"],
        &project["target_options"],
        Vec::new(),
    );
    let expected_marker = if case["id"] == "context-selected-supplied" {
        "2026-10-04T00:00:00Z  label-雪😀  maps/library.jsonmaps/main.json"
    } else {
        "complete"
    };
    assert_eq!(
        field(&value["primary"]["typed"], "Marker"),
        &json!({"kind":"Scalar","tag":"String","value":expected_marker})
    );
    for (extra, target) in value["extras"]
        .as_array()
        .unwrap()
        .iter()
        .zip(project["extra_targets"].as_array().unwrap())
    {
        let members = extra["documents"].as_array().unwrap();
        assert_eq!(members.len(), output_rows.len());
        for (member, driver) in members.iter().zip(&output_rows) {
            let alpha = extra["name"] == "alpha";
            assert_eq!(
                member["path"],
                child_text(*driver, if alpha { "OutputFile" } else { "BetaFile" })
            );
            let id = child_text(*driver, "Id").parse::<i64>()?;
            let group = &member["value"]["typed"];
            assert_eq!(
                field(group, "Id"),
                &json!({"kind":"Scalar","tag":"Int","value":id})
            );
            let lines = field(group, "Line");
            assert_eq!(lines["kind"], "Repeated");
            assert_eq!(lines["items"].as_array().unwrap().len(), loaded.len());
            for (line, (code, amount, text)) in
                lines["items"].as_array().unwrap().iter().zip(&loaded)
            {
                assert_eq!(
                    field(line, "Text"),
                    &json!({"kind":"Scalar","tag":"String","value":text})
                );
                if alpha {
                    assert_eq!(
                        field(line, "Amount"),
                        &json!({"kind":"Scalar","tag":"Float","bits_hex":format!("{:016x}",amount.to_bits())})
                    );
                    assert_eq!(
                        field(line, "Scaled"),
                        &json!({"kind":"Scalar","tag":"Float","bits_hex":format!("{:016x}",(amount*factor).to_bits())})
                    );
                } else {
                    assert_eq!(
                        field(line, "Code"),
                        &json!({"kind":"Scalar","tag":"Int","value":code})
                    );
                }
            }
            hints(
                &member["value"]["physical"],
                &target["options"],
                vec![json!({"name":"Id","namespace":"","value":id.to_string()})],
            );
            if case["id"] == "numeric-edge-normal" {
                let first = &lines["items"][0];
                if alpha {
                    for name in ["Amount", "Scaled"] {
                        assert_eq!(field(first, name)["bits_hex"], "8000000000000000");
                    }
                } else {
                    assert_eq!(field(first, "Code")["tag"], "Int");
                    assert_eq!(
                        field(first, "Code")["value"].as_i64(),
                        Some(9_007_199_254_740_993)
                    );
                }
            }
        }
    }
    Ok(())
}
fn boundary(value: &Json) -> Json {
    let causes = value["causes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cause| {
            let mut result = cause
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| {
                    !["debug", "reference_id", "full_payload", "inner"].contains(&key.as_str())
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>();
            if let Some(payload) = result.get_mut("payload").and_then(Json::as_object_mut) {
                payload.remove("debug");
            }
            if let Some(properties) = cause["full_payload"].as_object() {
                result.insert(
                    "domain_properties".into(),
                    json!(
                        properties
                            .iter()
                            .filter(|(key, _)| ![
                                "StackTrace",
                                "TargetSite",
                                "Data",
                                "HResult",
                                "Message",
                                "Source",
                                "HelpLink"
                            ]
                            .contains(&key.as_str()))
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect::<serde_json::Map<_, _>>()
                    ),
                );
            }
            if cause["type"] == "System.Text.DecoderFallbackException"
                && let Some(bytes) = result
                    .get_mut("domain_properties")
                    .and_then(Json::as_object_mut)
                    .and_then(|properties| properties.get_mut("BytesUnknown"))
                    .and_then(Json::as_object_mut)
            {
                bytes.remove("reference_id");
            }
            Json::Object(result)
        })
        .collect::<Vec<_>>();
    json!({"kind":value["kind"],"detail":value["detail"],"bytes":value["bytes"],"limit":value["limit"],"causes":causes})
}
fn runtime(error: &Json, language: &str) -> Json {
    let fields = if language == "csharp" {
        &error["boundary"]["runtime"]
    } else {
        &error["boundary"]["causes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|cause| cause["type"] == "codegen_runtime::RuntimeError")
            .unwrap()["payload"]["fields"]
    };
    common_runtime(fields)
}
fn common_runtime(fields: &Json) -> Json {
    let mut result = json!({"variant":fields["variant"]});
    let names: &[&str] = match fields["variant"].as_str().unwrap() {
        "MissingNamedSource" => &["name"],
        "DynamicSourceLoad" => &["source", "path", "message"],
        "EmptyDynamicTargetPath" => &["node"],
        "MissingRuntimeParameter" => &["node", "name"],
        other => panic!("unexpected original runtime {other}"),
    };
    for name in names {
        result[*name] = if *name == "name"
            && fields["variant"] == "MissingNamedSource"
            && fields["name"].is_null()
        {
            fields["detail"].clone()
        } else {
            fields[*name].clone()
        };
    }
    result
}
pub(super) fn callbacks(row: &Json, native: bool) -> Json {
    json!(
        row["callbacks"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, call)| {
                assert_eq!(call["attempt_index"], index);
                let mut value = json!({"source":call["source"],"path":call["path"]});
                if !call["buffer"].is_null() {
                    value["buffer"] = json!(retained_bytes(&call["buffer"]));
                    if native {
                        value["typed"] = typed(&call["typed"]);
                    } else if call["diagnostic_parse_outside_product"]["outcome"] == "parsed" {
                        value["typed"] = parsed(&call["diagnostic_parse_outside_product"]);
                    } else {
                        value["parse_error"] =
                            call["diagnostic_parse_outside_product"]["boundary"]["kind"].clone();
                    }
                }
                if !call["host_error"].is_null() {
                    value["host_error"] = if call["host_error"].is_object() {
                        call["host_error"]["detail"].clone()
                    } else {
                        call["host_error"].clone()
                    };
                }
                value
            })
            .collect::<Vec<_>>()
    )
}
pub(super) fn check_row(
    row: &Json,
    case: &Json,
    root: &Path,
    oracle: Option<&Json>,
    native_reference: &Json,
    first_native_callback: &Json,
    language: &str,
) -> TestResult<()> {
    assert_eq!(row["backend"], language);
    assert_eq!(row["case"], case["id"]);
    assert_eq!(
        row["project_original"],
        serde_json::from_slice::<Json>(&std::fs::read(
            root.join(case["project"].as_str().unwrap())
        )?)?
    );
    assert_eq!(
        retained_bytes(&row["project_buffer"]),
        std::fs::read(root.join(case["project"].as_str().unwrap()))?
    );
    assert_eq!(
        retained_bytes(&row["inputs"]["primary"]["buffer"]),
        std::fs::read(root.join(case["primary"].as_str().unwrap()))?
    );
    assert_eq!(&row["native_original_stream_reference"], native_reference);
    let supplied = row["inputs"]["statics_transport"].as_array().unwrap();
    let transport = case["statics_transport"].as_array().unwrap();
    assert_eq!(supplied.len(), transport.len());
    for (record, input) in supplied.iter().zip(transport) {
        assert_eq!(record["name"], input["name"]);
        assert_eq!(
            retained_bytes(&record["buffer"]),
            std::fs::read(root.join(input["document"].as_str().unwrap()))?
        );
        let declaration = row["project_original"]["extra_sources"]
            .as_array()
            .unwrap()
            .iter()
            .position(|source| source["name"] == input["name"])
            .unwrap();
        assert_eq!(record["original_declaration_index"], declaration);
    }
    for (index, callback) in row["callbacks"].as_array().unwrap().iter().enumerate() {
        assert_eq!(callback["attempt_index"], index);
        assert_eq!(callback["source"], "catalog");
        if !callback["buffer"].is_null() {
            let path = callback["path"].as_str().unwrap();
            let document = if index == 1 && case["loader_mode"] == "invalid-utf8-second" {
                "documents/invalid-utf8.bin".to_owned()
            } else {
                case["loader_documents"]
                    .get(path)
                    .and_then(Json::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("documents/{path}"))
            };
            assert_eq!(
                retained_bytes(&callback["buffer"]),
                std::fs::read(root.join(document))?
            );
        }
    }
    let context = &row["context"];
    let supplied_context = row["method"].as_str().unwrap().ends_with("context");
    assert_eq!(context["supplied"], supplied_context);
    assert_eq!(context["context_mode"], case["context_mode"]);
    assert_eq!(
        context["context_nodes_source_authored"],
        json!([11, 12, 15, 16])
    );
    for (name, value) in [
        ("mapping_file_path", json!("maps/library.json")),
        ("main_mapping_file_path", json!("maps/main.json")),
    ] {
        assert_eq!(
            context[name],
            if supplied_context { value } else { Json::Null }
        );
    }
    assert_eq!(
        context["current_datetime"],
        if supplied_context && case["context_mode"] != "empty" {
            json!("2026-10-04T00:00:00Z")
        } else {
            Json::Null
        }
    );
    assert_eq!(
        context["runtime_parameter"],
        if supplied_context && case["context_mode"] == "full" {
            json!({"name":"label","tag":"String","value":"  label-雪😀  "})
        } else {
            Json::Null
        }
    );
    if let Some(native) = oracle {
        assert_eq!(
            parsed(&row["inputs"]["primary"]["diagnostic_parse_outside_product"]),
            typed(&native["inputs"]["primary"]["typed"])
        );
        for source in supplied {
            let original = native["inputs"]["statics_declaration_order"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["name"] == source["name"])
                .unwrap();
            assert_eq!(
                parsed(&source["diagnostic_parse_outside_product"]),
                typed(&original["typed"])
            );
        }
    }
    let expected_runtime = match case["id"].as_str().unwrap() {
        "missing-unused-before-malformed-primary" => {
            Some(json!({"variant":"MissingNamedSource","name":"unused"}))
        }
        "external-host-marker-text" => Some(
            json!({"variant":"DynamicSourceLoad","source":"catalog","path":"b.xml","message":"generated XML input adapter refused a dynamic document"}),
        ),
        "late-beta-mapping-over-primary-alpha-writer" => {
            Some(json!({"variant":"EmptyDynamicTargetPath","node":9}))
        }
        "context-selected-missing-label" => {
            Some(json!({"variant":"MissingRuntimeParameter","node":12,"name":"label"}))
        }
        _ => None,
    };
    let writer = case["id"]
        .as_str()
        .unwrap()
        .starts_with("beta-final-member1-writer");
    let invalid_utf8 = case["id"] == "ordinal2-loader-invalid-utf8";
    if expected_runtime.is_some() || writer || invalid_utf8 {
        assert_eq!(row["outcome"], "error");
        assert_eq!(row["outputs"], Json::Null);
        let error = &row["error"];
        if language == "rust" {
            assert_eq!(error["source_is_boundary"], true);
        } else {
            assert_eq!(error["identity"]["inner_is_boundary"], true);
            assert_eq!(
                error["identity"]["boundary_reference_id"],
                error["boundary"]["original"]["reference_id"]
            );
            if !error["request"].is_null() {
                assert_eq!(
                    error["identity"]["request_reference_id"],
                    error["request"]["reference_id"]
                );
            }
        }
        if let Some(expected) = expected_runtime {
            assert_eq!(error["owner"], Json::Null);
            assert_eq!(error["request"], Json::Null);
            assert_eq!(error["boundary"]["kind"], "Mapping");
            assert_eq!(runtime(error, language), expected);
            if let Some(native) = oracle {
                assert_eq!(native["outcome"], "error");
                assert_eq!(common_runtime(&native["error"]["fields"]), expected);
                assert_eq!(callbacks(row, false), callbacks(native, true));
            } else {
                assert_eq!(row["callbacks"], json!([]));
            }
            if case["id"] == "external-host-marker-text" && language == "csharp" {
                let host_id = &row["original_host_exception"]["reference_id"];
                assert!(!host_id.is_null());
                assert!(
                    error["boundary"]["causes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|cause| &cause["reference_id"] == host_id)
                );
            }
            if case["id"] == "context-selected-missing-label" {
                assert_eq!(row["callbacks"], json!([]));
            }
        } else if writer {
            let declaration = row["project_original"]["extra_targets"]
                .as_array()
                .unwrap()
                .iter()
                .position(|target| target["name"] == "beta")
                .unwrap();
            assert_eq!(
                error["owner"],
                json!({"Output":{"NamedMember":{"declaration_index":declaration,"name":"beta","index":1,"path":"../opaque/二.xml"}}})
            );
            assert_eq!(error["request"], Json::Null);
            assert_eq!(error["boundary"]["kind"], "Output");
            let native = oracle.unwrap();
            assert_eq!(native["outcome"], "returned_complete_outputs");
            assert_eq!(callbacks(row, false), callbacks(native, true));
            let primitive = &row["original_same_language_serializer"];
            assert_eq!(primitive["outcome"], "error");
            assert_eq!(
                boundary(&error["boundary"]),
                boundary(&primitive["boundary"])
            );
            let bad = &native["outputs"]["extras"][declaration]["documents"][1];
            assert_eq!(bad["serialization_outcome"], "error");
            assert_eq!(
                bad["serialization_error"]["fields"]["variant"],
                "InvalidXmlCharacter"
            );
            assert_eq!(bad["serialization_error"]["fields"]["codepoint"], 1);
        } else {
            assert_eq!(
                error["owner"],
                json!({"Input":{"Named":{"index":1,"name":"catalog"}}})
            );
            let mut request = error["request"].clone();
            request.as_object_mut().unwrap().remove("reference_id");
            assert_eq!(
                request,
                json!({"declaration_index":1,"source":"catalog","path":"b.xml","ordinal":2,"callback_invoked":true})
            );
            assert_eq!(row["callbacks"].as_array().unwrap().len(), 2);
            assert_eq!(callbacks(row, false)[0], *first_native_callback);
            assert_eq!(row["callbacks"][1]["source"], "catalog");
            assert_eq!(row["callbacks"][1]["path"], "b.xml");
            assert_eq!(retained_bytes(&row["callbacks"][1]["buffer"]), [255]);
            let primitive = &row["callbacks"][1]["diagnostic_parse_outside_product"];
            assert_eq!(primitive["outcome"], "error");
            assert_eq!(primitive["boundary"]["kind"], "Utf8");
            assert_eq!(
                boundary(&error["boundary"]),
                boundary(&primitive["boundary"])
            );
        }
    } else {
        assert_eq!(row["outcome"], "returned_complete_outputs");
        assert_eq!(row["error"], Json::Null);
        let native = oracle.unwrap();
        assert_eq!(native["outcome"], "returned_complete_outputs");
        assert_eq!(callbacks(row, false), callbacks(native, true));
        let value = outputs(row);
        assert_eq!(value, outputs(native));
        authored_success(row, case, root, &value)?;
        if case["id"] == "empty-output-envelopes" {
            assert_eq!(row["callbacks"], json!([]));
        }
        if language == "csharp" {
            let ownership = &row["returned_ownership"];
            assert_eq!(ownership["extras_readonly"], true);
            assert!(
                ownership["documents_readonly"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|value| value == true)
            );
            if row["method"].as_str().unwrap().starts_with("bytes") {
                assert_eq!(ownership["all_buffers_distinct"], true);
                assert_eq!(
                    ownership["actual_buffer_count"],
                    ownership["expected_buffer_count"]
                );
                assert_eq!(
                    ownership["actual_buffer_count"].as_u64().unwrap(),
                    1 + row["outputs"]["extras"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|extra| extra["documents"].as_array().unwrap().len() as u64)
                        .sum::<u64>()
                );
            }
        }
    }
    Ok(())
}
