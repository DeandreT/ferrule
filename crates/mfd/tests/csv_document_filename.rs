use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use engine::{
    ExecutionContext, ExecutionPurpose, PrimaryFilenameEvaluationError, RuntimeParameters,
};
use ir::{Instance, Value};
use mapping::{GlobalFilename, Node, PrimaryFilenameError};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-document-filename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn write(&self, text: &str) -> std::io::Result<PathBuf> {
        let path = self.0.join("authored.mfd");
        std::fs::write(&path, text)?;
        Ok(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some_and(|v| v == "1") {
            eprintln!(
                "retained CSV document filename fixture: {}",
                self.0.display()
            );
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

const PARAMETER: &str = r#"<component name="path" library="core" kind="6"><targets><datapoint pos="0" key="1"/></targets><data><input datatype="string" usepreviewvalue="1" previewvalue="never-parameter-preview.csv"/><parameter usageKind="input" name="InputPath"/></data></component>"#;
const LITERAL: &str = r#"<component name="path" library="core" kind="2"><targets><datapoint pos="0" key="1"/></targets><data><constant datatype="string" value=" ./雪 &amp; β/*.csv "/></data></component>"#;

fn design(expression: &str, feed: u32) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
{expression}
<component name="source" library="text" kind="16"><data>
<root><entry name="FileInstance" ns="1" inpkey="13" expanded="1"><entry name="document"><entry name="Rows" outkey="10"><entry name="Value" outkey="11"/></entry></entry></entry></root>
<text type="csv" inputinstance="never-source-preview.csv"><settings separator="," firstrownames="true"><names root="Input" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry></entry></root>
<text type="csv" outputinstance="result.csv"><settings separator="|" firstrownames="false"><names root="Output" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
</children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="{feed}"><edges><edge vertexkey="13"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

fn compare_legacy(path: &Path, imported: &mfd::ImportedDocument) -> Result<(), Box<dyn Error>> {
    let ordinary = mfd::import(path)?;
    assert_eq!(imported.warnings, ordinary.warnings);
    assert_eq!(imported.mapping_path, ordinary.mapping_path);
    let mut body = imported.document.project().clone();
    body.prune_unreachable_nodes();
    assert_eq!(
        mapping::project_file::encode_pretty(&body)?,
        mapping::project_file::encode_pretty(&ordinary.project)?
    );
    assert!(imported.warnings.iter().any(|w| w == "CSV component `source` has connected FileInstance input port `13`; filename-driven documents are unsupported"));
    assert!(
        mfd::import_with_profile(
            path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
    assert!(
        !path
            .parent()
            .unwrap()
            .join("never-source-preview.csv")
            .exists()
    );
    assert!(
        !path
            .parent()
            .unwrap()
            .join("never-parameter-preview.csv")
            .exists()
    );
    Ok(())
}

#[test]
fn primary_filename_literal_survives_pruning_and_lossless_persistence() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new()?;
    let xml = design(LITERAL, 1);
    let path = fixture.write(&xml)?;
    let imported = mfd::import_csv_document(&path)?;
    compare_legacy(&path, &imported)?;
    let filename = imported.document.primary_source().filename;
    assert_eq!(
        imported.document.global_filename(),
        Ok(GlobalFilename::Literal(" ./雪 & β/*.csv "))
    );
    let mut document = imported.document.clone();
    document.prune_unreachable_nodes();
    let encoded = mapping::document_project_file::encode_pretty(&document)?;
    let decoded = mapping::document_project_file::decode_str(&encoded)?;
    assert_eq!(
        mapping::document_project_file::encode_pretty(&decoded)?,
        encoded
    );
    assert_eq!(decoded.primary_source().filename, filename);
    assert_eq!(
        engine::evaluate_primary_filename(&decoded, &ExecutionContext::new(&path))?,
        " ./雪 & β/*.csv "
    );
    let program = codegen::lower_document(&decoded)?;
    assert!(
        matches!(program.primary_filename(), codegen::PrimaryFilenamePlan::Literal { node, text } if *node == filename && text == " ./雪 & β/*.csv ")
    );
    let source = Instance::Repeated(vec![Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("kept".into())),
        )]
        .into(),
    )]);
    assert_eq!(engine::run(document.project(), &source)?, source);
    assert_eq!(std::fs::read_to_string(path)?, xml);
    Ok(())
}

#[test]
fn primary_filename_parameter_keeps_identity_and_ignores_both_previews()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let xml = design(PARAMETER, 1);
    let path = fixture.write(&xml)?;
    let imported = mfd::import_csv_document(&path)?;
    compare_legacy(&path, &imported)?;
    let node = imported.document.primary_source().filename;
    assert!(
        matches!(&imported.document.project().graph.nodes[&node], Node::RuntimeParameter {name, preview, ..}
        if name == "InputPath" && preview.as_deref() == Some("never-parameter-preview.csv"))
    );
    for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
        let execution = ExecutionContext::new(&path).with_purpose(purpose);
        assert!(
            matches!(engine::evaluate_primary_filename(&imported.document, &execution),
            Err(PrimaryFilenameEvaluationError::Parameter(engine::EngineError::MissingRuntimeParameter {node: found, name})) if found == node && name == "InputPath")
        );
        let mut parameters = RuntimeParameters::new();
        parameters.insert("InputPath", Value::String(" ./actual-*.csv ".into()))?;
        assert_eq!(
            engine::evaluate_primary_filename(
                &imported.document,
                &execution.with_parameters(&parameters)
            )?,
            " ./actual-*.csv "
        );
    }
    let program = codegen::lower_document(&imported.document)?;
    assert!(
        matches!(program.primary_filename(), codegen::PrimaryFilenamePlan::Parameter {node: found, name} if *found == node && name == "InputPath")
    );
    assert_eq!(std::fs::read_to_string(path)?, xml);
    Ok(())
}

#[test]
fn source_dependent_filename_stays_retained_but_is_not_admitted() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let xml = design("", 11);
    let path = fixture.write(&xml)?;
    let imported = mfd::import_csv_document(&path)?;
    compare_legacy(&path, &imported)?;
    let node = imported.document.primary_source().filename;
    assert!(
        matches!(&imported.document.project().graph.nodes[&node], Node::SourceField {path, ..} if path == &["Value"])
    );
    assert_eq!(
        imported.document.global_filename(),
        Err(PrimaryFilenameError::UnsupportedExpression { node })
    );
    assert!(
        matches!(codegen::lower_document(&imported.document), Err(codegen::DocumentLowerError::Filename(PrimaryFilenameError::UnsupportedExpression {node: found})) if found == node)
    );
    assert_eq!(std::fs::read_to_string(path)?, xml);
    Ok(())
}

#[test]
fn unconnected_ambiguous_and_controlled_wrappers_cannot_be_inferred() -> Result<(), Box<dyn Error>>
{
    let original = design(LITERAL, 1);
    let feed = r#"<vertex vertexkey="1"><edges><edge vertexkey="13"/></edges></vertex>"#;
    assert_eq!(original.matches(feed).count(), 1);
    let wrapper = r#"name="FileInstance" ns="1" inpkey="13" expanded="1""#;
    assert_eq!(original.matches(wrapper).count(), 1);
    let start = "<root><entry name=\"FileInstance\" ns=\"1\" inpkey=\"13\" expanded=\"1\">";
    assert_eq!(original.matches(start).count(), 1);
    for xml in [
        original.replace(feed, ""),
        original.replace(wrapper, &format!("{wrapper} condition=\"true\"")),
        original.replace(start, &format!("{start}<condition/>")),
        original.replace(
            feed,
            &format!(
                "{feed}<vertex vertexkey=\"11\"><edges><edge vertexkey=\"13\"/></edges></vertex>"
            ),
        ),
        original.replace(
            start,
            &format!(
                "<root><entry name=\"other\"/>{}",
                start.strip_prefix("<root>").unwrap()
            ),
        ),
    ] {
        let fixture = Fixture::new()?;
        let path = fixture.write(&xml)?;
        let error = mfd::import_csv_document(&path)
            .err()
            .ok_or("unsupported wrapper was retained")?;
        assert!(
            error
                .to_string()
                .contains("CSV document filename retention"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(path)?, xml);
    }
    Ok(())
}
