//! Explicit pre-filename XML work plus repeatable deferred materialization.
use crate::{DocumentMember, Instance, RuntimeError, SchemaNode, Value};
use ir::SchemaKind;
use std::fmt;

#[derive(Debug)]
pub enum DeferredXmlError {
    Evaluation(RuntimeError),
    Schema(codegen_schema::CodecError),
    Shape(&'static str),
    Limit,
}
impl fmt::Display for DeferredXmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evaluation(e) => e.fmt(f),
            Self::Schema(e) => e.fmt(f),
            Self::Shape(s) => write!(f, "deferred XML: {s}"),
            Self::Limit => f.write_str("deferred XML merge limit exceeded"),
        }
    }
}
impl std::error::Error for DeferredXmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(e) => Some(e),
            Self::Schema(e) => Some(e),
            _ => None,
        }
    }
}

pub struct DeferredXmlDocument<'a> {
    descriptor: &'a str,
    early: DocumentMember,
    late: Box<dyn Fn() -> Result<Instance, RuntimeError> + 'a>,
}
impl<'a> DeferredXmlDocument<'a> {
    pub fn new(
        descriptor: &'a str,
        early: DocumentMember,
        late: impl Fn() -> Result<Instance, RuntimeError> + 'a,
    ) -> Self {
        Self {
            descriptor,
            early,
            late: Box::new(late),
        }
    }
    pub fn path(&self) -> &str {
        self.early.path()
    }
    pub fn materialize(&self) -> Result<DocumentMember, DeferredXmlError> {
        let schema = codegen_schema::decode(self.descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
            .map_err(DeferredXmlError::Schema)?;
        let late = (self.late)().map_err(DeferredXmlError::Evaluation)?;
        let value = merge(&schema, Some(self.early.value()), Some(&late), 0, &mut 0)?
            .unwrap_or_else(|| Instance::Group(vec![].into()));
        DocumentMember::new(self.path(), value)
            .ok_or(DeferredXmlError::Shape("invalid prepared path"))
    }
}

fn present(value: Option<&Instance>) -> Option<&Instance> {
    value.filter(|v| !matches!(v, Instance::Scalar(Value::Null)))
}
fn merge(
    schema: &SchemaNode,
    a: Option<&Instance>,
    b: Option<&Instance>,
    depth: usize,
    nodes: &mut usize,
) -> Result<Option<Instance>, DeferredXmlError> {
    *nodes += 1;
    if depth > 64 || *nodes > 1_000_000 {
        return Err(DeferredXmlError::Limit);
    }
    let (a, b) = (present(a), present(b));
    if a.is_none() && b.is_none() {
        return Ok(None);
    }
    fn sequence(value: Option<&Instance>) -> Option<(&[Instance], bool)> {
        match value {
            Some(Instance::Repeated(rows)) => Some((rows.as_slice(), false)),
            Some(Instance::MappedSequence(rows)) => Some((rows.as_slice(), true)),
            _ => None,
        }
    }
    let (sa, sb) = (sequence(a), sequence(b));
    if sa.is_some() || sb.is_some() {
        if a.is_some() && sa.is_none() || b.is_some() && sb.is_none() {
            return Err(DeferredXmlError::Shape(
                "sequence ownership differs between phases",
            ));
        }
        let count = sa.or(sb).unwrap().0.len();
        if count > 1_000_000 - *nodes {
            return Err(DeferredXmlError::Limit);
        }
        let mapped = sa.or(sb).unwrap().1;
        if sa
            .zip(sb)
            .is_some_and(|(a, b)| a.0.len() != b.0.len() || a.1 != b.1)
        {
            return Err(DeferredXmlError::Shape(
                "sequence occurrences differ between phases",
            ));
        }
        let mut rows = Vec::with_capacity(count);
        for i in 0..count {
            rows.push(
                merge(
                    schema,
                    sa.map(|s| &s.0[i]),
                    sb.map(|s| &s.0[i]),
                    depth + 1,
                    nodes,
                )?
                .unwrap_or(Instance::Scalar(Value::Null)),
            );
        }
        return Ok(Some(if mapped {
            Instance::MappedSequence(rows)
        } else {
            Instance::Repeated(rows)
        }));
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            if a.is_some() && b.is_some() {
                return Err(DeferredXmlError::Shape("scalar belongs to both phases"));
            }
            let value = a.or(b).unwrap();
            if !matches!(value, Instance::Scalar(_)) {
                return Err(DeferredXmlError::Shape("non-scalar XML leaf"));
            }
            Ok(Some(value.clone()))
        }
        SchemaKind::Group { children, .. } => {
            for value in [a, b].into_iter().flatten() {
                let Instance::Group(fields) = value else {
                    return Err(DeferredXmlError::Shape("non-group XML container"));
                };
                let mut names = std::collections::BTreeSet::new();
                for (name, _) in fields.iter() {
                    if !names.insert(name) || !children.iter().any(|c| c.name == *name) {
                        return Err(DeferredXmlError::Shape("unknown or duplicate XML field"));
                    }
                }
            }
            let mut fields = Vec::new();
            for child in children {
                if let Some(value) = merge(
                    child,
                    a.and_then(|v| v.field(&child.name)),
                    b.and_then(|v| v.field(&child.name)),
                    depth + 1,
                    nodes,
                )? {
                    fields.push((child.name.clone(), value));
                }
            }
            Ok(Some(Instance::Group(fields.into())))
        }
        _ => Err(DeferredXmlError::Shape("unsupported XML schema kind")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn text(value: &str) -> Instance {
        Instance::Scalar(Value::String(value.into()))
    }
    fn group(name: &str, value: Instance) -> Instance {
        Instance::Group(vec![(name.into(), value)].into())
    }
    #[test]
    fn materialization_reuses_early_values_but_reexecutes_late_values() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Early", ir::ScalarType::String),
                SchemaNode::scalar("Late", ir::ScalarType::String),
            ],
        );
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let calls = Cell::new(0);
        let doc = DeferredXmlDocument::new(
            &descriptor,
            DocumentMember::new("one.xml", group("Early", text("saved"))).unwrap(),
            || {
                calls.set(calls.get() + 1);
                Ok(group("Late", text(&calls.get().to_string())))
            },
        );
        for expected in ["1", "2"] {
            let body = doc.materialize().unwrap();
            assert_eq!(body.path(), "one.xml");
            assert_eq!(body.value().field("Early"), Some(&text("saved")));
            assert_eq!(body.value().field("Late"), Some(&text(expected)));
        }
    }
    #[test]
    fn incompatible_phase_values_fail_without_silent_truncation() {
        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Text", ir::ScalarType::String)],
        );
        assert!(
            merge(
                &schema,
                Some(&group("Text", text("a"))),
                Some(&group("Text", text("a"))),
                0,
                &mut 0
            )
            .is_err()
        );
        let a = Instance::Repeated(vec![group("Text", text("a"))]);
        let b = Instance::Repeated(vec![]);
        assert!(merge(&schema, Some(&a), Some(&b), 0, &mut 0).is_err());
        assert!(merge(&schema, None, Some(&group("Unknown", text("a"))), 0, &mut 0).is_err());
    }
}
