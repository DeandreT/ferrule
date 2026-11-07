//! Bounded, fixture-independent controls for the maintained XML element-tree oracle.
//! Copied unchanged into each generated Rust consumer; no mapping is called here.
use super::xml_tree;

fn rejects(actual: &str, expected: &str) {
    assert!(
        std::panic::catch_unwind(|| xml_tree::assert_xml(actual, expected)).is_err(),
        "XML oracle accepted a deliberately different complete element tree"
    );
}

pub(super) fn assert_xml_oracle_controls() {
    let compact = "<Results><Included>0</Included><Excluded>0</Excluded></Results>";
    xml_tree::assert_xml(
        "<Results> \t\r\n<Included>0</Included>\n \t<Excluded>0</Excluded>\r\n</Results>",
        compact,
    );
    for actual in [
        "<Results><Excluded>0</Excluded><Included>0</Included></Results>",
        "<Results><Included>0</Included><Excluded>0</Excluded><Extra/></Results>",
        "<Results><Included>0</Included><Included>0</Included><Excluded>0</Excluded></Results>",
        "<Results><Included>0</Included>\u{a0}<Excluded>0</Excluded></Results>",
        "<Results><Included>0</Included>\u{2003}<Excluded>0</Excluded></Results>",
    ] {
        rejects(actual, compact);
    }
    xml_tree::assert_xml(
        "<a:R xmlns:a='urn:r' a:k='v' z='1'><!--keep--><?guard ok?><a:A>v</a:A></a:R>",
        "<b:R xmlns:b='urn:r' z='1' b:k='v'><!--keep--><?guard ok?><b:A>v</b:A></b:R>",
    );
    let nodes = "<R marker='yes'><!--keep--><?guard ok?><A>v</A></R>";
    for actual in [
        "<R marker='yes'><!--changed--><?guard ok?><A>v</A></R>",
        "<R marker='yes'><!--keep--><?guard changed?><A>v</A></R>",
        "<R marker='yes'><?guard ok?><!--keep--><A>v</A></R>",
        "<R marker='no'><!--keep--><?guard ok?><A>v</A></R>",
        "<R xmlns='urn:changed' marker='yes'><!--keep--><?guard ok?><A>v</A></R>",
    ] {
        rejects(actual, nodes);
    }
    xml_tree::assert_xml(
        "<R><A>\u{a0}\u{2003}</A></R>",
        "<R><A>\u{a0}\u{2003}</A></R>",
    );
    rejects("<R><A> </A></R>", "<R><A/></R>");
}

#[test]
fn complete_xml_oracle_rejects_order_shape_and_character_data_drift() {
    assert_xml_oracle_controls();
}
