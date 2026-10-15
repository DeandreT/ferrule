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
    segments: usize,
    transaction: Option<(usize, usize)>,
    group: Option<(usize, usize)>,
    interchange_groups: usize,
    created_scalar_bytes: usize,
    created_nodes: usize,
    created_names: usize,
}

pub(in super::super) fn preflight(
    profile: &Profile,
    document: &Instance,
    limits: OutputLimits,
) -> Result<(), X12BoundaryError> {
    reserve(profile, document, limits).map(|_| ())
}

fn reserve<'a>(
    profile: &'a Profile,
    document: &Instance,
    limits: OutputLimits,
) -> Result<Reservation<'a>, X12BoundaryError> {
    let mut reservation = Reservation {
        profile,
        limits,
        budget: Budget::default(),
        projected: 0,
        raw: 0,
        groups: 0,
        transactions: 0,
        segments: 0,
        transaction: None,
        group: None,
        interchange_groups: 0,
        created_scalar_bytes: 0,
        created_nodes: 0,
        created_names: 0,
    };
    reservation.node(
        SchemaCursor::root(&profile.schema),
        Some(document),
        &mut Vec::new(),
        0,
        false,
    )?;
    Ok(reservation)
}

#[cfg(test)]
pub(in super::super) fn completion_stats(
    profile: &Profile,
    document: &Instance,
    limits: OutputLimits,
) -> Result<(usize, usize, usize, usize), X12BoundaryError> {
    let reservation = reserve(profile, document, limits)?;
    // Native completion owns short segment IDs, not schema field-name clones.
    Ok((
        reservation.projected,
        reservation.created_scalar_bytes,
        reservation.created_nodes,
        reservation.created_names,
    ))
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
        node: SchemaCursor<'_>,
        value: Option<&Instance>,
        path: &mut Vec<String>,
        depth: usize,
        item: bool,
    ) -> Result<(), X12BoundaryError> {
        let schema = node.schema();
        self.budget.node(depth)?;
        if schema.repeating && node.role() != SchemaRole::Root && !item {
            let items = match value {
                None => &[][..],
                Some(Instance::Scalar(Value::Null | Value::JsonNull(_)))
                    if matches!(
                        node.role(),
                        SchemaRole::Element { .. } | SchemaRole::Component
                    ) =>
                {
                    &[][..]
                }
                Some(Instance::Repeated(items) | Instance::MappedSequence(items)) => items,
                _ => return Err(self.bad(path, "repeating output requires a collection")),
            };
            self.budget.loops(items.len())?;
            if items.is_empty()
                && self.profile.autocomplete.is_some()
                && matches!(node.segment_id(), Some("SE" | "GE" | "IEA"))
            {
                return self.created_trailer(node, path, depth);
            }
            for value in items {
                self.node(node, Some(value), path, depth + 1, true)?;
            }
            return Ok(());
        }
        match &schema.kind {
            SchemaKind::Scalar { .. } => {
                let minimum = self.physical_minimum(node.role());
                let length =
                    self.scalar(schema, value.and_then(Instance::as_scalar), path, minimum)?;
                match node.role() {
                    SchemaRole::Element {
                        segment: "ST",
                        index: 1,
                    } => {
                        if let Some(owner) = self.transaction.as_mut() {
                            owner.1 = length;
                        }
                    }
                    SchemaRole::Element {
                        segment: "GS",
                        index: 5,
                    } => {
                        if let Some(owner) = self.group.as_mut() {
                            owner.1 = length;
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            SchemaKind::Group { .. } => {
                let id = node.segment_id();
                if value.is_none() {
                    if self.profile.autocomplete.is_some()
                        && matches!(id, Some("SE" | "GE" | "IEA"))
                    {
                        return self.created_trailer(node, path, depth);
                    }
                    if matches!(
                        node.role(),
                        SchemaRole::Root | SchemaRole::Container | SchemaRole::Segment(_)
                    ) {
                        return Ok(());
                    }
                }
                if value.is_some_and(|value| !matches!(value, Instance::Group(_))) {
                    return Err(self.bad(path, "output group requires a group instance"));
                }
                if id == Some("GS") {
                    self.groups += 1;
                }
                if id == Some("ST") {
                    self.transactions += 1;
                }
                if let Some(id) = id {
                    self.begin_segment(id);
                }
                for child_node in node.children() {
                    let child = child_node.schema();
                    let supplied = value.and_then(|value| value.field(&child.name));
                    if supplied.is_some() {
                        self.budget.name(&child.name, self.limits.names)?;
                    }
                    path.push(child.name.clone());
                    self.node(child_node, supplied, path, depth + 1, false)?;
                    path.pop();
                }
                if let Some(id) = id {
                    self.finish_segment(id);
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

    fn physical_minimum(&self, role: SchemaRole<'_>) -> usize {
        let SchemaRole::Element { segment, index } = role else {
            return 0;
        };
        let id = Some(segment);
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
    ) -> Result<usize, X12BoundaryError> {
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
        let length = length.max(minimum);
        self.add(length)?;
        Ok(length)
    }

    fn begin_segment(&mut self, id: &str) {
        match id {
            "ISA" => self.interchange_groups = 0,
            "GS" => self.group = Some((0, self.groups.to_string().len())),
            "ST" => {
                self.transaction = Some((self.segments, self.transactions.to_string().len().max(4)))
            }
            _ => {}
        }
        self.segments += 1;
    }

    fn finish_segment(&mut self, id: &str) {
        match id {
            "SE" => {
                self.transaction = None;
                if let Some(group) = self.group.as_mut() {
                    group.0 += 1;
                }
            }
            "GE" => {
                self.group = None;
                self.interchange_groups += 1;
            }
            _ => {}
        }
    }

    fn created_trailer(
        &mut self,
        node: SchemaCursor<'_>,
        path: &[String],
        depth: usize,
    ) -> Result<(), X12BoundaryError> {
        let id = node
            .segment_id()
            .ok_or_else(|| self.bad(path, "completion requires a physical trailer"))?;
        let (count, control) = match id {
            "SE" => {
                let (start, control) = self
                    .transaction
                    .ok_or_else(|| self.bad(path, "completion trailer has no transaction owner"))?;
                let count = self
                    .segments
                    .checked_sub(start)
                    .and_then(|count| count.checked_add(1))
                    .ok_or_else(|| self.bad(path, "completion count overflow"))?;
                (count, control)
            }
            "GE" => self
                .group
                .ok_or_else(|| self.bad(path, "completion trailer has no group owner"))?,
            "IEA" => (self.interchange_groups, 9),
            _ => return Err(self.bad(path, "unsupported completion trailer")),
        };
        // Native completion creates a wire Segment with two scalar slots, not
        // a schema-shaped Instance. Reserve exactly its ID name and text.
        self.budget.name(id, self.limits.names)?;
        self.budget.node(depth)?;
        self.budget.node(depth + 1)?;
        self.budget.node(depth + 1)?;
        charge(
            &mut self.created_names,
            id.len(),
            "x12_materialized_name_bytes",
            self.limits.names,
        )?;
        charge(&mut self.created_nodes, 3, "x12_nodes", MAX_X12_NODES)?;
        let bytes = count
            .to_string()
            .len()
            .checked_add(control)
            .ok_or_else(|| self.bad(path, "completion text overflow"))?;
        charge(
            &mut self.created_scalar_bytes,
            bytes,
            "x12_projected_scalar_bytes",
            self.limits.projected,
        )?;
        self.add(bytes)?;
        self.begin_segment(id);
        self.finish_segment(id);
        Ok(())
    }
}
