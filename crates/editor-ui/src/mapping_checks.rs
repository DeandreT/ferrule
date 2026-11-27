//! Named expectations over decoded CSV records. Hosts supply the complete data
//! rows and the physical header, if present; a schema name is not a CSV header.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedCheck {
    pub name: String,
    pub assertion: Assertion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assertion {
    RowCount {
        expected: usize,
    },
    Columns {
        expected: Vec<String>,
    },
    KeyedCell {
        key: Vec<KeyPart>,
        field: String,
        expected: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPart {
    pub column: String,
    pub value: String,
}

/// The detail is the observed value or the reason there was no single value.
/// Row positions are deliberately not retained with results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub passed: bool,
    pub actual: String,
}

impl Assertion {
    /// Capture a named-cell expectation from the independently selected reference.
    /// A duplicate reference key cannot establish a baseline, even if all of
    /// its values happen to be identical.
    pub fn capture_cell(
        key: Vec<KeyPart>,
        field: String,
        headers: Option<&[String]>,
        rows: &[Vec<String>],
    ) -> Result<Self, String> {
        let (_, value) = resolve_cell(&key, &field, headers, rows)?;
        Ok(Self::KeyedCell {
            key,
            field,
            expected: value.to_owned(),
        })
    }

    pub fn capture_columns(headers: Option<&[String]>) -> Result<Self, String> {
        let headers = physical_header(headers)?;
        validate_columns(headers)?;
        Ok(Self::Columns {
            expected: headers.to_vec(),
        })
    }

    pub fn expected_detail(&self) -> String {
        match self {
            Self::RowCount { expected } => format!("{expected} data rows"),
            Self::Columns { expected } => format!("{expected:?}"),
            Self::KeyedCell {
                key,
                field,
                expected,
            } => {
                let key = key
                    .iter()
                    .map(|part| format!("{} = {:?}", part.column, part.value))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{field} = {expected:?} for exactly one row where {key}")
            }
        }
    }

    pub fn evaluate(&self, headers: Option<&[String]>, rows: &[Vec<String>]) -> CheckResult {
        let (passed, actual) = match self {
            Self::RowCount { expected } => {
                (rows.len() == *expected, format!("{} data rows", rows.len()))
            }
            Self::Columns { expected } => match physical_header(headers) {
                Ok(actual) => match validate_columns(actual) {
                    Ok(()) => (actual == expected, format!("{actual:?}")),
                    Err(reason) => (false, format!("{actual:?} — {reason}")),
                },
                Err(reason) => (false, reason),
            },
            Self::KeyedCell {
                key,
                field,
                expected,
            } => match resolve_cell(key, field, headers, rows) {
                Ok((_, value)) => (
                    value == expected,
                    format!("{field} = {value:?} (1 matching row)"),
                ),
                Err(reason) => (false, reason),
            },
        };
        CheckResult { passed, actual }
    }

    /// Resolve again on activation against the host's current complete selection.
    /// Count/header failures have no specific cell; a repaired value has no
    /// failure to inspect.
    pub fn failed_cell<'a>(
        &'a self,
        headers: Option<&[String]>,
        rows: &[Vec<String>],
    ) -> Result<Option<(usize, &'a str)>, String> {
        let Self::KeyedCell {
            key,
            field,
            expected,
        } = self
        else {
            return Ok(None);
        };
        let (row, value) = resolve_cell(key, field, headers, rows)?;
        Ok((value != expected).then_some((row, field.as_str())))
    }
}

fn physical_header(headers: Option<&[String]>) -> Result<&[String], String> {
    match headers {
        None => Err("Headerless CSV: named columns are unavailable.".into()),
        Some([]) => Err("Missing CSV header: no column names were captured.".into()),
        Some(headers) => Ok(headers),
    }
}

fn validate_columns(headers: &[String]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for (index, name) in headers.iter().enumerate() {
        if name.is_empty() {
            return Err(format!("Column {} has no header name.", index + 1));
        }
        if !names.insert(name) {
            return Err(format!("Duplicate header {name:?}."));
        }
    }
    Ok(())
}

fn unique_column(headers: &[String], field: &str) -> Result<usize, String> {
    if field.is_empty() {
        return Err("A named-column check requires a nonempty header name.".into());
    }
    let mut positions = headers
        .iter()
        .enumerate()
        .filter(|(_, name)| name.as_str() == field);
    let (column, _) = positions
        .next()
        .ok_or_else(|| format!("Missing column {field:?}."))?;
    if positions.next().is_some() {
        return Err(format!("Ambiguous column {field:?}: duplicate headers."));
    }
    Ok(column)
}

fn resolve_cell<'a>(
    key: &[KeyPart],
    field: &str,
    headers: Option<&[String]>,
    rows: &'a [Vec<String>],
) -> Result<(usize, &'a str), String> {
    let headers = physical_header(headers)?;
    if key.is_empty() {
        return Err("Choose at least one key column.".into());
    }
    let field_index = unique_column(headers, field)?;
    let mut columns = Vec::with_capacity(key.len());
    for part in key {
        let index = unique_column(headers, &part.column)?;
        if columns.contains(&index) {
            return Err(format!(
                "Key column {:?} was selected more than once.",
                part.column
            ));
        }
        columns.push(index);
    }
    let mut matches = rows.iter().enumerate().filter(|(_, row)| {
        columns
            .iter()
            .zip(key)
            .all(|(&index, part)| row.get(index) == Some(&part.value))
    });
    let (index, row) = matches.next().ok_or("Missing key: 0 matching rows.")?;
    let extra = matches.count();
    if extra != 0 {
        return Err(format!("Ambiguous key: {} matching rows.", extra + 1));
    }
    let value = row.get(field_index).ok_or_else(|| {
        format!(
            "Data row {} matches the key, but has no cell in {field:?}.",
            index + 1
        )
    })?;
    Ok((index, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().copied().map(String::from).collect()
    }

    fn key() -> Vec<KeyPart> {
        vec![
            KeyPart {
                column: "order".into(),
                value: "A".into(),
            },
            KeyPart {
                column: "item".into(),
                value: "1".into(),
            },
        ]
    }

    #[test]
    fn reordering_and_unchecked_changes_pass_but_the_changed_cell_alone_fails() {
        let names = strings(&["order", "item", "description", "price"]);
        let header = Some(names.as_slice());
        let mut rows = vec![
            strings(&["A", "1", "BRUSH", "5"]),
            strings(&["A", "2", "BIT", "4"]),
        ];
        let checks = [
            Assertion::RowCount {
                expected: rows.len(),
            },
            Assertion::capture_columns(header).unwrap(),
            Assertion::capture_cell(key(), "description".into(), header, &rows).unwrap(),
        ];
        rows.reverse();
        rows[1][3] = "99".into();
        assert!(
            checks
                .iter()
                .all(|check| check.evaluate(header, &rows).passed)
        );
        rows[1][2] = "brush".into();
        assert_eq!(
            checks
                .each_ref()
                .map(|check| check.evaluate(header, &rows).passed),
            [true, true, false]
        );
        assert_eq!(
            checks[2].failed_cell(header, &rows).unwrap(),
            Some((1, "description"))
        );
        rows.reverse();
        assert_eq!(
            checks[2].failed_cell(header, &rows).unwrap(),
            Some((0, "description"))
        );
        rows[0][2] = "BRUSH".into();
        assert_eq!(checks[2].failed_cell(header, &rows).unwrap(), None);
    }

    #[test]
    fn membership_is_exact_and_missing_or_duplicate_keys_never_supply_a_row() {
        let names = strings(&["order", "item", "text"]);
        let header = Some(names.as_slice());
        let row = strings(&["A", "1", "first\nsecond"]);
        let check =
            Assertion::capture_cell(key(), "text".into(), header, std::slice::from_ref(&row))
                .unwrap();
        let duplicate = vec![row.clone(), strings(&["A", "01", "else"]), row];
        let reason = "Ambiguous key: 2 matching rows.";
        assert_eq!(
            Assertion::capture_cell(key(), "text".into(), header, &duplicate).unwrap_err(),
            reason
        );
        assert_eq!(check.failed_cell(header, &duplicate).unwrap_err(), reason);
        assert_eq!(check.evaluate(header, &duplicate).actual, reason);
        assert_eq!(
            check.evaluate(header, &duplicate[1..2]).actual,
            "Missing key: 0 matching rows."
        );
        assert!(check.failed_cell(header, &[]).is_err());
    }

    #[test]
    fn headerless_empty_missing_duplicate_and_ragged_cells_are_distinct() {
        let row_count = Assertion::RowCount { expected: 0 };
        assert!(row_count.evaluate(None, &[]).passed);
        let names = strings(&["order", "item", "text"]);
        let headers = Assertion::capture_columns(Some(&names)).unwrap();
        assert!(headers.evaluate(None, &[]).actual.contains("Headerless"));
        assert!(headers.evaluate(Some(&[]), &[]).actual.contains("Missing"));
        let dup = strings(&["order", "text", "text"]);
        assert!(
            headers
                .evaluate(Some(&dup), &[])
                .actual
                .contains("Duplicate")
        );
        let changed = strings(&["item", "order", "text"]);
        assert!(!headers.evaluate(Some(&changed), &[]).passed);
        let ragged = vec![strings(&["A", "1"])];
        let capture = |names: &[String]| {
            Assertion::capture_cell(key(), "text".into(), Some(names), &ragged).unwrap_err()
        };
        assert!(capture(&dup).contains("Ambiguous column"));
        assert!(capture(&strings(&["order", "text"])).contains("Missing column"));
        assert!(capture(&names).contains("no cell"));
        assert!(capture(&strings(&["order", "order", "text"])).contains("Ambiguous column"));
        let empty = vec![strings(&["A", "1", ""])];
        assert!(Assertion::capture_cell(key(), "text".into(), Some(&names), &empty).is_ok());
    }
}
