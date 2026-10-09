mod support;

use support::{Result, require};

fn main_inner(args: &[String]) -> Result<()> {
    require(
        args.len() == 4 || args.len() == 8,
        "measure MODE INPUT FRESH_DIR; verify MODE COUNT WIDTH INPUT EXPECTED TIMED FRESH_DIR",
    )?;
    support::mode(&args[1])?;
    let mode = args[1].as_str();
    match (args[0].as_str(), args.len()) {
        ("measure", 4) => {
            let paths = support::paths(&args[2..])?;
            support::fresh_dir(&paths[1])?;
            let started = std::time::Instant::now();
            let text = support::read_text(&paths[0])?;
            support::phase("before-call", started)?;
            let actual = if mode == "numeric" {
                numeric::execute_json(&text)
            } else {
                capture::execute_json(&text)
            };
            match actual {
                Ok(document) => {
                    support::phase("after-call", started)?;
                    require(
                        document.len() < support::CEILING,
                        "output exceeds study ceiling",
                    )?;
                    support::write(&paths[1].join("actual.json"), document.as_bytes())?;
                    support::phase("after-write", started)?;
                }
                Err(error) => {
                    support::retain(
                        &paths[1].join("error.original.json"),
                        &support::error(&error),
                    )?;
                    return Err(error.into());
                }
            }
        }
        ("verify", 8) => {
            let count: usize = args[2].parse()?;
            let width: usize = args[3].parse()?;
            support::dimension(count, width)?;
            let paths = support::paths(&args[4..])?;
            let dir = &paths[3];
            support::fresh_dir(dir)?;
            let input_row = support::retain_file(&paths[0], &dir.join("input.original.json"))?;
            support::retain(&dir.join("input-identity.original.json"), &input_row)?;
            let text = support::read_text(&dir.join("input.original.json"))?;
            let source = codegen_runtime::parse_json(include_str!("source-schema.json"), &text);
            support::retain(
                &dir.join("source-result.original.json"),
                &support::outcome(&source),
            )?;
            if let Ok(value) = &source {
                support::retain(
                    &dir.join("source-typed.original.json"),
                    &support::instance(value),
                )?;
            }
            let expected_source = support::expected_source(count, width);
            let expected_output = support::expected_output(mode, count, width);
            support::retain(
                &dir.join("expected-source.original.json"),
                &support::instance(&expected_source),
            )?;
            support::retain(
                &dir.join("expected-output.original.json"),
                &support::instance(&expected_output),
            )?;
            let actual = source.as_ref().ok().map(|value| {
                if mode == "numeric" {
                    numeric::execute(value)
                } else {
                    capture::execute(value)
                }
            });
            if let Some(result) = &actual {
                support::retain(
                    &dir.join("execute-result.original.json"),
                    &support::outcome(result),
                )?;
                if let Ok(value) = result {
                    support::retain(
                        &dir.join("execute-typed.original.json"),
                        &support::instance(value),
                    )?;
                }
            } else {
                support::retain(
                    &dir.join("execute-result.original.json"),
                    &serde_json::json!({"outcome":"NotReached", "cause":"source parse failed"}),
                )?;
            }
            let bytes_equal = support::verify_bytes(&paths[1], &paths[2], dir)?;
            let source_equal = source.as_ref().is_ok_and(|value| value == &expected_source);
            let output_equal = actual
                .as_ref()
                .is_some_and(|result| result.as_ref().is_ok_and(|value| value == &expected_output));
            support::retain(
                &dir.join("verification.original.json"),
                &serde_json::json!({
                "source_equal":source_equal, "output_equal":output_equal,
                "bytes_equal":bytes_equal, "input_stable":input_row["stable"]}),
            )?;
            require(
                source_equal && output_equal && bytes_equal && input_row["stable"] == true,
                "complete retained typed/byte verification failed",
            )?;
        }
        _ => return Err(std::io::Error::other("unsupported host command").into()),
    }
    Ok(())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = main_inner(&args) {
        eprintln!("{}", support::error(error.as_ref()));
        std::process::exit(1);
    }
}
