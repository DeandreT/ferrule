use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{EdiAutocomplete, X12Autocomplete};

#[test]
fn x12_autocomplete_is_boundary_metadata_not_a_fixed_schema_value() {
    let directory = TempDir::new("boundary_metadata");
    let design = directory.path().join("mapping.mfd");
    std::fs::write(&design, self_describing_design()).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.target_options.edi_autocomplete,
        Some(EdiAutocomplete::X12(X12Autocomplete {
            request_acknowledgement: true,
            transaction_set: None,
        }))
    );
    assert_eq!(
        imported
            .project
            .target
            .child("ISA")
            .and_then(|isa| isa.child("FI01"))
            .and_then(|field| field.fixed.as_deref()),
        None
    );

    let exported = directory.path().join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&exported).unwrap();
    assert!(text.contains("autocompletedata=\"true\""), "{text}");
    assert!(text.contains("requestacknowledgement=\"true\""), "{text}");

    let roundtrip = mfd::import(&exported).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(
        roundtrip.project.target_options.edi_autocomplete,
        imported.project.target_options.edi_autocomplete
    );
}

#[test]
fn imported_x12_autocomplete_completes_unbound_isa_fields_with_the_run_clock() {
    let directory = TempDir::new("runtime_defaults");
    let design = directory.path().join("mapping.mfd");
    std::fs::write(&design, self_describing_design()).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let source = Instance::Group(vec![(
        "SRC".into(),
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("SENDER".into())),
        )]),
    )]);
    let timestamp = "2026-07-18T12:00:27-07:00";
    let execution = engine::ExecutionContext::new(&design).with_current_datetime(timestamp);
    let output = engine::run_with_context(&imported.project, &source, &execution).unwrap();

    let mapping_syntax = imported.project.target_options.x12_separators.unwrap();
    let syntax = format_edi::x12::Separators {
        element: mapping_syntax.element,
        component: mapping_syntax.component,
        segment: mapping_syntax.segment,
        repetition: mapping_syntax.repetition,
        release: mapping_syntax.release,
    };
    let Some(EdiAutocomplete::X12(autocomplete)) =
        imported.project.target_options.edi_autocomplete.as_ref()
    else {
        panic!("expected retained X12 autocomplete metadata");
    };
    let result = directory.path().join("result.x12");
    format_edi::x12::write_with_syntax_and_autocomplete(
        &result,
        &imported.project.target,
        &output,
        syntax,
        imported
            .project
            .target_options
            .x12_interchange_version
            .as_deref(),
        format_edi::x12::Autocomplete {
            current_datetime: timestamp,
            request_acknowledgement: autocomplete.request_acknowledgement,
            transaction_set: autocomplete.transaction_set.as_deref(),
        },
    )
    .unwrap();

    let written = std::fs::read_to_string(&result).unwrap();
    assert_eq!(
        written,
        "ISA+00+          +00+          +ZZ+SENDER         +ZZ+               +260718+1200+!+00505+000000000+1+P+:'\nIEA+0+000000000'\n"
    );
}

fn self_describing_design() -> &'static str {
    r#"<mapping version="26"><component name="map"><structure><children>
      <component name="source" library="text" kind="16"><data>
        <root><entry name="FileInstance"><entry name="document">
          <entry name="Source" ferrule-kind="group" ferrule-repeating="0">
            <entry name="SRC" ferrule-kind="group" ferrule-repeating="0">
              <entry name="Value" ferrule-kind="scalar" ferrule-repeating="0" datatype="string" outkey="10"/>
            </entry>
          </entry>
        </entry></entry></root>
        <text type="edi" kind="EDIX12" inputinstance="source.x12"/>
      </data></component>
      <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
        <root><entry name="FileInstance"><file role="outputinstance" name="result.x12"/><entry name="document">
          <entry name="Envelope" ferrule-kind="group" ferrule-repeating="0">
            <entry name="ISA" ferrule-kind="group" ferrule-repeating="0">
              <entry name="FI01" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI02" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI03" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI04" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI05_1" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI06" ferrule-kind="scalar" ferrule-repeating="0" datatype="string" inpkey="20"/>
              <entry name="FI05_2" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI07" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI08" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI09" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI65" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI11" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI12" ferrule-kind="scalar" ferrule-repeating="0" datatype="decimal"/>
              <entry name="FI13" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI14" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
              <entry name="FI15" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>
            </entry>
          </entry>
        </entry></entry></root>
        <text type="edi" kind="EDIX12"><settings autocompletedata="true" requestacknowledgement="true" interchangecontrolversionnumber="00505"><separators dataelement="+" component=":" segment="%27" repetition="%21"/></settings></text>
      </data></component>
    </children><graph><vertices>
      <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    </vertices></graph></structure></component></mapping>"#
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_x12_autocomplete_{label}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
