use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Package(PathBuf);
impl Package {
    fn new(cast_first: bool, explicit: bool) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_cli_float_cast_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        let package = Self(root);
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/target_float_cast");
        for name in ["source.xsd", "target.xsd"] {
            std::fs::copy(fixture.join(name), package.0.join(name))?;
        }
        let mut mapping = std::fs::read_to_string(fixture.join("cast-first.mfd"))?;
        if !explicit {
            assert!(mapping.contains(" casttotargettypemode=\"cast-in-subtree\""));
            mapping = mapping.replace(" casttotargettypemode=\"cast-in-subtree\"", "");
        }
        if !cast_first {
            mapping = mapping
                .replace(
                    "<edge vertexkey=\"120\"/>",
                    "<edge vertexkey=\"temporary\"/>",
                )
                .replace("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"120\"/>")
                .replace(
                    "<edge vertexkey=\"temporary\"/>",
                    "<edge vertexkey=\"20\"/>",
                );
        }
        let design = package.0.join("mapping.mfd");
        std::fs::write(&design, mapping)?;
        let warnings = cli::import_mfd_with_profile(
            &design,
            &package.0.join("project.json"),
            Some(&package.0),
            None,
            &[],
            &[],
            mfd::ImportProfile::Executable,
        )?;
        assert!(warnings.is_empty());
        Ok(package)
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn numeric_target_failure_order_is_exact_and_never_publishes_partial_output()
-> Result<(), Box<dyn Error>> {
    for cast_first in [true, false] {
        for explicit in [true, false] {
            let package = Package::new(cast_first, explicit)?;
            let source = package.0.join("input.xml");
            std::fs::write(
                &source,
                "<Envelope><Numerator>1</Numerator><Denominator>0</Denominator><Text>not-a-number</Text><Label>safe</Label></Envelope>",
            )?;
            let expected = if cast_first {
                engine::EngineError::Function(codegen_runtime::FunctionError::InvalidArgument {
                    function: "coerce_float",
                    message: "requires finite floating-point text",
                })
            } else {
                engine::EngineError::Function(codegen_runtime::FunctionError::DivideByZero)
            };
            for existing in [false, true] {
                let output = package.0.join(if existing {
                    "existing.xml"
                } else {
                    "absent/output.xml"
                });
                let sentinel = b"unchanged target\n";
                if existing {
                    std::fs::write(&output, sentinel)?;
                }
                let error = cli::run_project(&package.0.join("project.json"), &source, &output)
                    .unwrap_err();
                assert_eq!(error.downcast_ref::<engine::EngineError>(), Some(&expected));
                let binary = Command::new(env!("CARGO_BIN_EXE_ferrule"))
                    .args(["--diagnostics", "json", "run", "--project"])
                    .arg(package.0.join("project.json"))
                    .arg("--input")
                    .arg(&source)
                    .arg("--output")
                    .arg(&output)
                    .output()?;
                assert_eq!(binary.status.code(), Some(1));
                assert!(binary.stdout.is_empty());
                let diagnostics = std::str::from_utf8(&binary.stderr)?
                    .lines()
                    .map(serde_json::from_str::<serde_json::Value>)
                    .collect::<Result<Vec<_>, _>>()?;
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0]["command"], "run");
                assert_eq!(diagnostics[0]["severity"], "error");
                assert_eq!(diagnostics[0]["message"], expected.to_string());
                if existing {
                    assert_eq!(std::fs::read(&output)?, sentinel);
                } else {
                    assert!(!output.parent().unwrap().exists());
                }
            }
            std::fs::write(
                &source,
                "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>29</Text><Label>safe</Label></Envelope>",
            )?;
            let output = package.0.join("success.xml");
            cli::run_project(&package.0.join("project.json"), &source, &output)?;
            let values = if cast_first { [29, 17] } else { [17, 29] };
            assert_eq!(
                std::fs::read_to_string(&output)?,
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Envelope>\n  <Amount>{}</Amount>\n  <Amount>{}</Amount>\n  <Label>safe</Label>\n</Envelope>",
                    values[0], values[1]
                )
            );
        }
    }
    Ok(())
}
