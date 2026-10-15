//! Helpers shared by this crate's unit tests.

/// The current test's thread name as a file-name component that is valid on
/// every platform. The test harness names threads by module path
/// (`config::tests::name`), and `:` is not allowed in Windows file names.
pub(crate) fn thread_file_label() -> String {
    std::thread::current()
        .name()
        .unwrap_or("test")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
