//! Explicit MapForce-compatible X12 output; strict X12 remains unchanged.
use super::*;
use mapping::{EdiLexicalKind, MapForceEdiScalar, MapForceEdiScalarKind};

pub fn serialize_mapforce_x12_document(
    descriptor_text: &str,
    document: &Instance,
    execution: &ExecutionContext<'_>,
) -> Result<Vec<u8>, X12BoundaryError> {
    if descriptor_text.len() > 2 * MAX_EMBEDDED_X12_SCHEMA_BYTES {
        return Err(descriptor("MapForce writer descriptor too large"));
    }
    let wrapper: mapping::MapForceX12WriterDescriptor =
        serde_json::from_str(descriptor_text).map_err(|e| descriptor(e.to_string()))?;
    let profile = Profile::parse(&wrapper.boundary)?;
    if profile.envelope != EnvelopeProfile::MapForce {
        return Err(descriptor("requires MapForce interchange ownership"));
    }
    let minimums = wrapper.minimums;
    if minimums.len() > 10000 {
        return Err(descriptor("too many writer minimums"));
    }
    let mut minimum = BTreeMap::new();
    for row in minimums {
        let mut node = &profile.schema;
        for part in &row.path {
            node = node
                .child(part)
                .ok_or_else(|| descriptor("unknown minimum path"))?;
        }
        if row.minimum > 1024
            || row.path.len() > 64
            || !matches!(node.kind, SchemaKind::Group { .. })
            || minimum.insert(row.path, row.minimum as usize).is_some()
        {
            return Err(descriptor("invalid writer minimum"));
        }
    }
    let syntax = wire::Syntax::output(&profile).native();
    if syntax.segment == '\n' || syntax.release.is_some() {
        return Err(descriptor("unqualified MapForce output syntax"));
    }
    let mut budget = WriterBudget::default();
    let view = adapt(
        SchemaCursor::root(&profile.schema),
        Some(document),
        &profile,
        &minimum,
        &mut vec![],
        false,
        &mut budget,
        0,
        None,
    )?
    .ok_or_else(|| descriptor("missing output root"))?;
    let mut schema = profile.schema.clone();
    string_schema(&mut schema);
    let clock = execution.value(RuntimeValue::CurrentDateTime);
    let mut automatic = Vec::new();
    collect_group_times(SchemaCursor::root(&profile.schema), &view, &mut automatic);
    let completion = match (&profile.autocomplete, &clock) {
        (Some(completion), Some(Value::String(clock))) => Some(format_edi::x12::Autocomplete {
            current_datetime: clock,
            request_acknowledgement: completion.request_acknowledgement,
            transaction_set: completion.transaction_set.as_deref(),
        }),
        (None, _) => None,
        _ => return Err(descriptor("completion requires a clock value")),
    };
    let text = format_edi::x12::to_mapforce_document_string_with_syntax(
        &schema,
        &view,
        syntax,
        Some(profile.version),
        completion,
    )?;
    let mut flags = automatic.into_iter();
    let separator = format!("{}\n", syntax.segment);
    let mut parts = Vec::new();
    for part in text.split(&separator) {
        if part.starts_with("GS") && part.chars().nth(2) == Some(syntax.element) {
            let automatic = flags
                .next()
                .ok_or_else(|| descriptor("unexpected completed GS"))?;
            if automatic && profile.autocomplete.is_some() {
                let mut fields = part
                    .split(syntax.element)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if fields.len() < 6 {
                    return Err(descriptor("incomplete GS output"));
                }
                fields[5].truncate(4);
                parts.push(fields.join(&syntax.element.to_string()));
                continue;
            }
        }
        parts.push(part.to_string());
    }
    if flags.next().is_some() {
        return Err(descriptor("missing completed GS"));
    }
    let text = parts.join(&separator);
    let text = text.replace(
        &format!("{}\n", syntax.segment),
        &format!("{}\r\n", syntax.segment),
    );
    output_size(text.len())?;
    Ok(text
        .encode_utf16()
        .map(super::latin1_best_fit::unit)
        .collect())
}

fn collect_group_times(node: SchemaCursor<'_>, value: &Instance, flags: &mut Vec<bool>) {
    match value {
        Instance::Repeated(items) => {
            for item in items {
                collect_group_times(node, item, flags)
            }
        }
        Instance::Group(fields) => {
            if node.segment_id() == Some("GS")
                && let Some(field) = node.children().nth(4)
            {
                flags.push(fields.iter().find(|(name,_)|*name==field.schema().name).is_none_or(|(_,value)|matches!(value,Instance::Scalar(Value::Null)) || matches!(value,Instance::Scalar(Value::String(s)) if s.is_empty())));
            }
            for child in node.children() {
                if let Some((_, value)) =
                    fields.iter().find(|(name, _)| *name == child.schema().name)
                {
                    collect_group_times(child, value, flags);
                }
            }
        }
        _ => {}
    }
}

fn string_schema(node: &mut SchemaNode) {
    match &mut node.kind {
        SchemaKind::Scalar { ty } => *ty = ir::ScalarType::String,
        SchemaKind::Group { children, .. } => {
            for child in children {
                string_schema(child)
            }
        }
        SchemaKind::ScalarUnion { .. } => {}
    }
}

#[derive(Default)]
struct WriterBudget {
    inner: Budget,
    scalar_bytes: usize,
}
impl WriterBudget {
    fn node(&mut self, depth: usize) -> Result<(), X12BoundaryError> {
        self.inner.node(depth)
    }
    fn loops(&mut self, count: usize) -> Result<(), X12BoundaryError> {
        self.inner.loops(count)
    }
    fn text(&mut self, bytes: usize) -> Result<(), X12BoundaryError> {
        self.scalar_bytes = self
            .scalar_bytes
            .checked_add(bytes)
            .ok_or_else(|| descriptor("writer scalar byte count overflow"))?;
        output_size(self.scalar_bytes)
    }
    fn name(&mut self, name: &str) -> Result<(), X12BoundaryError> {
        self.inner.name(name, MAX_X12_MATERIALIZED_NAME_BYTES)
    }
}

#[allow(clippy::too_many_arguments)]
fn adapt(
    cursor: SchemaCursor<'_>,
    input: Option<&Instance>,
    profile: &Profile,
    minimums: &BTreeMap<Vec<String>, usize>,
    path: &mut Vec<String>,
    item: bool,
    budget: &mut WriterBudget,
    depth: usize,
    allowed: Option<char>,
) -> Result<Option<Instance>, X12BoundaryError> {
    budget.node(depth)?;
    let node = cursor.schema();
    let minimum = minimums.get(path).copied().unwrap_or(0);
    let failure = || X12BoundaryError::Value {
        path: path.join("/"),
        reason: "MapForce writer input shape differs",
    };
    if node.repeating && !item {
        let empty = [];
        let items = match input {
            None => &empty[..],
            Some(Instance::Repeated(items) | Instance::MappedSequence(items)) => items.as_slice(),
            _ => return Err(failure()),
        };
        if path.len() == 1 && items.is_empty() {
            return Err(failure());
        }
        let count = items.len().max(minimum);
        budget.loops(count)?;
        let absent = Instance::Group(vec![].into());
        let mut output = Vec::with_capacity(count);
        for index in 0..count {
            if let Some(value) = adapt(
                cursor,
                Some(items.get(index).unwrap_or(&absent)),
                profile,
                minimums,
                path,
                true,
                budget,
                depth + 1,
                allowed,
            )? {
                output.push(value);
            }
        }
        return Ok(Some(Instance::Repeated(output)));
    }
    if let SchemaKind::Scalar { ty } = node.kind {
        let fixed = node.fixed.as_ref().map(|s| Value::String(s.clone()));
        let missing = Value::Null;
        let raw = match input {
            Some(Instance::Scalar(value)) => value,
            None => fixed.as_ref().unwrap_or(&missing),
            _ => return Err(failure()),
        };
        if matches!(raw, Value::Null | Value::JsonNull(_))
            || matches!(raw,Value::String(s) if s.is_empty())
        {
            return Ok(Some(Instance::Scalar(Value::String(String::new()))));
        }
        let constraint = profile.constraints.get(path);
        let min = constraint.map_or(0, |c| c.min_chars());
        let max = constraint.map_or(65536, |c| c.max_chars());
        let lexical = profile
            .lexical
            .iter()
            .find(|f| f.path() == path.as_slice())
            .map(|f| f.kind());
        let kind = match lexical {
            Some(EdiLexicalKind::CompactDate6 | EdiLexicalKind::CompactDate8) => {
                MapForceEdiScalarKind::Date
            }
            Some(EdiLexicalKind::CompactTime { .. }) => MapForceEdiScalarKind::Time,
            Some(EdiLexicalKind::Decimal { .. }) => MapForceEdiScalarKind::Decimal,
            None if matches!(ty, ir::ScalarType::Float | ir::ScalarType::Int) => {
                MapForceEdiScalarKind::Decimal
            }
            _ => MapForceEdiScalarKind::Text,
        };
        let text = crate::format_mapforce_edi_scalar(
            MapForceEdiScalar {
                kind,
                min_chars: min,
                max_chars: max,
            },
            raw,
        )
        .map_err(|source| X12BoundaryError::MapForceScalar {
            path: path.join("/"),
            source,
        })?;
        budget.text(text.len())?;
        let syntax = wire::Syntax::output(profile).native();
        let text = if text.chars().count() == 1 && text.chars().next() == allowed {
            text
        } else {
            text.chars()
                .map(|c| {
                    if c == syntax.element
                        || c == syntax.component
                        || c == syntax.segment
                        || Some(c) == syntax.repetition
                    {
                        ' '
                    } else {
                        c
                    }
                })
                .collect()
        };
        return Ok(Some(Instance::Scalar(Value::String(text))));
    }
    let absent = Instance::Group(vec![].into());
    let value = match input {
        None if minimum == 0 && !path.is_empty() => return Ok(None),
        None => &absent,
        Some(value) => value,
    };
    let Instance::Group(fields) = value else {
        return Err(failure());
    };
    let mut seen = std::collections::BTreeSet::new();
    for (name, _) in fields.iter() {
        if node.child(name).is_none() || !seen.insert(name) {
            return Err(failure());
        }
    }
    let mut output = Vec::new();
    let syntax = wire::Syntax::output(profile).native();
    for (index, child) in cursor.children().enumerate() {
        budget.name(&child.schema().name)?;
        let name = child.schema().name.clone();
        path.push(name.clone());
        let allowed = if cursor.segment_id() == Some("ISA") {
            match index {
                10 => syntax.repetition,
                15 => Some(syntax.component),
                _ => None,
            }
        } else {
            None
        };
        if let Some(value) = adapt(
            child,
            value.field(&name),
            profile,
            minimums,
            path,
            false,
            budget,
            depth + 1,
            allowed,
        )? {
            output.push((name, value));
        }
        path.pop();
    }
    Ok(Some(Instance::Group(output.into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fields(name: &str, count: usize) -> SchemaNode {
        SchemaNode::group(
            name,
            (1..=count)
                .map(|i| SchemaNode::scalar(format!("E{i:02}"), ir::ScalarType::String))
                .collect(),
        )
    }
    fn group(fields: Vec<(&str, Instance)>) -> Instance {
        Instance::Group(
            fields
                .into_iter()
                .map(|(n, v)| (n.into(), v))
                .collect::<Vec<_>>()
                .into(),
        )
    }
    fn repeated(value: Instance) -> Instance {
        Instance::Repeated(vec![value])
    }
    fn scalar(value: &str) -> Instance {
        Instance::Scalar(Value::String(value.into()))
    }
    fn descriptor_text() -> String {
        let schema = SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchange",
                    vec![
                        fields("ISA", 16).repeating(),
                        SchemaNode::group(
                            "Group",
                            vec![
                                fields("GS", 8).repeating(),
                                SchemaNode::group(
                                    "Message_850",
                                    vec![
                                        fields("ST", 2),
                                        SchemaNode::group("Rows", vec![fields("LIN", 1)])
                                            .repeating(),
                                        fields("SE", 2),
                                    ],
                                )
                                .repeating(),
                                fields("GE", 2).repeating(),
                            ],
                        )
                        .repeating(),
                        fields("IEA", 2).repeating(),
                    ],
                )
                .repeating(),
            ],
        );
        let boundary = serde_json::json!({"version":"006040","schema":codegen_schema::encode(&schema,MAX_EMBEDDED_X12_SCHEMA_BYTES).unwrap(),"envelope_profile":"mapforce_interchanges","separators":{"element":"*","component":":","segment":"~","repetition":"^"},"constraints":[],"lexical_formats":[],"implied_decimals":[],"autocomplete":{"request_acknowledgement":true,"transaction_set":"850"},"lenient_segments":true});
        serde_json::json!({"boundary":boundary.to_string(),"minimums":[{"path":["Interchange","Group","Message_850","Rows"],"minimum":1},{"path":["Interchange","Group","Message_850","Rows","LIN"],"minimum":1}]}).to_string()
    }
    #[test]
    fn required_empty_line_and_short_automatic_group_time_are_explicit() {
        let isa = group(vec![
            ("E05", scalar("ZZ")),
            ("E06", scalar("ALPHA")),
            ("E07", scalar("ZZ")),
            ("E08", scalar("BETA")),
            ("E13", scalar("42")),
            ("E15", scalar("T")),
        ]);
        let gs = group(vec![
            ("E01", scalar("PO")),
            ("E02", scalar("ALPHA")),
            ("E03", scalar("BETA")),
            ("E06", scalar("42")),
            ("E07", scalar("X")),
            ("E08", scalar("004010")),
        ]);
        let message = group(vec![
            (
                "ST",
                group(vec![("E01", scalar("850")), ("E02", scalar("0001"))]),
            ),
            ("Rows", Instance::Repeated(vec![])),
        ]);
        let body = group(vec![(
            "Interchange",
            repeated(group(vec![
                ("ISA", repeated(isa)),
                (
                    "Group",
                    repeated(group(vec![
                        ("GS", repeated(gs)),
                        ("Message_850", repeated(message)),
                    ])),
                ),
            ])),
        )]);
        let execution = ExecutionContext::new(std::path::Path::new("mapping.mfd"))
            .with_current_datetime("2030-06-15T09:45:23.123Z");
        let bytes = serialize_mapforce_x12_document(&descriptor_text(), &body, &execution).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("GS*PO*ALPHA*BETA*20300615*0945*42*X*004010~\r\n"));
        assert!(text.contains("LIN~\r\nSE*3*0001~\r\n"));
        assert!(
            serialize_mapforce_x12_document(
                &descriptor_text(),
                &group(vec![("Interchange", Instance::Repeated(vec![]))]),
                &execution
            )
            .is_err()
        );
    }
}
