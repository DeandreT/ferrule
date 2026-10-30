//! A design nested far deeper than any real one must be refused with an error.
//! The XML parser once recursed per level and aborted the whole process.

use std::sync::mpsc;
use std::time::Duration;

use mfd::{MfdPackage, import_package};

const LEVELS: usize = 200_000;

fn nested(open: &str, close: &str) -> String {
    let mut text = open.repeat(LEVELS);
    text.push_str(&close.repeat(LEVELS));
    text
}

#[test]
fn deeply_nested_design_is_refused() {
    let unclosed = "<a>".repeat(LEVELS);
    let closed = format!("<mapping>{}</mapping>", nested("<a>", "</a>"));
    for design in [unclosed, closed] {
        let package = MfdPackage::new("design.mfd", [("design.mfd", design.as_bytes())]).unwrap();
        assert!(import_package(&package).is_err());
    }
}

/// A long chain of plain elements inside a design component costs one step per
/// element in the root-view inventory, so the import finishes promptly. The
/// old per-element ancestor search was quadratic and would not.
#[test]
fn deep_component_chain_finishes_promptly() {
    const CHAIN: usize = 60_000;
    let design = format!(
        "<mapping version=\"22\"><component name=\"defaultmap\"><structure><children><component name=\"c\" library=\"xml\" uid=\"1\">{}</component></children><graph directed=\"1\"/></structure></component></mapping>",
        nested_chain(CHAIN)
    );
    let (sender, receiver) = mpsc::channel();
    // Debug frames are large; a big stack keeps any per-level recursion
    // elsewhere from masking the timing being tested.
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let package =
                MfdPackage::new("design.mfd", [("design.mfd", design.as_bytes())]).unwrap();
            let _ = sender.send(import_package(&package).map(|_| ()));
        })
        .unwrap();
    // The design has no target, so a refusal is expected; any result is fine.
    match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(_) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("the import did not finish in time"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the import panicked"),
    }
}

fn nested_chain(levels: usize) -> String {
    let mut text = "<n>".repeat(levels);
    text.push_str(&"</n>".repeat(levels));
    text
}
