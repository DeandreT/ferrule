use super::*;
use sha2::{Digest, Sha256};
use std::io::Write;

pub(super) const M: usize = 64 * 1024 * 1024;
pub(super) const Q: usize = (M - 8) / 6;
pub(super) const R: usize = (M - 8) % 6;
pub(super) const S: usize = (M - 14) / 2;
pub(super) const SMALL_OUTPUT: &[u8] = b"{\n  \"n\": 7\n}\n";
pub(super) const STRING_PREFIX: &[u8] = b"{\n  \"n\": \"";
pub(super) const STRING_SUFFIX: &[u8] = b"\"\n}\n";
pub(super) const PROFILES: [&str; 4] = ["original", "normalized", "output-exact", "output-plus1"];

pub(super) fn program(profile: &str) -> TestResult<codegen::Program> {
    use codegen::{
        Binding, Expression, ExpressionNode, ScalarFunction, TargetConstruction, TargetScope,
    };
    let source_type = if profile == "original" {
        ir::ScalarType::Int
    } else {
        ir::ScalarType::String
    };
    let target_type = if matches!(profile, "original" | "normalized") {
        ir::ScalarType::Int
    } else {
        ir::ScalarType::String
    };
    let mut source = ir::SchemaNode::group("Input", vec![ir::SchemaNode::scalar("n", source_type)]);
    let ir::SchemaKind::Group { required, .. } = &mut source.kind else {
        return Err("literal source group required".into());
    };
    required.push("n".into());
    let target = ir::SchemaNode::group("Output", vec![ir::SchemaNode::scalar("n", target_type)]);
    let mut expressions = if profile == "normalized" {
        Vec::new()
    } else {
        vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                path: vec!["n".into()],
                frame: None,
            },
        }]
    };
    let output = match profile {
        "original" => 1,
        "normalized" => {
            expressions.push(ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: ir::Value::Int(7),
                },
            });
            2
        }
        "output-exact" | "output-plus1" => {
            let mut args = vec![1, 1];
            if profile == "output-plus1" {
                expressions.push(ExpressionNode {
                    id: 2,
                    expression: Expression::Const {
                        value: ir::Value::String("a".into()),
                    },
                });
                args.push(2);
            }
            expressions.push(ExpressionNode {
                id: 3,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args,
                },
            });
            3
        }
        _ => return Err("unknown literal resource profile".into()),
    };
    Ok(codegen::Program {
        xml_boundary: None,
        source,
        extra_sources: Vec::new(),
        target,
        expressions,
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        extra_targets: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "n".into(),
                expression: output,
                target_domain: codegen::ScalarTargetDomain::Single(target_type),
                repeating: false,
            }],
            children: Vec::new(),
        },
    })
}

// These are manual byte recipes, independent of either generated writer.
pub(super) fn expected_hash(long: bool) -> (usize, String) {
    let mut hash = Sha256::new();
    if long {
        hash.update(STRING_PREFIX);
        let block = [b'x'; 65536];
        let mut remaining = S * 2;
        while remaining > 0 {
            let n = remaining.min(block.len());
            hash.update(&block[..n]);
            remaining -= n;
        }
        hash.update(STRING_SUFFIX);
        (M, format!("{:x}", hash.finalize()))
    } else {
        hash.update(SMALL_OUTPUT);
        (SMALL_OUTPUT.len(), format!("{:x}", hash.finalize()))
    }
}

pub(super) fn write_repeat(writer: &mut impl Write, byte: u8, count: usize) -> TestResult<()> {
    let block = [byte; 65536];
    let mut left = count;
    while left > 0 {
        let n = left.min(block.len());
        writer.write_all(&block[..n])?;
        left -= n;
    }
    Ok(())
}

pub(super) fn prepare(root: &Path) -> TestResult<Vec<Json>> {
    assert_eq!((Q, R, S), (11_184_809, 2, 33_554_425));
    assert_eq!(STRING_PREFIX.len() + STRING_SUFFIX.len(), 14);
    assert_eq!(SMALL_OUTPUT.len(), 13);
    let mut cases = Vec::new();
    for id in [
        "original-exact",
        "original-plus1",
        "normalized-exact",
        "normalized-plus1",
        "output-exact",
        "output-plus1",
    ] {
        let path = root.join(format!("{id}.json5"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let (profile, original_bytes, normalized_bytes, outcome) = if id.starts_with("original") {
            file.write_all(b"{n:7}/*")?;
            write_repeat(&mut file, b'x', M - 9)?;
            file.write_all(b"*/")?;
            let plus = id.ends_with("plus1");
            if plus {
                file.write_all(b" ")?;
            }
            (
                "original",
                M + usize::from(plus),
                Some(7),
                if plus { "original-limit" } else { "success" },
            )
        } else if id.starts_with("normalized") {
            file.write_all(b"{n:'")?;
            write_repeat(&mut file, 0, Q)?;
            let plus = id.ends_with("plus1");
            write_repeat(&mut file, b'a', R + usize::from(plus))?;
            file.write_all(b"'}")?;
            (
                "normalized",
                Q + R + 6 + usize::from(plus),
                Some(M + usize::from(plus)),
                if plus { "normalized-limit" } else { "success" },
            )
        } else {
            file.write_all(b"{n:'")?;
            write_repeat(&mut file, b'x', S)?;
            file.write_all(b"'}")?;
            (
                id,
                S + 6,
                Some(S + 8),
                if id == "output-exact" {
                    "success"
                } else {
                    "output-limit"
                },
            )
        };
        file.sync_all()?;
        drop(file);
        let identity = support::identity(&path)?;
        assert_eq!(identity.bytes, original_bytes as u64);
        let (output_bytes, output_sha256) = expected_hash(id == "output-exact");
        let case = json!({"id":id,"profile":profile,"input":identity.json(),"outcome":outcome,
            "original_bytes":original_bytes,"normalized_bytes_literal":normalized_bytes,
            "output_bytes":if outcome == "success" { Some(output_bytes) } else { None },
            "output_sha256":if outcome == "success" { Some(output_sha256) } else { None },
            "normalized_limit_offset":if outcome == "normalized-limit" { Some(Q + R + 7) } else { None },
            "requested":if outcome.ends_with("limit") { Some(M + 1) } else { None },"max":M});
        std::fs::write(
            root.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&case)?,
        )?;
        cases.push(case);
    }
    Ok(cases)
}
