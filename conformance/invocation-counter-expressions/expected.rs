//! Literal document oracles, independent of counter implementation arithmetic.

fn hex(text: &str) -> String {
    text.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[allow(dead_code)]
pub fn operands() -> String {
    let mut lines = Vec::new();
    for role in ["start", "step"] {
        for kind in [
            "missing",
            "string",
            "float",
            "decimal",
            "bool",
            "json-null",
            "xml-nil",
        ] {
            lines.push(format!("ERR|{role}|{kind}"));
        }
    }
    for kind in [
        "integer",
        "float",
        "decimal",
        "bool",
        "json-null",
        "xml-nil",
    ] {
        lines.push(format!("ERR|restart|{kind}"));
    }
    lines.push("ERR|unconfigured|before-operands".into());
    for presence in ["missing", "empty"] {
        for repeat in 0..2 {
            for (target, document) in [
                ("primary", "{\n  \"A\": 1,\n  \"B\": 2\n}\n"),
                ("audit", "{\n  \"Value\": 3\n}\n"),
            ] {
                lines.push(format!(
                    "DOC|{presence}-{repeat}-{target}|{}",
                    hex(document)
                ));
            }
        }
    }
    lines.join("\n")
}

#[allow(dead_code)]
pub fn rows() -> String {
    let document = "{\n  \"Items\": [\n    {\n      \"Value\": 10\n    },\n    {\n      \"Value\": 13\n    },\n    {\n      \"Value\": 20\n    },\n    {\n      \"Value\": 20\n    },\n    {\n      \"Value\": 30\n    },\n    {\n      \"Value\": 28\n    }\n  ]\n}\n";
    (0..3)
        .map(|repeat| format!("DOC|rows-{repeat}|{}", hex(document)))
        .collect::<Vec<_>>()
        .join("\n")
}
