//! Explicit read-only Windows filename selection. Literal paths (including
//! URLs) stay unchanged; opening, authentication and path authorization are not
//! performed here. Wildcard rules follow .NET's Win32 expression contract.
use std::fmt;

#[cfg(any(windows, test))]
#[path = "source_filename_case.rs"]
mod case;

pub const MAX_SOURCE_FILENAME_ENTRIES: usize = 100_000;
#[cfg(windows)]
const MAX_SCANNED_ENTRIES: usize = 1_000_000;
#[cfg(windows)]
const MAX_SELECTED_NAME_UNITS: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceFilenameError {
    InvalidLimit,
    Limit,
    UnsupportedPlatform,
}
impl fmt::Display for SourceFilenameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimit => "invalid source filename limit",
            Self::Limit => "source filename selection exceeded its bounds",
            Self::UnsupportedPlatform => "native filename selection requires Windows",
        })
    }
}
impl std::error::Error for SourceFilenameError {}

pub fn has_mapforce_filename_wildcard(value: &str) -> bool {
    value.contains(['*', '?'])
        && (value.encode_utf16().nth(1) == Some(b':' as u16) || !value.contains(':'))
}

/// Enumerate local files in native order without trimming the expression.
/// Missing/invalid directory searches produce no matches, as in the selected
/// native profile. A literal is returned without checking whether it exists.
/// Matching is bound to the qualified Windows .NET 8.0.22 casing profile,
/// rather than the Rust compiler's Unicode version. This does not load URLs.
pub fn select_mapforce_source_filenames(
    value: &str,
    maximum: usize,
) -> Result<Vec<String>, SourceFilenameError> {
    if maximum > MAX_SOURCE_FILENAME_ENTRIES {
        return Err(SourceFilenameError::InvalidLimit);
    }
    if !has_mapforce_filename_wildcard(value) {
        if maximum == 0 {
            return Err(SourceFilenameError::Limit);
        }
        return Ok(vec![value.into()]);
    }
    #[cfg(windows)]
    {
        windows::select(value, maximum)
    }
    #[cfg(not(windows))]
    {
        Err(SourceFilenameError::UnsupportedPlatform)
    }
}

// See System.IO.Enumeration.FileSystemName in dotnet/runtime v8.0.22 for
// the documented DOS_STAR, DOS_QM and DOS_DOT rules. This bounded dynamic
// program does not recurse with the length of an untrusted expression.
#[cfg(any(windows, test))]
fn expression(value: &str) -> Vec<u16> {
    if matches!(value, "" | "*" | "*.*") {
        return vec![b'*' as u16];
    }
    let units = value.encode_utf16().collect::<Vec<_>>();
    let mut result = Vec::new();
    for (i, ch) in units.iter().copied().enumerate() {
        match ch {
            63 => result.push(62),
            46 if i > 0 && i + 1 == units.len() && units[i - 1] == 42 => {
                result.pop();
                result.push(60);
            }
            46 if units.get(i + 1).is_some_and(|c| matches!(c, 42 | 63)) => result.push(34),
            _ => result.push(ch),
        }
    }
    result
}

#[cfg(any(windows, test))]
fn upper(unit: u16) -> u16 {
    case::upper(unit as u32, false) as u16
}

#[cfg(any(windows, test))]
fn matches(pattern: &[u16], name: &[u16], work: &mut usize) -> Result<bool, SourceFilenameError> {
    if name.is_empty() {
        return Ok(false);
    }
    if pattern.first() == Some(&(b'*' as u16))
        && pattern[1..]
            .iter()
            .all(|c| !matches!(c, 34 | 60 | 62 | 42 | 63))
    {
        if name.len() < pattern.len() - 1 {
            return Ok(false);
        }
        let tail = &name[name.len() - (pattern.len() - 1)..];
        let fold = |units: &[u16]| {
            char::decode_utf16(units.iter().copied())
                .map(|r| {
                    r.map_or_else(
                        |e| 0x110000 + e.unpaired_surrogate() as u32,
                        |c| case::upper(c as u32, true),
                    )
                })
                .collect::<Vec<_>>()
        };
        return Ok(fold(tail) == fold(&pattern[1..]));
    }
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < pattern.len() {
        if pattern[i] == 92 && i + 1 < pattern.len() {
            tokens.push((pattern[i + 1], true));
            i += 2;
        } else {
            tokens.push((pattern[i], false));
            i += 1;
        }
    }
    let cost = tokens.len().saturating_mul(name.len() + 1);
    *work = work.checked_add(cost).ok_or(SourceFilenameError::Limit)?;
    if *work > 50_000_000 {
        return Err(SourceFilenameError::Limit);
    }
    let last_dot = name.iter().rposition(|c| *c == 46);
    let mut next = vec![false; name.len() + 1];
    next[name.len()] = true;
    for (ch, literal) in tokens.into_iter().rev() {
        let mut row = vec![false; name.len() + 1];
        for at in (0..=name.len()).rev() {
            row[at] = if !literal && ch == 42 {
                next[at] || (at < name.len() && row[at + 1])
            } else if !literal && ch == 60 {
                next[at] || (at < name.len() && Some(at) != last_dot && row[at + 1])
            } else if !literal && ch == 62 {
                if at == name.len() || name[at] == 46 {
                    next[at]
                } else {
                    next[at + 1]
                }
            } else if !literal && ch == 34 {
                (at == name.len() && next[at])
                    || (at < name.len() && name[at] == 46 && next[at + 1])
            } else {
                at < name.len()
                    && (ch == 63 && !literal || upper(ch) == upper(name[at]))
                    && next[at + 1]
            };
        }
        next = row;
    }
    Ok(next[0])
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    pub(super) fn select(value: &str, maximum: usize) -> Result<Vec<String>, SourceFilenameError> {
        if value.contains('\0') {
            return Ok(vec![]);
        }
        if value.encode_utf16().count() > 32760 {
            return Err(SourceFilenameError::Limit);
        }
        let split = value.rfind(['\\', '/']).map_or(0, |i| i + 1);
        let root = if split == 0 { "." } else { &value[..split] };
        let leaf = &value[split..];
        let Ok(full) = std::path::absolute(root) else {
            return Ok(vec![]);
        };
        let Ok(entries) = std::fs::read_dir(&full) else {
            return Ok(vec![]);
        };
        let mut selected = Vec::new();
        let mut units = 0;
        let mut work = 0;
        let pattern = expression(leaf);
        for (count, entry) in entries.enumerate() {
            if count >= MAX_SCANNED_ENTRIES {
                return Err(SourceFilenameError::Limit);
            }
            let Ok(entry) = entry else {
                return Ok(vec![]);
            };
            let Ok(metadata) = entry.metadata() else {
                return Ok(vec![]);
            };
            if metadata.file_attributes() & 0x10 == 0 {
                let name = entry.file_name();
                let name_units = name.encode_wide().collect::<Vec<_>>();
                if matches(&pattern, &name_units, &mut work)? {
                    let path = full
                        .join(name)
                        .into_os_string()
                        .into_string()
                        .map_err(|_| SourceFilenameError::Limit)?;
                    units += path.encode_utf16().count();
                    if selected.len() == maximum || units > MAX_SELECTED_NAME_UNITS {
                        return Err(SourceFilenameError::Limit);
                    }
                    selected.push(path);
                }
            }
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_classifier_keeps_original_input_and_urls() {
        assert!(has_mapforce_filename_wildcard("C:\\*.x12"));
        assert!(has_mapforce_filename_wildcard("*.x12"));
        assert!(!has_mapforce_filename_wildcard(" C:\\*.x12 "));
        assert!(!has_mapforce_filename_wildcard("http://host/*.x12"));
        assert_eq!(
            select_mapforce_source_filenames(" raw literal ", 1).unwrap(),
            [" raw literal "]
        );
    }
    #[test]
    fn win32_expression_contract_includes_optional_question_marks() {
        for (pattern, name, wanted) in [
            ("*.x12", "a.X12", true),
            ("a?.x12", "a.x12", true),
            ("a?.x12", "abc.x12", false),
            ("*.*", "extensionless", true),
            ("*.", "extensionless", true),
            ("*.", "a.txt", false),
            ("a<*.x12", "a.x12", true),
            ("literal?.x12", "literal[1].x12", false),
        ] {
            assert_eq!(
                matches(
                    &expression(pattern),
                    &name.encode_utf16().collect::<Vec<_>>(),
                    &mut 0
                )
                .unwrap(),
                wanted,
                "{pattern} {name}"
            );
        }
    }
}
