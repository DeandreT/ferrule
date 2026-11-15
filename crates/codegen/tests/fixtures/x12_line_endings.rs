//! Independently authored ASCII toy; no writer constructs these expected bytes.
#![allow(dead_code)]
use codegen::{X12BoundaryOptions, X12BoundaryPolicy, X12EnvelopeProfile};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{Project, Scope, ScopeConstruction, X12SegmentLineEnding};
use serde_json::{Value, json};

pub const SEGMENTS: [&str; 8] = [
    "ISA*00*          *00*          *ZZ*ORIGIN         *ZZ*DEST           *301122*1015*U*00401*000000007*0*T*:~",
    "GS*PO*ORIGIN*DEST*20301122*1015*7*X*004010~",
    "ST*850*0001~",
    "NTE*alpha**omega~",
    "NTE*beta~",
    "SE*4*0001~",
    "GE*1*7~",
    "IEA*1*000000007~",
];
pub fn expected(ending: &str) -> String {
    SEGMENTS.join(ending) + ending
}
fn fields(id: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        id,
        (1..=count)
            .map(|n| SchemaNode::scalar(format!("E{n:02}"), ScalarType::String))
            .collect(),
    )
}
pub fn schema(profile: X12EnvelopeProfile) -> SchemaNode {
    let mut isa = fields("ISA", 16);
    let SchemaKind::Group { children, .. } = &mut isa.kind else {
        unreachable!("fixture ISA is a group");
    };
    children[11].fixed = Some("00401".into());
    let mut gs = fields("GS", 8);
    let SchemaKind::Group { children, .. } = &mut gs.kind else {
        unreachable!("fixture GS is a group");
    };
    children[7].fixed = Some("004010".into());
    let (st, notes, se, ge, iea) = (
        fields("ST", 2),
        fields("NTE", 3).repeating(),
        fields("SE", 2),
        fields("GE", 2),
        fields("IEA", 2),
    );
    match profile {
        X12EnvelopeProfile::SingleTransaction => {
            SchemaNode::group("Envelope", vec![isa, gs, st, notes, se, ge, iea])
        }
        X12EnvelopeProfile::GroupedTransactions => SchemaNode::group(
            "Envelope",
            vec![
                isa,
                SchemaNode::group(
                    "Groups",
                    vec![
                        gs,
                        SchemaNode::group("Messages", vec![st, notes, se]).repeating(),
                        ge,
                    ],
                )
                .repeating(),
                iea,
            ],
        ),
        X12EnvelopeProfile::MapForceInterchanges => SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchanges",
                    vec![
                        isa.repeating(),
                        SchemaNode::group(
                            "Groups",
                            vec![
                                gs.repeating(),
                                SchemaNode::group("Messages", vec![st, notes, se.repeating()])
                                    .repeating(),
                                ge.repeating(),
                            ],
                        )
                        .repeating(),
                        iea.repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
    }
}
pub fn project(profile: X12EnvelopeProfile) -> Project {
    let schema = schema(profile);
    Project {
        execution_contract: None,
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    }
}
fn segment(values: &[&str]) -> Value {
    Value::Object(
        values
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("E{:02}", i + 1), json!(v)))
            .collect(),
    )
}
pub fn values(profile: X12EnvelopeProfile, complete: bool) -> Value {
    let isa = segment(&[
        "00",
        "",
        "00",
        "",
        "ZZ",
        "ORIGIN",
        "ZZ",
        "DEST",
        "301122",
        "1015",
        "U",
        "00401",
        "000000007",
        "0",
        "T",
        ":",
    ]);
    let gs = segment(&[
        "PO", "ORIGIN", "DEST", "20301122", "1015", "7", "X", "004010",
    ]);
    let st = segment(&["850", "0001"]);
    let notes = json!([{"E01":"alpha","E03":"omega"},{"E01":"beta"}]);
    let (se, ge, iea) = (
        segment(&["4", "0001"]),
        segment(&["1", "7"]),
        segment(&["1", "000000007"]),
    );
    let mut value = match profile {
        X12EnvelopeProfile::SingleTransaction => {
            json!({"ISA":isa,"GS":gs,"ST":st,"NTE":notes,"SE":se,"GE":ge,"IEA":iea})
        }
        X12EnvelopeProfile::GroupedTransactions => {
            json!({"ISA":isa,"Groups":[{"GS":gs,"Messages":[{"ST":st,"NTE":notes,"SE":se}],"GE":ge}],"IEA":iea})
        }
        X12EnvelopeProfile::MapForceInterchanges => {
            json!({"Interchanges":[{"ISA":[isa],"Groups":[{"GS":[gs],"Messages":[{"ST":st,"NTE":notes,"SE":[se]}],"GE":[ge]}],"IEA":[iea]}]})
        }
    };
    if !complete {
        remove_trailers(&mut value);
    }
    value
}
fn remove_trailers(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for id in ["SE", "GE", "IEA"] {
                fields.remove(id);
            }
            for child in fields.values_mut() {
                remove_trailers(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_trailers(item);
            }
        }
        _ => {}
    }
}
pub fn policy(
    profile: X12EnvelopeProfile,
    ending: X12SegmentLineEnding,
    completion: bool,
) -> X12BoundaryPolicy {
    let options = X12BoundaryOptions {
        envelope_profile: profile,
        interchange_version: Some("00401".into()),
        ..Default::default()
    };
    X12BoundaryPolicy {
        source: (!completion).then_some(options.clone()),
        target: Some(X12BoundaryOptions {
            segment_line_ending: ending,
            autocomplete: completion.then_some(mapping::X12Autocomplete {
                request_acknowledgement: false,
                transaction_set: Some("850".into()),
            }),
            ..options
        }),
    }
}
