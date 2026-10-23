use super::*;

/// Every field samples independently, even when its supplied value is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapForceX12ClockField {
    InterchangeDate,
    InterchangeTime,
    GroupDate,
    GroupTime,
}

pub(super) struct Defaults {
    segment: &'static str,
    date: Option<String>,
    time: Option<String>,
}

pub(super) fn sample(
    schema: SchemaCursor<'_>,
    value: &Instance,
    clock: &mut dyn FnMut(MapForceX12ClockField) -> Result<String, String>,
) -> Result<Vec<Defaults>, X12BoundaryError> {
    fn datetime(
        clock: &mut dyn FnMut(MapForceX12ClockField) -> Result<String, String>,
        field: MapForceX12ClockField,
    ) -> Result<String, X12BoundaryError> {
        let text =
            clock(field).map_err(|message| X12BoundaryError::CompletionClock { field, message })?;
        if text.len() > MAX_X12_DOCUMENT_BYTES {
            return Err(X12BoundaryError::CompletionClock {
                field,
                message: "clock exceeds the text limit".into(),
            });
        }
        functions::call("time_from_datetime", &[Value::String(text.clone())]).map_err(|e| {
            X12BoundaryError::CompletionClock {
                field,
                message: e.to_string(),
            }
        })?;
        Ok(text)
    }
    fn visit(
        schema: SchemaCursor<'_>,
        value: &Instance,
        clock: &mut dyn FnMut(MapForceX12ClockField) -> Result<String, String>,
        result: &mut Vec<Defaults>,
    ) -> Result<(), X12BoundaryError> {
        if let Instance::Repeated(items) | Instance::MappedSequence(items) = value {
            for item in items {
                visit(schema, item, clock, result)?;
            }
            return Ok(());
        }
        if let Some(id @ ("ISA" | "GS")) = schema.segment_id() {
            if result.len() >= MAX_X12_SEGMENTS {
                return Err(descriptor("too many clock-owned segments"));
            }
            let (date_index, time_index, date_field, time_field) = if id == "ISA" {
                (
                    8,
                    9,
                    MapForceX12ClockField::InterchangeDate,
                    MapForceX12ClockField::InterchangeTime,
                )
            } else {
                (
                    3,
                    4,
                    MapForceX12ClockField::GroupDate,
                    MapForceX12ClockField::GroupTime,
                )
            };
            let children = schema.children().collect::<Vec<_>>();
            let empty = |index: usize| {
                children
                    .get(index)
                    .and_then(|c| value.field(&c.schema().name))
                    .is_none_or(|v| {
                        matches!(v, Instance::Scalar(Value::Null | Value::JsonNull(_)))
                            || matches!(v,Instance::Scalar(Value::String(s)) if s.is_empty())
                    })
            };
            let date = datetime(clock, date_field)?;
            let time = datetime(clock, time_field)?;
            let date = date
                .split_once('T')
                .ok_or_else(|| descriptor("clock requires a dateTime"))?
                .0
                .replace('-', "");
            if date.len() != 8 {
                return Err(descriptor("clock requires a four-digit year"));
            }
            let time = time
                .split_once('T')
                .ok_or_else(|| descriptor("clock requires a dateTime"))?
                .1;
            let time = format!("{}{}", &time[..2], &time[3..5]);
            result.push(Defaults {
                segment: if id == "ISA" { "ISA" } else { "GS" },
                date: empty(date_index).then(|| if id == "ISA" { date[2..].into() } else { date }),
                time: empty(time_index).then(|| {
                    if id == "GS" {
                        format!("{time}00")
                    } else {
                        time
                    }
                }),
            });
            return Ok(());
        }
        for child in schema.children() {
            if let Some(value) = value.field(&child.schema().name) {
                visit(child, value, clock, result)?;
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(schema, value, clock, &mut result)?;
    Ok(result)
}

pub(super) fn apply(
    schema: SchemaCursor<'_>,
    value: &Instance,
    defaults: &[Defaults],
) -> Result<Instance, X12BoundaryError> {
    fn visit(
        schema: SchemaCursor<'_>,
        value: &Instance,
        defaults: &[Defaults],
        index: &mut usize,
    ) -> Result<Instance, X12BoundaryError> {
        match value {
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                let items = items
                    .iter()
                    .map(|v| visit(schema, v, defaults, index))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(if matches!(value, Instance::Repeated(_)) {
                    Instance::Repeated(items)
                } else {
                    Instance::MappedSequence(items)
                })
            }
            Instance::Group(fields) => {
                let mut fields = fields
                    .iter()
                    .map(|(n, v)| (n.clone(), v.clone()))
                    .collect::<Vec<_>>();
                if let Some(id @ ("ISA" | "GS")) = schema.segment_id() {
                    let row = defaults
                        .get(*index)
                        .filter(|d| d.segment == id)
                        .ok_or_else(|| descriptor("clock segment ownership differs"))?;
                    *index += 1;
                    let children = schema.children().collect::<Vec<_>>();
                    for (offset, value) in [
                        (if id == "ISA" { 8 } else { 3 }, &row.date),
                        (if id == "ISA" { 9 } else { 4 }, &row.time),
                    ] {
                        if let Some(value) = value {
                            let name = &children
                                .get(offset)
                                .ok_or_else(|| descriptor("missing clock field"))?
                                .schema()
                                .name;
                            let field = fields
                                .iter_mut()
                                .find(|(n, _)| n == name)
                                .ok_or_else(|| descriptor("missing materialized clock field"))?;
                            field.1 = Instance::Scalar(Value::String(value.clone()));
                        }
                    }
                } else {
                    for child in schema.children() {
                        if let Some((_, field)) =
                            fields.iter_mut().find(|(n, _)| n == &child.schema().name)
                        {
                            *field = visit(child, field, defaults, index)?;
                        }
                    }
                }
                Ok(Instance::Group(fields.into()))
            }
            _ => Ok(value.clone()),
        }
    }
    let mut index = 0;
    let result = visit(schema, value, defaults, &mut index)?;
    if index != defaults.len() {
        return Err(descriptor("unused clock defaults"));
    }
    Ok(result)
}
