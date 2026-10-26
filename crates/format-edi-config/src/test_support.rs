//! Helpers shared by this crate's unit tests.

/// Keep test-thread names portable when used in temporary paths.
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
