use std::error::Error;

use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};

use test_support::TempDir;

#[test]
fn narrowed_transparent_pdf_regions_reject_executable_import() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new("transparent-pdf-region")?;
    let path = temp.path().join("mapping.mfd");
    std::fs::write(
        &path,
        r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="text" kind="16"><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="10"><entry name="Value" outkey="11"/></entry></entry></entry></root>
<text type="csv"><settings firstrownames="false"><names root="Input" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry></entry></root>
<text type="csv"><settings firstrownames="false"><names root="Output" block="Rows"><field0 name="Value" type="string"/></names></settings></text>
</data></component>
<component name="unsupported-pdf" library="pdf" kind="34"><data>
<root><entry name="FileInstance"><file role="inputinstance" name="input.pdf"/>
<entry name="document" type="doc-pdf"><document schemafile="layout.pxt" root="Document"/>
<entry name="Document"><entry name="Value" outkey="101"/></entry></entry></entry></root>
</data></component></children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    for filter in ["", "1", "2"] {
        std::fs::write(
            temp.path().join("layout.pxt"),
            format!(
                "<Document><Template><Model><Root><Label>Document</Label><Children>\
                 <Grouping><Label/><Region>{{ Left: Left + 5pt, Top: Top, Right: Right, Bottom: Bottom }}</Region>\
                 <Kind><OneGroupPerPage/></Kind><Filter>{filter}</Filter><Children>\
                 <Capture><Label>Value</Label><Region>{{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }}</Region></Capture>\
                 </Children></Grouping></Children></Root></Model></Template></Document>"
            ),
        )?;
        let repair =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::BestEffort)?;
        assert!(!repair.report.executable);
        assert_eq!(repair.imported.project.source.name, "Input");
        assert!(repair.imported.project.extra_sources.is_empty());
        assert!(repair.report.issues.iter().any(|issue| {
            issue.kind == ImportIssueKind::ImportWarning
                && issue.message.contains("unnamed Grouping Region")
                && issue.message.contains("unsupported-pdf")
        }));
        assert!(matches!(
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
            Err(MfdError::IncompatibleImport(report)) if !report.executable
        ));
    }
    Ok(())
}
