// Shared source-emission checks; actual public host execution is covered by CLI regressions.

fn xml_input_program() -> codegen::Program {
    let project: mapping::Project = serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap();
    codegen::lower(&project).unwrap()
}

#[test]
fn structured_xml_emits_all_additive_sources_methods_and_original_delegates() {
    let source = emit_xml_input_source(&xml_input_program());
    let methods: &[&str] = if XML_INPUT_LANGUAGE == "rust" {
        &[
            "execute_xml_outputs_with_sources(", "execute_xml_outputs_with_sources_and_context(",
            "execute_xml_bytes_outputs_with_sources(", "execute_xml_bytes_outputs_with_sources_and_context(",
            "execute_xml_with_sources(", "execute_xml_with_sources_and_context(",
            "execute_xml_bytes_with_sources(", "execute_xml_bytes_with_sources_and_context(",
        ]
    } else {
        &["ExecuteXmlOutputsWithSources(", "ExecuteXmlBytesOutputsWithSources(", "ExecuteXmlWithSources(", "ExecuteXmlBytesWithSources("]
    };
    for method in methods { assert!(source.contains(method), "missing {method}"); }
    if XML_INPUT_LANGUAGE == "rust" {
        assert!(source.contains("EXTRA_XML_INPUT_SCHEMA_0"));
        assert!(source.contains("EXTRA_XML_INPUT_SCHEMA_1"));
        assert!(source.find("preflight_xml_input_sizes(&sizes)").unwrap() < source.find("parse_structured_xml(SOURCE_XML_SCHEMA, source)").unwrap());
        assert!(source.contains("XmlInputSource::Named { index: 1, name: \"labels\" }"));
        assert!(source.contains("map_err(codegen_runtime::XmlExecutionError::into_output_set)"));
    } else {
        for method in methods {
            assert_eq!(source.matches(&format!("public static {}", if method.contains("BytesOutputs") { "XmlBytesExecutionOutputs ExecuteXmlBytesOutputsWithSources(" } else if method.contains("Outputs") { "XmlExecutionOutputs ExecuteXmlOutputsWithSources(" } else if method.contains("Bytes") { "byte[] ExecuteXmlBytesWithSources(" } else { "string ExecuteXmlWithSources(" })).count(), 2);
        }
        assert!(source.contains("ExtraXmlInputSchema_0"));
        assert!(source.contains("ExtraXmlInputSchema_1"));
        let names = source.find("FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames)").unwrap();
        let shape = source.find("ThrowIfNull(input.Document)").unwrap();
        let size = source.find("RequireDocumentSize(primaryOwner, primaryBytes)").unwrap();
        assert!(names < shape && shape < size);
        assert!(source.find("RequireDocumentSize(owner_1, bytes_1)").unwrap() < source.find("budget.Charge(primaryOwner, primaryBytes)").unwrap());
        assert!(source.contains("FerruleXmlInputSource.Named(1, \"labels\")"));
        assert!(source.contains("throw error.ToOutputSet()"));
    }
}

#[test]
fn observed_root_view_keeps_original_parser_and_has_no_with_sources_api() {
    let project: mapping::Project = serde_json::from_str(include_str!("fixtures/static_named_xml_rootview.json")).unwrap();
    let program = codegen::lower(&project).unwrap();
    assert_eq!(program.xml_boundary.as_ref().unwrap().input.profile(), Some(codegen::XmlInputProfile::RootView));
    let source = emit_xml_input_source(&program);
    assert!(!source.contains("NamedXmlInput"));
    assert!(!source.contains("execute_xml_outputs_with_sources("));
    assert!(!source.contains("ExecuteXmlOutputsWithSources("));
    assert!(if XML_INPUT_LANGUAGE == "rust" {
        source.contains("parse_xml(SOURCE_XML_SCHEMA, source, true, true)")
    } else {
        source.contains("ParseEmbedded(SourceXmlSchema, source, true, true)")
    });
}

#[test]
fn unproved_ordinary_named_source_keeps_typed_json_core_and_omits_whole_xml_adapter() {
    let mut project: mapping::Project = serde_json::from_str(include_str!("fixtures/static_named_xml_inputs.json")).unwrap();
    project.extra_sources[1].options.xml_document = false;
    let program = codegen::lower(&project).unwrap();
    assert!(program.xml_boundary.is_none());
    assert_eq!(program.extra_sources.len(), 2);
    let source = emit_xml_input_source(&program);
    assert!(if XML_INPUT_LANGUAGE == "rust" { source.contains("execute_json_outputs_with_sources(") } else { source.contains("ExecuteJsonOutputsWithSources(") });
    assert!(!source.contains("NamedXmlInput"));
}


#[test]
fn multiple_dynamic_xml_adapters_keep_all_policies_and_existing_eight_signatures() {
    let project: mapping::Project = serde_json::from_str(include_str!(
        "fixtures/multiple_dynamic_named_xml_input_mixed.json"
    )).unwrap();
    let source = emit_xml_input_source(&codegen::lower(&project).unwrap());
    let methods: &[&str] = if XML_INPUT_LANGUAGE == "rust" {
        &[
            "execute_xml_outputs_with_sources_and_dynamic_source_loader(",
            "execute_xml_outputs_with_sources_context_and_dynamic_source_loader(",
            "execute_xml_with_sources_and_dynamic_source_loader(",
            "execute_xml_with_sources_context_and_dynamic_source_loader(",
            "execute_xml_bytes_outputs_with_sources_and_dynamic_source_loader(",
            "execute_xml_bytes_outputs_with_sources_context_and_dynamic_source_loader(",
            "execute_xml_bytes_with_sources_and_dynamic_source_loader(",
            "execute_xml_bytes_with_sources_context_and_dynamic_source_loader(",
        ]
    } else {
        &[
            "ExecuteXmlOutputsWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlOutputsWithSourcesContextAndDynamicSourceLoader(",
            "ExecuteXmlWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlWithSourcesContextAndDynamicSourceLoader(",
            "ExecuteXmlBytesOutputsWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlBytesOutputsWithSourcesContextAndDynamicSourceLoader(",
            "ExecuteXmlBytesWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlBytesWithSourcesContextAndDynamicSourceLoader(",
        ]
    };
    for method in methods { assert!(source.contains(method), "missing unchanged {method}"); }
    if XML_INPUT_LANGUAGE == "rust" {
        assert_eq!(source.matches("XmlDynamicSourceAdapter::for_sources(loader, vec![").count(), 4);
        assert!(!source.contains("XmlDynamicSourceAdapter::new(loader,"));
        for (index, name) in [(1, "alpha"), (3, "beta")] {
            assert_eq!(source.matches(&format!("XmlDynamicSourcePolicy {{ declaration_index: {index}, source: \"{name}\", schema: EXTRA_XML_INPUT_SCHEMA_{index} }}")).count(), 4);
        }
        assert!(source.contains("XmlInputSource::Named { index: 2, name: \"labels\" }"));
        assert!(source.contains("inputs[indices[1]].document"));
    } else {
        assert_eq!(source.matches("FerruleXmlDynamicSourceAdapter.ForSources(loader,").count(), 4);
        assert!(!source.contains("new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,"));
        assert_eq!(source.matches("new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy[] {\n").count(), 4);
        for (index, name) in [(1, "alpha"), (3, "beta")] {
            assert_eq!(source.matches(&format!("new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy({index}, \"{name}\", ExtraXmlInputSchema_{index})")).count(), 4);
        }
        assert!(source.contains("FerruleXmlInputSource.Named(2, \"labels\")"));
    }
}

#[test]
fn one_dynamic_xml_source_keeps_original_single_policy_constructor_emission() {
    let project: mapping::Project = serde_json::from_str(include_str!(
        "fixtures/dynamic_named_xml_input_mixed.json"
    )).unwrap();
    let source = emit_xml_input_source(&codegen::lower(&project).unwrap());
    if XML_INPUT_LANGUAGE == "rust" {
        assert_eq!(source.matches("XmlDynamicSourceAdapter::new(loader,").count(), 4);
        assert!(!source.contains("XmlDynamicSourceAdapter::for_sources("));
        assert_eq!(source.matches("XmlDynamicSourcePolicy { declaration_index: 1, source: \"catalog\", schema: EXTRA_XML_INPUT_SCHEMA_1 }").count(), 4);
    } else {
        assert_eq!(source.matches("new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,").count(), 4);
        assert!(!source.contains("FerruleXmlDynamicSourceAdapter.ForSources("));
        assert_eq!(source.matches("new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy(1, \"catalog\", ExtraXmlInputSchema_1)").count(), 4);
    }
}

#[test]
fn multiple_dynamic_inputs_and_named_document_lists_emit_all_policies_and_only_four_xml_methods() {
    let mut project: mapping::Project = serde_json::from_str(include_str!(
        "fixtures/multiple_dynamic_named_inputs_static_primary_dynamic_named_xml_documents.json"
    )).unwrap();
    for zero_statics in [false, true] {
        if zero_statics {
            project.extra_sources.retain(|source| source.dynamic_path.is_some());
            project.graph.nodes.insert(7, mapping::Node::Const { value: ir::Value::Float(2.0) });
        }
        let program = codegen::lower(&project).unwrap();
        assert_eq!(program.xml_output_mode(), Ok(Some(codegen::XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments)));
        let source = emit_xml_input_source(&program);
        let indices = if zero_statics { [0, 1] } else { [1, 3] };
        if XML_INPUT_LANGUAGE == "rust" {
            for method in [
                "execute_xml_document_outputs_with_sources_and_dynamic_source_loader(",
                "execute_xml_document_outputs_with_sources_context_and_dynamic_source_loader(",
                "execute_xml_bytes_document_outputs_with_sources_and_dynamic_source_loader(",
                "execute_xml_bytes_document_outputs_with_sources_context_and_dynamic_source_loader(",
            ] { assert_eq!(source.matches(&format!("pub fn {method}")).count(), 1); }
            assert_eq!(source.matches("XmlDynamicSourceAdapter::for_sources(loader, vec![").count(), 4);
            assert!(!source.contains("XmlDynamicSourceAdapter::new(loader,"));
            for (index, name) in indices.into_iter().zip(["catalog", "codes"]) {
                assert_eq!(source.matches(&format!("XmlDynamicSourcePolicy {{ declaration_index: {index}, source: \"{name}\", schema: EXTRA_XML_INPUT_SCHEMA_{index} }}")).count(), 4);
            }
            assert!(!source.contains("pub fn execute_xml_documents("));
            assert!(!source.contains("pub fn execute_xml_outputs_with_sources_and_dynamic_source_loader("));
            if zero_statics { assert!(source.contains("let _ = &indices;")); }
            else { assert!(source.contains("XmlInputSource::Named { index: 2, name: \"unused\" }")); }
            let map = source.find("let mapped = execute_outputs_with_sources_and_dynamic_source_loader").unwrap();
            assert!(map < source.find("serialize_xml_document_outputs(mapped)").unwrap());
            let primary = source.find("serialize_xml_document(TARGET_XML_SCHEMA").unwrap();
            assert!(primary < source.find("serialize_xml_document(NAMED_XML_DOCUMENT_SCHEMA_0").unwrap());
            assert!(source.contains("named_serialization(1, NAMED_XML_DOCUMENT_OUTPUT_NAME_1, index, member.path(), error)"));
        } else {
            for method in [
                "ExecuteXmlDocumentOutputsWithSourcesAndDynamicSourceLoader(",
                "ExecuteXmlDocumentOutputsWithSourcesContextAndDynamicSourceLoader(",
                "ExecuteXmlBytesDocumentOutputsWithSourcesAndDynamicSourceLoader(",
                "ExecuteXmlBytesDocumentOutputsWithSourcesContextAndDynamicSourceLoader(",
            ] { assert_eq!(source.matches(method).count(), 1); }
            assert_eq!(source.matches("FerruleXmlDynamicSourceAdapter.ForSources(loader,").count(), 4);
            assert!(!source.contains("new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,"));
            for (index, name) in indices.into_iter().zip(["catalog", "codes"]) {
                assert_eq!(source.matches(&format!("new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy({index}, \"{name}\", ExtraXmlInputSchema_{index})")).count(), 4);
            }
            assert!(!source.contains(" ExecuteXmlDocuments("));
            assert!(!source.contains(" ExecuteXmlOutputsWithSourcesAndDynamicSourceLoader("));
            if zero_statics { assert!(source.contains("ExtraXmlInputNames = new string[] {  };")); }
            else { assert!(source.contains("FerruleXmlInputSource.Named(2, \"unused\")")); }
            let map = source.find("mapped = ExecuteOutputsWithSourcesAndDynamicSourceLoader").unwrap();
            assert!(map < source.find("return SerializeXmlDynamicInputDocumentOutputs(mapped)").unwrap());
            let primary = source.find("SerializeDocumentEmbedded(TargetXmlSchema").unwrap();
            assert!(primary < source.find("SerializeDocumentEmbedded(NamedXmlDocumentSchema0").unwrap());
            assert!(source.contains("NamedSerialization(1, NamedXmlDocumentOutputName1, index, member.Path, error)"));
        }
    }
}

#[test]
fn one_dynamic_input_and_named_document_lists_keep_single_policy_emission() {
    let project: mapping::Project = serde_json::from_str(include_str!(
        "fixtures/one_dynamic_named_input_static_primary_dynamic_named_xml_documents.json"
    )).unwrap();
    let source = emit_xml_input_source(&codegen::lower(&project).unwrap());
    if XML_INPUT_LANGUAGE == "rust" {
        assert_eq!(source.matches("XmlDynamicSourceAdapter::new(loader,").count(), 4);
        assert!(!source.contains("XmlDynamicSourceAdapter::for_sources("));
    } else {
        assert_eq!(source.matches("new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,").count(), 4);
        assert!(!source.contains("FerruleXmlDynamicSourceAdapter.ForSources("));
    }
}
