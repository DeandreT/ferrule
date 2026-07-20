use ir::Value;

use crate::FunctionError;

pub(super) fn get_folder(args: &[Value]) -> Result<Value, FunctionError> {
    unary_path(args, "get_folder", |path| match last_separator(path) {
        Some(index) => path[..=index].to_string(),
        None => String::new(),
    })
}

pub(super) fn remove_folder(args: &[Value]) -> Result<Value, FunctionError> {
    unary_path(args, "remove_folder", |path| match last_separator(path) {
        Some(index) => path[index + 1..].to_string(),
        None => path.to_string(),
    })
}

pub(super) fn get_fileext(args: &[Value]) -> Result<Value, FunctionError> {
    unary_path(args, "get_fileext", |path| {
        let separator = last_separator(path);
        match path.rfind('.') {
            Some(dot) if separator.is_none_or(|separator| dot > separator) => {
                path[dot..].to_string()
            }
            _ => String::new(),
        }
    })
}

pub(super) fn resolve_filepath(args: &[Value]) -> Result<Value, FunctionError> {
    let [base, path] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "resolve_filepath",
            expected: 2,
            got: args.len(),
        });
    };
    let Value::String(base) = base else {
        return Err(type_error("resolve_filepath", base));
    };
    let Value::String(path) = path else {
        return Err(type_error("resolve_filepath", path));
    };
    if is_absolute(path) {
        return Ok(Value::String(path.clone()));
    }

    let delimiter = common_separator(base, path);
    let mut folder = base.clone();
    if !folder.is_empty() && !folder.ends_with(['/', '\\']) {
        folder.push(delimiter);
    }
    let mut relative = strip_current_directories(path);
    while let Some(rest) = parent_remainder(relative) {
        folder = parent_folder(&folder, delimiter);
        relative = rest;
    }
    Ok(Value::String(format!("{folder}{relative}")))
}

fn unary_path(
    args: &[Value],
    name: &'static str,
    operation: impl FnOnce(&str) -> String,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(path)] => Ok(Value::String(operation(path))),
        [other] => Err(type_error(name, other)),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 1,
            got: args.len(),
        }),
    }
}

fn type_error(function: &'static str, value: &Value) -> FunctionError {
    FunctionError::TypeMismatch {
        function,
        got: value.type_name(),
    }
}

fn last_separator(path: &str) -> Option<usize> {
    path.rfind(['/', '\\'])
}

fn path_separator(path: &str) -> Option<char> {
    match (path.contains('/'), path.contains('\\')) {
        (true, false) => Some('/'),
        (false, true) => Some('\\'),
        _ => None,
    }
}

fn common_separator(base: &str, path: &str) -> char {
    match (path_separator(base), path_separator(path)) {
        (Some(left), Some(right)) if left == right => left,
        (Some(separator), None) | (None, Some(separator)) => separator,
        // MapForce's format-neutral implementation uses backslash when the
        // inputs disagree or provide no separator preference.
        _ => '\\',
    }
}

fn is_absolute(path: &str) -> bool {
    path.starts_with(['/', '\\']) || path.contains(':')
}

fn strip_current_directories(mut path: &str) -> &str {
    loop {
        if path == "." {
            return "";
        }
        match path.strip_prefix("./").or_else(|| path.strip_prefix(".\\")) {
            Some(rest) => path = rest,
            None => return path,
        }
    }
}

fn parent_remainder(path: &str) -> Option<&str> {
    if path == ".." {
        Some("")
    } else {
        path.strip_prefix("../")
            .or_else(|| path.strip_prefix("..\\"))
    }
}

fn parent_folder(folder: &str, delimiter: char) -> String {
    let trimmed = folder.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && folder.starts_with(['/', '\\']) {
        return delimiter.to_string();
    }
    if is_drive_prefix(trimmed) {
        return format!("{trimmed}{delimiter}");
    }
    match last_separator(trimmed) {
        Some(index) => trimmed[..=index].to_string(),
        None if trimmed.is_empty() => format!("..{delimiter}"),
        None => String::new(),
    }
}

fn is_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call;

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    #[test]
    fn folder_and_filename_split_on_the_last_separator() {
        for (path, folder, filename) in [
            ("/var/data/file.xml", "/var/data/", "file.xml"),
            (r"C:\data\file.xml", "C:\\data\\", "file.xml"),
            (r"one/two\file.xml", "one/two\\", "file.xml"),
            ("file.xml", "", "file.xml"),
            ("/var/data/", "/var/data/", ""),
        ] {
            assert_eq!(call("get_folder", &[string(path)]), Ok(string(folder)));
            assert_eq!(call("remove_folder", &[string(path)]), Ok(string(filename)));
        }
    }

    #[test]
    fn file_extension_keeps_the_dot_after_the_last_separator() {
        for (path, extension) in [
            ("/var/data/file.xml", ".xml"),
            (r"C:\data\archive.tar.gz", ".gz"),
            ("folder.name/file", ""),
            (".profile", ".profile"),
            ("file.", "."),
            ("folder/.hidden", ".hidden"),
            ("folder/", ""),
        ] {
            assert_eq!(call("get_fileext", &[string(path)]), Ok(string(extension)));
        }
    }

    #[test]
    fn resolves_posix_and_windows_relative_paths_lexically() {
        for (base, path, expected) in [
            ("/var/data", "reports/out.xml", "/var/data/reports/out.xml"),
            (
                r"C:\work\data",
                r"reports\out.xml",
                r"C:\work\data\reports\out.xml",
            ),
            ("/var/data/current/", "../out.xml", "/var/data/out.xml"),
            (r"C:\work\data\", r"..\out.xml", r"C:\work\out.xml"),
            ("/", "../out.xml", "/out.xml"),
            (r"C:\", r"..\out.xml", r"C:\out.xml"),
            ("/var/data", "././out.xml", "/var/data/out.xml"),
        ] {
            assert_eq!(
                call("resolve_filepath", &[string(base), string(path)]),
                Ok(string(expected))
            );
        }
    }

    #[test]
    fn absolute_paths_are_returned_verbatim() {
        for (base, path) in [
            ("/ignored/base", "/etc/config.xml"),
            (r"C:\ignored", r"D:\data\config.xml"),
            (r"C:\ignored", r"\\server\share\config.xml"),
            ("/ignored/base", "https://example.test/config.xml"),
        ] {
            assert_eq!(
                call("resolve_filepath", &[string(base), string(path)]),
                Ok(string(path))
            );
        }
    }

    #[test]
    fn mixed_separators_use_the_unambiguous_input_style() {
        for (base, path, expected) in [
            (
                r"C:/work\data",
                "reports/out.xml",
                r"C:/work\data/reports/out.xml",
            ),
            (
                "/var/data",
                r"reports\out.xml",
                r"/var/data\reports\out.xml",
            ),
            (r"C:\work", "reports/out.xml", r"C:\work\reports/out.xml"),
        ] {
            assert_eq!(
                call("resolve_filepath", &[string(base), string(path)]),
                Ok(string(expected))
            );
        }
    }

    #[test]
    fn path_functions_reject_invalid_arity() {
        for (name, expected) in [("get_folder", 1), ("remove_folder", 1), ("get_fileext", 1)] {
            assert_eq!(
                call(name, &[]),
                Err(FunctionError::ArityMismatch {
                    function: name,
                    expected,
                    got: 0,
                })
            );
            assert_eq!(
                call(name, &[string("one"), string("two")]),
                Err(FunctionError::ArityMismatch {
                    function: name,
                    expected,
                    got: 2,
                })
            );
        }
        assert_eq!(
            call("resolve_filepath", &[string("base")]),
            Err(FunctionError::ArityMismatch {
                function: "resolve_filepath",
                expected: 2,
                got: 1,
            })
        );
        assert_eq!(
            call(
                "resolve_filepath",
                &[string("base"), string("path"), string("extra")],
            ),
            Err(FunctionError::ArityMismatch {
                function: "resolve_filepath",
                expected: 2,
                got: 3,
            })
        );
    }
}
