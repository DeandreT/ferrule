//! A design nested far deeper than any real one must be refused with an error.
//! The XML parser once recursed per level and aborted the whole process.

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
