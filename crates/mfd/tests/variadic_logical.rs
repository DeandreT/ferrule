use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, Value};
use mapping::Node;

#[test]
fn authored_variadic_logical_mfds_retain_positions_and_complete_boolean_results()
-> Result<(), Box<dyn std::error::Error>> {
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/variadic_logical");
    let parent = std::env::var_os("FERRULE_LOGICAL_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if !parent.is_absolute() {
        return Err("evidence parent must be absolute".into());
    }
    let evidence = parent.join(format!(
        "ferrule-logical237-mfd-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir_all(&evidence)?;
    eprintln!("LOGICAL237_MFD_ORIGINALS={}", evidence.display());
    for entry in std::fs::read_dir(&fixtures)? {
        let path = entry?.path();
        std::fs::copy(
            &path,
            evidence.join(path.file_name().ok_or("fixture name")?),
        )?;
    }
    let mut admissions = Vec::new();
    for function in ["and", "or"] {
        for count in [3_u32, 4, 5] {
            let design = fixtures.join(format!("{function}-{count}.mfd"));
            let result = mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default().with_package_root(&fixtures),
                mfd::ImportProfile::Executable,
            );
            let original = match &result {
                Ok(outcome) => format!(
                    "project={:#?}\nreport={:#?}\nwarnings={:#?}\n",
                    outcome.imported.project, outcome.report, outcome.imported.warnings
                ),
                Err(error) => format!("{error:#?}\n{error}\n"),
            };
            std::fs::write(
                evidence.join(format!("{function}-{count}-COMPLETE-ADMISSION.txt")),
                original,
            )?;
            admissions.push((function, count, result));
        }
    }
    if admissions.iter().any(|(_, _, result)| result.is_err()) {
        return Err("complete authored MFD admission differs; originals retained".into());
    }
    let mut all = Vec::new();
    let mut positions = Vec::new();
    for (function, count, result) in admissions {
        let imported = result?.imported;
        for (id, node) in &imported.project.graph.nodes {
            if let Node::Call {
                function: actual,
                args,
            } = node
                && actual == function
            {
                let paths = args
                    .iter()
                    .map(|id| match imported.project.graph.nodes.get(id) {
                        Some(Node::SourceField { path, .. }) => Some(path.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                positions.push((function, count, *id, args.clone(), paths));
            }
        }
        for mask in 0_u32..32 {
            let source_xml = format!(
                "<Source>{}</Source>",
                (0..5)
                    .map(|index| format!(
                        "<Arg{}>{}</Arg{}>",
                        index + 1,
                        if mask & (1 << index) == 0 {
                            "false"
                        } else {
                            "true"
                        },
                        index + 1
                    ))
                    .collect::<String>()
            );
            let source = format_xml::from_str(&source_xml, &imported.project.source)?;
            let actual = engine::run(&imported.project, &source);
            let raw_wire = actual.as_ref().map(|output| {
                format_xml::to_string_with_options(
                    &imported.project.target,
                    output,
                    &format_xml::XmlWriteOptions {
                        declaration: false,
                        indent: false,
                        ..Default::default()
                    },
                )
            });
            let complete_wire_original = format!("{raw_wire:#?}");
            let wire = raw_wire
                .map(|result| result.map_err(|error| error.to_string()))
                .map_err(|error| error.to_string());
            let selected = mask & ((1 << count) - 1);
            // Independent bit-mask oracle, frozen separately from reducer implementation.
            let expected = if function == "and" {
                selected == (1 << count) - 1
            } else {
                selected != 0
            };
            let wanted_wire = format!(
                "<Target><Result>{}</Result></Target>",
                if expected { "true" } else { "false" }
            );
            let original = (
                serde_json::to_value(actual.as_ref().ok())?,
                complete_wire_original,
                match &source {
                    Instance::Group(fields) => Some(format!("{:?}", fields.xml_type_origin())),
                    _ => None,
                },
                match &actual {
                    Ok(Instance::Group(fields)) => Some(format!("{:?}", fields.xml_type_origin())),
                    _ => None,
                },
            );
            all.push((
                function,
                count,
                mask,
                source_xml,
                source,
                actual,
                original,
                wire,
                expected,
                wanted_wire,
            ));
        }
    }
    std::fs::write(
        evidence.join("ALL-COMPLETE-POSITIONS.txt"),
        format!("{positions:#?}\n"),
    )?;
    std::fs::write(
        evidence.join("ALL-COMPLETE-INPUTS-OUTCOMES-EXPECTED.txt"),
        format!("{all:#?}\n"),
    )?;
    let positions_match = positions.len() == 6
        && positions.iter().all(|(_, count, _, args, paths)| {
            args.len() == *count as usize
                && paths
                    == &(1..=*count)
                        .map(|index| Some(vec![format!("Arg{index}")]))
                        .collect::<Vec<_>>()
        });
    let mismatches = all.iter().filter(|(_, _, _, _, _, actual, _, wire, expected, wanted_wire)| {
        !matches!(actual, Ok(Instance::Group(fields)) if fields.len() == 1 && fields[0].0 == "Result" && fields[0].1 == Instance::Scalar(Value::Bool(*expected)))
            || !matches!(wire, Ok(Ok(actual)) if actual == wanted_wire)
    }).collect::<Vec<_>>();
    std::fs::write(
        evidence.join("COMPLETE-COMPARISONS.txt"),
        format!(
            "calls={}\npositions_match={positions_match}\n{mismatches:#?}\n",
            all.len()
        ),
    )?;
    if all.len() != 192 || !positions_match || !mismatches.is_empty() {
        return Err("complete authored MFD logical cohort differs".into());
    }
    Ok(())
}
