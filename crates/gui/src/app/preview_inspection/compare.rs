use std::collections::{BTreeMap, BTreeSet};

use crate::preview_table::PreviewTable;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct MatchBy {
    pub keys: Vec<String>,
    pub position: bool,
}

/// Indices into the two immutable snapshots, never a copy of their cells.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Change {
    pub a: Option<usize>,
    pub b: Option<usize>,
    pub field: Option<usize>,
}

#[derive(Debug)]
pub(super) struct Differences {
    pub fields: Vec<(Option<usize>, Option<usize>)>,
    pub changes: Vec<Change>,
    pub matched: usize,
    pub added: usize,
    pub removed: usize,
}

pub(super) fn compare(
    a: &PreviewTable,
    b: &PreviewTable,
    mode: &MatchBy,
) -> Result<Differences, String> {
    let pairs = if mode.position {
        (0..a.rows.len().max(b.rows.len()))
            .map(|row| {
                (
                    (row < a.rows.len()).then_some(row),
                    (row < b.rows.len()).then_some(row),
                )
            })
            .collect()
    } else {
        if mode.keys.is_empty() {
            return Err(
                "Choose one or more key fields, or explicitly compare by row position.".into(),
            );
        }
        let a_keys = unique_keys(a, &mode.keys, "A")?;
        let b_keys = unique_keys(b, &mode.keys, "B")?;
        // Baseline order is for presentation only. Never zip rows in keyed mode.
        let mut pairs = vec![(None, None); a.rows.len()];
        for (key, &row) in &a_keys {
            pairs[row] = (Some(row), b_keys.get(key).copied());
        }
        let mut added: Vec<_> = b_keys
            .iter()
            .filter(|(key, _)| !a_keys.contains_key(*key))
            .map(|(_, row)| (None, Some(*row)))
            .collect();
        added.sort_unstable();
        pairs.extend(added);
        pairs
    };
    let mut fields = Vec::new();
    let mut seen = BTreeSet::new();
    for name in a.headers.iter().chain(&b.headers) {
        if seen.insert(name) {
            let resolve = |table: &PreviewTable| {
                if table.headers.contains(name) {
                    table.column(name).map(Some)
                } else {
                    Ok(None)
                }
            };
            fields.push((resolve(a)?, resolve(b)?));
        }
    }
    let mut diff = Differences {
        fields,
        changes: Vec::new(),
        matched: 0,
        added: 0,
        removed: 0,
    };
    for (left, right) in pairs {
        if let (Some(ar), Some(br)) = (left, right) {
            diff.matched += 1;
            for (field, &(ac, bc)) in diff.fields.iter().enumerate() {
                if ac.map(|col| &a.rows[ar][col]) != bc.map(|col| &b.rows[br][col]) {
                    diff.changes.push(Change {
                        a: left,
                        b: right,
                        field: Some(field),
                    });
                }
            }
        } else {
            diff.added += usize::from(left.is_none());
            diff.removed += usize::from(right.is_none());
            diff.changes.push(Change {
                a: left,
                b: right,
                field: None,
            });
        }
    }
    Ok(diff)
}

fn unique_keys<'a>(
    table: &'a PreviewTable,
    names: &[String],
    side: &str,
) -> Result<BTreeMap<Vec<&'a str>, usize>, String> {
    let columns = names
        .iter()
        .map(|name| table.column(name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut index = BTreeMap::new();
    for (row, values) in table.rows.iter().enumerate() {
        let key: Vec<_> = columns.iter().map(|&col| values[col].as_str()).collect();
        if key.iter().any(|value| value.is_empty()) {
            return Err(format!(
                "Missing key in {side}, data row {}. Choose another key or row position.",
                row + 1
            ));
        }
        if let Some(previous) = index.insert(key, row) {
            return Err(format!(
                "Ambiguous key in {side}, data rows {} and {}. Choose another key or row position.",
                previous + 1,
                row + 1
            ));
        }
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(input: &str) -> PreviewTable {
        PreviewTable::parse(
            input.as_bytes(),
            &Default::default(),
            &ir::SchemaNode::group("row", vec![]),
        )
        .unwrap()
    }

    #[test]
    fn compound_matches_survive_reordering_and_include_added_removed_rows_and_columns() {
        let a = table("g,i,value,old\na,1,hot,X\na,2,warm,Y\nb,1,cold,Z\n");
        let b = table("g,i,value,new\na,2,warm,2\nc,1,fresh,1\na,1,cold,3\n");
        let mode = MatchBy {
            keys: vec!["g".into(), "i".into()],
            position: false,
        };
        let diff = compare(&a, &b, &mode).unwrap();
        assert_eq!((diff.matched, diff.added, diff.removed), (2, 1, 1));
        assert_eq!(diff.changes.len(), 7);
        assert_eq!(
            diff.changes[0],
            Change {
                a: Some(0),
                b: Some(2),
                field: Some(2)
            }
        );
        assert!(compare(&a, &a, &mode).unwrap().changes.is_empty());
        let duplicate = table("g,i,value\na,1,x\na,1,x\n");
        assert!(
            compare(&a, &duplicate, &mode)
                .unwrap_err()
                .contains("Ambiguous")
        );
        let missing = table("g,i,value\na,,x\n");
        assert!(
            compare(&a, &missing, &mode)
                .unwrap_err()
                .contains("Missing key")
        );
        assert!(compare(&a, &missing, &MatchBy::default()).is_err());
        assert!(
            compare(
                &a,
                &missing,
                &MatchBy {
                    position: true,
                    keys: vec![]
                }
            )
            .is_ok()
        );
    }
}
