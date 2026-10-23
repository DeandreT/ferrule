use std::{collections::BTreeMap, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["source.mfd", "a.json", "b.json"] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}
fn fixture() -> Fixture {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ferrule-csv-json-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let schema = r#"{"title":"Output","type":"object","properties":{"value":{"type":"string"}}}"#;
    for name in ["a.json", "b.json"] {
        std::fs::write(root.join(name), schema).unwrap();
    }
    let function = |name: &str, kind: u32, inputs: &str, outputs: &str, data: &str| {
        format!(
            "<component library='core' name='{name}' kind='{kind}' uid='{}'><sources>{inputs}</sources><targets>{outputs}</targets>{data}</component>",
            match name {
                "InputPath" => 1,
                "group-by-order" => 2,
                "group-by-flag" => 3,
                "concat" => 4,
                "constant" => 5,
                _ => 6,
            }
        )
    };
    let mut components = String::new();
    components.push_str(&function("InputPath",6,"<datapoint/>","<datapoint pos='0' key='1'/>","<data><input datatype='string' previewvalue='must-not-open.csv'/><parameter name='InputPath' usageKind='input'/></data>"));
    components.push_str("<component name='InputComponent' library='text' kind='16' uid='10'><properties DynamicFilenames='1'/><data><root><header><namespaces><namespace/><namespace uid='http://www.altova.com/mapforce'/></namespaces></header><entry name='FileInstance' ns='1' inpkey='5' outkey='2'><entry name='document' ns='1'><entry name='Rows' outkey='10'><entry name='Order' outkey='11'/><entry name='Flag' outkey='12'/><entry name='Value' outkey='13'/></entry></entry></entry></root><text type='csv' encoding='1000'><settings separator='|' quote='&quot;' firstrownames='false' removeempty='true'><names root='Input' block='Rows'><field0 name='Order' type='string'/><field1 name='Flag' type='string'/><field2 name='Value' type='string'/></names></settings></text></data></component>");
    for (uid, a, b, out) in [(20, 21, 22, 23), (24, 25, 26, 27)] {
        components.push_str(&format!("<component library='core' name='group-by' kind='5' uid='{uid}'><sources><datapoint pos='0' key='{a}'/><datapoint pos='1' key='{b}'/></sources><targets><datapoint/><datapoint pos='1' key='{out}'/></targets></component>"));
    }
    components.push_str("<component library='db' name='is-null' kind='5' uid='30'><sources><datapoint pos='0' key='31'/></sources><targets><datapoint pos='0' key='32'/></targets></component>");
    components.push_str("<component library='core' name='constant' kind='2' uid='40'><targets><datapoint pos='0' key='41'/></targets><data><constant value='.json' datatype='string'/></data></component>");
    components.push_str("<component library='core' name='concat' kind='5' uid='45'><sources><datapoint pos='0' key='46'/><datapoint pos='1' key='47'/></sources><targets><datapoint pos='0' key='48'/></targets></component>");
    components.push_str("<component library='core' name='if-else' kind='4' uid='50'><sources><datapoint pos='0' key='51'/><datapoint pos='1' key='52'/><datapoint/></sources><targets><datapoint pos='0' key='53'/></targets></component>");
    for (name, schema, uid, filename, value) in
        [("A", "a.json", 60, 61, 62), ("B", "b.json", 63, 64, 65)]
    {
        components.push_str(&format!("<component library='json' name='{name}' kind='31' uid='{uid}'><properties DynamicFilenames='1'/><data><root><header><namespaces><namespace/><namespace uid='http://www.altova.com/mapforce'/></namespaces></header><entry name='FileInstance' ns='1' inpkey='{filename}'><entry name='document' ns='1' casttotargettypemode='cast-in-subtree'><entry name='root'><entry name='object'><entry name='value' type='json-property'><entry name='string' inpkey='{value}'/></entry></entry></entry></entry></entry></root><json schema='{schema}' json5='1'/></data></component>"));
    }
    let mut feeds = BTreeMap::<u32, Vec<u32>>::new();
    for (input, output) in [
        (5, 1),
        (21, 10),
        (22, 11),
        (25, 10),
        (26, 12),
        (31, 27),
        (46, 23),
        (47, 41),
        (51, 32),
        (52, 48),
        (61, 53),
        (64, 53),
        (62, 13),
        (65, 13),
    ] {
        feeds.entry(output).or_default().push(input);
    }
    let mut graph = String::new();
    for (output, inputs) in feeds {
        graph.push_str(&format!("<vertex vertexkey='{output}'><edges>"));
        for input in inputs {
            graph.push_str(&format!("<edge vertexkey='{input}'/>"));
        }
        graph.push_str("</edges></vertex>");
    }
    std::fs::write(root.join("source.mfd"),format!("<mapping version='30'><component name='map' uid='100'><structure><children>{components}</children><graph><vertices>{graph}</vertices></graph></structure></component></mapping>")).unwrap();
    Fixture(root)
}

#[test]
fn explicit_import_preserves_schemas_and_file_set_shape() {
    let fixture = fixture();
    let path = fixture.0.join("source.mfd");
    let original = mfd::import(&path).unwrap();
    let imported =
        mfd::import_csv_json_document_with_options(&path, &mfd::ImportOptions::default())
            .unwrap_or_else(|error| panic!("{error}: {:?}", original.warnings));
    assert_eq!(imported.row_schema, original.project.source);
    assert_eq!(imported.document.project().target, original.project.target);
    assert_eq!(
        imported.document.project().source,
        mapping::csv_file_set_source_schema(&imported.row_schema)
    );
    assert_eq!(
        imported.document.project().extra_targets[0].schema,
        original.project.extra_targets[0].schema
    );
    assert!(engine::validate(imported.document.project()).is_empty());
    let program = codegen::lower_document(&imported.document).unwrap();
    let policy =
        codegen::CsvInputPolicy::from_input_format_options(&imported.source_options).unwrap();
    let options = codegen_rust::Options {
        package_name: "csv_json_neutral".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    assert!(
        codegen_rust::emit_csv_file_set_to_mapforce_json5_documents(
            &program,
            &options,
            &imported.row_schema,
            &policy,
            None
        )
        .is_ok()
    );
    assert!(
        codegen_csharp::emit_csv_file_set_to_mapforce_json5_documents(
            &program,
            &imported.row_schema,
            &policy,
            Some("B")
        )
        .is_ok()
    );
    assert!(codegen::prepare_csv_to_mapforce_json5_documents(&program, &policy, None).is_err());
}

#[test]
fn unknown_endpoint_settings_do_not_gain_admission() {
    let fixture = fixture();
    let path = fixture.0.join("source.mfd");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("json5='1'", "json5='1' unknown='1'")).unwrap();
    assert!(
        mfd::import_csv_json_document_with_options(&path, &mfd::ImportOptions::default()).is_err()
    );
}
