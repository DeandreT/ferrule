use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::PdfRepairDependency;
use mfd::{ExportProfile, ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_pdf_object_repair_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn template(text_groups: bool) -> String {
    let kind = if text_groups {
        "<GroupByText><Region/><Mode><filter-containing/></Mode><Match><Search>MARK</Search><AllowArbitrarySpace>1</AllowArbitrarySpace><WordAnchor><none/></WordAnchor><CaseFolding><ignore-case/></CaseFolding></Match></GroupByText>"
    } else {
        "<OneGroupPerPage/>"
    };
    format!(
        "<Document><Template><Model><Root><Label>Pdf</Label><Children><Grouping><Label/><Region/><Kind><OneGroupPerPage/></Kind><Filter>1</Filter><Children><Splitter><Region/><Search/><FeatureFind><ObjectFind><Background>#fff</Background><Tolerance>10</Tolerance><MinimumExtent>10pt</MinimumExtent><Fill>0pt</Fill><Edge><start/></Edge><Displace>0pt</Displace></ObjectFind></FeatureFind><PostProcess><MinimumExtent/><Behavior><discard/></Behavior></PostProcess><SkipInitial>0</SkipInitial><SkipFinal>0</SkipFinal><Children><Grouping><Label>Row</Label><Region/><Kind>{kind}</Kind><Filter/><Children><Capture><Label>Value</Label><Region>{{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }}</Region></Capture></Children></Grouping></Children></Splitter></Children></Grouping></Children></Root></Model></Template></Document>"
    )
}

fn design() -> &'static str {
    r#"<mapping version="42"><resources/><component name="repair" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
      <component name="pdf-source" library="pdf" uid="2" kind="34"><data><root><header><namespaces><namespace/></namespaces></header><entry name="FileInstance"><file role="inputinstance" name="missing.pdf"/><entry name="document" type="doc-pdf"><document schemafile="layout.pxt" root="Pdf"/><entry name="Pdf"><entry name="Row" outkey="10"><entry name="Value" outkey="11"/></entry></entry></entry></entry></root></data></component>
      <component name="rows" library="text" uid="3" kind="16"><properties XSLTDefaultOutput="1"/><data><root><header><namespaces><namespace/></namespaces></header><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry></entry></root><text type="csv" outputinstance="out.csv"><settings separator="," firstrownames="true"><names root="Output" block="Rows"><field0 name="Value" type="string"/></names></settings></text></data></component>
    </children><graph directed="1"><edges/><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex></vertices></graph></structure></component></mapping>"#
}

#[test]
fn object_finder_preserves_repair_ports_but_blocks_execution_and_export_after_reload() {
    for text_groups in [false, true] {
        let temp = TempDir::new();
        let path = temp.0.join("mapping.mfd");
        std::fs::write(&path, design()).unwrap();
        std::fs::write(temp.0.join("layout.pxt"), template(text_groups)).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
        assert!(imported.warnings[0].contains("ObjectFind"));
        assert!(engine::validate(&imported.project).is_empty());
        let row = imported.project.source.child("Row").unwrap();
        assert!(row.repeating && row.child("Value").is_some());
        let source = Instance::Group(vec![(
            "Row".into(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String("host-parsed".into())),
            )])]),
        )]);
        assert!(
            engine::run(&imported.project, &source).is_ok(),
            "intentional typed host input remains usable"
        );
        assert!(
            matches!(mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable), Err(MfdError::IncompatibleImport(report)) if report.issues.iter().any(|issue| issue.kind == ImportIssueKind::RuntimeDependency && issue.message.contains("ObjectFind")))
        );

        let reopened = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&imported.project).unwrap(),
        )
        .unwrap();
        let clean_reload = mfd::Imported {
            project: reopened,
            warnings: Vec::new(),
            mapping_path: path.clone(),
        };
        let report = mfd::assess_import(&clean_reload);
        assert!(!report.executable);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].kind, ImportIssueKind::RuntimeDependency);
        let layout = clean_reload.project.source_options.pdf.as_ref().unwrap();
        assert_eq!(
            layout.repair_dependency(),
            Some(PdfRepairDependency::ObjectFind)
        );
        assert!(matches!(
            format_pdf::read(&temp.0.join("does-not-exist.bin"), layout),
            Err(format_pdf::PdfError::RepairDependency(
                PdfRepairDependency::ObjectFind
            ))
        ));
        assert!(matches!(
            format_pdf::from_bytes(b"not a PDF", layout),
            Err(format_pdf::PdfError::RepairDependency(
                PdfRepairDependency::ObjectFind
            ))
        ));
        // A private layout payload must not erase the persisted native blocker.
        let payload = serde_json::to_string(layout)
            .unwrap()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        std::fs::write(
            temp.0.join("layout.pxt"),
            format!("<Document><FerruleLayout version=\"1\">{payload}</FerruleLayout></Document>"),
        )
        .unwrap();
        let canonical = mfd::import(&path).unwrap();
        assert_eq!(canonical.warnings.len(), 1);
        assert_eq!(canonical.project.pdf_runtime_dependencies().len(), 1);
        assert!(!mfd::assess_import(&canonical).executable);
        for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
            let output = temp.0.join(format!("blocked-{profile:?}/mapping.mfd"));
            assert!(
                matches!(mfd::export_with_profile(&clean_reload.project, &output, profile), Err(MfdError::Unsupported(message)) if message.contains("ObjectFind"))
            );
            assert!(!output.parent().unwrap().exists());
        }
    }
}
