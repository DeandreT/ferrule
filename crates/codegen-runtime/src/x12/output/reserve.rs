//! Borrowed, occurrence-weighted reservations before eager native output.
use super::*;

#[derive(Clone, Copy)]
pub(in super::super) struct OutputLimits {
    pub names: usize,
    pub projected: usize,
}
impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            names: MAX_X12_MATERIALIZED_NAME_BYTES,
            projected: MAX_X12_PROJECTED_SCALAR_BYTES,
        }
    }
}

struct Reservation<'a> {
    profile: &'a Profile,
    limits: OutputLimits,
    budget: Budget,
    projected: usize,
    raw: usize,
    groups: usize,
    transactions: usize,
}

pub(in super::super) fn preflight(
    profile: &Profile,
    document: &Instance,
    limits: OutputLimits,
) -> Result<(), X12BoundaryError> {
    let mut reservation = Reservation {
        profile,
        limits,
        budget: Budget::default(),
        projected: 0,
        raw: 0,
        groups: 0,
        transactions: 0,
    };
    reservation.node(
        &profile.schema,
        Some(document),
        &mut Vec::new(),
        0,
        true,
        false,
    )
}

fn charge(
    total: &mut usize,
    amount: usize,
    resource_name: &'static str,
    maximum: usize,
) -> Result<(), X12BoundaryError> {
    *total = match total.checked_add(amount) {
        Some(bytes) => bytes,
        None => {
            return Err(X12BoundaryError::Resource {
                resource: resource_name,
                observed: usize::MAX,
                maximum,
            });
        }
    };
    resource(resource_name, *total, maximum)
}

impl Reservation<'_> {
    fn add(&mut self, bytes: usize) -> Result<(), X12BoundaryError> {
        charge(
            &mut self.projected,
            bytes,
            "x12_projected_scalar_bytes",
            self.limits.projected,
        )
    }

    fn node(
        &mut self,
        schema: &SchemaNode,
        value: Option<&Instance>,
        path: &mut Vec<String>,
        depth: usize,
        root: bool,
        item: bool,
    ) -> Result<(), X12BoundaryError> {
        self.budget.node(depth)?;
        if schema.repeating && !root && !item {
            let items = match value {
                None => &[][..],
                Some(Instance::Repeated(items) | Instance::MappedSequence(items)) => items,
                _ => return Err(self.bad(path, "repeating output requires a collection")),
            };
            self.budget.loops(items.len())?;
            for value in items {
                self.node(schema, Some(value), path, depth + 1, false, true)?;
            }
            return Ok(());
        }
        match &schema.kind {
            SchemaKind::Scalar { .. } => {
                self.scalar(schema, value.and_then(Instance::as_scalar), path, 0)
            }
            SchemaKind::Group { .. } => {
                let id = (!root).then(|| segment_id(&schema.name)).flatten();
                if value.is_none() {
                    if self.profile.autocomplete.is_some()
                        && matches!(id, Some("SE" | "GE" | "IEA"))
                    {
                        self.add(18)?;
                    }
                    return Ok(());
                }
                let Some(Instance::Group(_)) = value else {
                    return Err(self.bad(path, "output group requires a group instance"));
                };
                if id == Some("GS") {
                    self.groups += 1;
                }
                if id == Some("ST") {
                    self.transactions += 1;
                }
                for (index, child) in children(schema).iter().enumerate() {
                    let supplied = value.and_then(|value| value.field(&child.name));
                    if supplied.is_some() {
                        self.budget.name(&child.name, self.limits.names)?;
                    }
                    path.push(child.name.clone());
                    let minimum = self.physical_minimum(id, index);
                    if id.is_some() {
                        self.element(child, supplied, path, depth + 1, minimum, false)?;
                    } else {
                        self.node(child, supplied, path, depth + 1, false, false)?;
                    }
                    path.pop();
                }
                Ok(())
            }
            _ => Err(self.bad(path, "unsupported X12 scalar union")),
        }
    }

    fn bad(&self, path: &[String], reason: &'static str) -> X12BoundaryError {
        X12BoundaryError::Value {
            path: path.join("/"),
            reason,
        }
    }

    fn element(
        &mut self,
        schema: &SchemaNode,
        value: Option<&Instance>,
        path: &mut Vec<String>,
        depth: usize,
        minimum: usize,
        item: bool,
    ) -> Result<(), X12BoundaryError> {
        self.budget.node(depth)?;
        if schema.repeating && !item {
            let items = match value {
                None | Some(Instance::Scalar(Value::Null | Value::JsonNull(_))) => &[][..],
                Some(Instance::Repeated(items) | Instance::MappedSequence(items)) => items,
                _ => return Err(self.bad(path, "repeating element requires a collection")),
            };
            self.budget.loops(items.len())?;
            for item in items {
                self.element(schema, Some(item), path, depth + 1, minimum, true)?;
            }
            return Ok(());
        }
        if matches!(schema.kind, SchemaKind::Scalar { .. }) {
            return self.scalar(schema, value.and_then(Instance::as_scalar), path, minimum);
        }
        // Missing composites still cause native fixed/default materialization of
        // every declared component; unlike absent structural groups they write.
        for child in children(schema) {
            let supplied = value.and_then(|value| value.field(&child.name));
            if supplied.is_some() {
                self.budget.name(&child.name, self.limits.names)?;
            }
            path.push(child.name.clone());
            self.element(child, supplied, path, depth + 1, 0, false)?;
            path.pop();
        }
        Ok(())
    }

    fn physical_minimum(&self, id: Option<&str>, index: usize) -> usize {
        if id == Some("ISA") {
            return [2, 10, 2, 10, 2, 15, 2, 15, 6, 4, 1, 5, 9, 1, 1, 1]
                .get(index)
                .copied()
                .unwrap_or(0);
        }
        if id == Some("IEA") && index == 1 {
            return 9;
        }
        if self.profile.autocomplete.is_none() {
            return 0;
        }
        match (id, index) {
            (Some("GS"), 3) => 8,
            (Some("GS"), 4) => 6,
            (Some("GS"), 5) => self.groups.to_string().len(),
            (Some("ST"), 0) => 3,
            (Some("ST"), 1) => self.transactions.to_string().len().max(4),
            (Some("SE" | "GE" | "IEA"), _) => 9,
            _ => 0,
        }
    }

    fn scalar(
        &mut self,
        schema: &SchemaNode,
        value: Option<&Value>,
        path: &[String],
        minimum: usize,
    ) -> Result<(), X12BoundaryError> {
        if let Some(Value::String(text)) = value {
            charge(
                &mut self.raw,
                text.len(),
                "x12_output_scalar_bytes",
                MAX_X12_DOCUMENT_BYTES,
            )?;
        }
        let missing = value.is_none_or(|value| {
            matches!(value, Value::Null | Value::JsonNull(_))
                || matches!(value, Value::String(text) if text.is_empty())
        });
        // Fixed lexicals are returned verbatim by the native writer, even for
        // numeric fixed values. Reserve them for every actual occurrence.
        let mut length = schema.fixed.as_ref().map_or(0, String::len);
        if !missing {
            let value = value.expect("nonmissing scalar");
            let lexical = self
                .profile
                .lexical
                .iter()
                .find(|format| format.path() == path);
            let formatted;
            let value = if let Some(format) = lexical {
                // Reuse the native lexical primitive on one bounded temporary
                // scalar. Its tiny temporary names/config path are charged too.
                self.budget.name("v", self.limits.names)?;
                self.budget.name("v", self.limits.names)?;
                let mut temporary =
                    Instance::Group(vec![("v".into(), Instance::Scalar(value.clone()))].into());
                let selected = EdiLexicalFormat::new(vec!["v".into()], format.kind())
                    .ok_or_else(|| descriptor("invalid lexical format"))?;
                format_edi::apply_output_lexical_formats(&mut temporary, &[selected]).map_err(
                    |error| match error {
                        format_edi::EdiFormatError::LexicalFormatValue {
                            value, reason, ..
                        } => X12BoundaryError::Native(
                            format_edi::EdiFormatError::LexicalFormatValue {
                                path: path.join("/"),
                                value,
                                reason,
                            },
                        ),
                        other => X12BoundaryError::Native(other),
                    },
                )?;
                formatted = temporary;
                formatted
                    .field("v")
                    .and_then(Instance::as_scalar)
                    .ok_or_else(|| descriptor("native scalar formatting changed shape"))?
            } else {
                value
            };
            let size = match value {
                Value::String(text) => match schema.kind {
                    SchemaKind::Scalar {
                        ty: ir::ScalarType::Int,
                    } => text
                        .trim()
                        .parse::<i64>()
                        .map_or(text.len(), |n| n.to_string().len()),
                    SchemaKind::Scalar {
                        ty: ir::ScalarType::Float,
                    } => text
                        .trim()
                        .parse::<f64>()
                        .map_or(text.len(), |n| n.to_string().len()),
                    _ => text.len(),
                },
                Value::Int(n) => n.to_string().len(),
                Value::Float(n) => n.to_string().len(),
                Value::Bool(n) => n.to_string().len(),
                _ => 0,
            };
            length = length.max(size);
        }
        self.add(length.max(minimum))
    }
}
