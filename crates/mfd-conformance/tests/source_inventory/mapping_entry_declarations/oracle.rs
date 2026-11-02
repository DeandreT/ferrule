use super::schema::{Anchor, Inventory, Type};
use serde_json::{Value, json};

pub(super) const ORACLE: &str = include_str!(
    "../../../../../conformance/mfd-2026r2-source-inventory/oracles/mapping-entry-declarations.jsonl"
);

fn ty(t: &Type) -> Value {
    json!([t.spelling, t.resolved.0, t.binding])
}

fn anchors(values: &[Anchor]) -> Vec<Value> {
    values
        .iter()
        .map(|a| json!([a.source_index, a.declaration_lines]))
        .collect()
}

pub(super) fn rows(inventory: &Inventory) -> Vec<Value> {
    let mut rows = Vec::new();
    for p in &inventory.profiles {
        rows.push(json!([
            "P",
            p.id,
            p.manifest_sha256,
            p.generator_version,
            p.settings,
            p.static_input_count,
            p.output_count,
            p.api_scope
        ]));
    }
    for s in &inventory.sources {
        rows.push(json!([
            "S",
            s.index,
            s.profile,
            s.language,
            s.path,
            s.bytes,
            s.sha256,
            s.lf_lines,
            s.namespace,
            s.namespace_lines,
            s.imports
                .iter()
                .map(|i| json!([i.name, i.declaration_lines]))
                .collect::<Vec<_>>(),
            s.conditions
        ]));
    }
    for o in &inventory.owners {
        rows.push(json!([
            "O",
            o.name,
            o.language,
            o.kind,
            o.modifiers,
            o.extends.iter().map(ty).collect::<Vec<_>>(),
            o.implements.iter().map(ty).collect::<Vec<_>>(),
            o.anchors
                .iter()
                .map(|a| json!([a.source_index, a.declaration_lines, a.closing_line]))
                .collect::<Vec<_>>(),
            o.explicit_constructors,
            o.conditions,
            o.assessment_ref
        ]));
        for m in &o.members {
            rows.push(json!([
                "M",
                o.name,
                m.index,
                m.kind,
                m.name,
                ty(&m.r#type),
                m.modifiers,
                m.parameters
                    .iter()
                    .map(|p| json!([p.index, p.name, ty(&p.r#type), p.direction, p.default.0]))
                    .collect::<Vec<_>>(),
                m.throws.iter().map(ty).collect::<Vec<_>>(),
                m.accessors
                    .iter()
                    .map(|a| json!([
                        a.name,
                        a.visibility,
                        a.explicit_visibility.0,
                        anchors(&a.anchors)
                    ]))
                    .collect::<Vec<_>>(),
                anchors(&m.anchors),
                m.directly_declared,
                m.conditions
            ]));
        }
    }
    rows
}

pub(super) fn validate(inventory: &Inventory) -> Result<(), String> {
    let expected: Vec<Value> = ORACLE
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let actual = rows(inventory);
    if actual != expected {
        let mismatch = actual.iter().zip(&expected).position(|(a, e)| a != e);
        return Err(format!(
            "independent source oracle mismatch at {mismatch:?}"
        ));
    }
    Ok(())
}
