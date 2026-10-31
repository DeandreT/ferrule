#![no_main]

use flextext_layout_fuzz::{MAX_INPUT_BYTES, check};
use libfuzzer_sys::{Corpus, fuzz_target};

fuzz_target!(|bytes: &[u8]| -> Corpus {
    if bytes.len() > MAX_INPUT_BYTES {
        return Corpus::Reject;
    }
    check(bytes);
    Corpus::Keep
});
