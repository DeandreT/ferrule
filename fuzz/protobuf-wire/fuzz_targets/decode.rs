#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::{Corpus, fuzz_target};
use protobuf_wire_fuzz::{MAX_INPUT_BYTES, Root, check, roots};

static ROOTS: LazyLock<Vec<Root>> = LazyLock::new(roots);

fuzz_target!(|bytes: &[u8]| -> Corpus {
    if bytes.len() > MAX_INPUT_BYTES {
        return Corpus::Reject;
    }
    check(&ROOTS, bytes);
    Corpus::Keep
});
