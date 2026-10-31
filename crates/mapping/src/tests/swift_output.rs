use crate::{
    FormatOptions, SwiftMtDirection, SwiftMtMessageHeaders, SwiftMtOutputError,
    SwiftMtOutputOptions,
};

fn headers(count: usize) -> Vec<SwiftMtMessageHeaders> {
    (0..count)
        .map(|sequence| SwiftMtMessageHeaders {
            basic_header: format!("F01BANKBEBBAXXX0000{sequence:06}"),
            application_header: "BANKDEFFXXXXN".into(),
        })
        .collect()
}

#[test]
fn swift_output_options_round_trip_in_message_order() {
    let options = FormatOptions {
        swift_mt_output: Some(
            SwiftMtOutputOptions::new(SwiftMtDirection::Output, headers(2)).unwrap(),
        ),
        ..FormatOptions::default()
    };
    let json = serde_json::to_value(&options).unwrap();
    assert_eq!(json["swift_mt_output"]["direction"], "output");
    assert_eq!(
        json["swift_mt_output"]["messages"][1]["basic_header"],
        "F01BANKBEBBAXXX0000000001"
    );
    let decoded: FormatOptions = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, options);
}

#[test]
fn swift_output_options_refuse_empty_oversized_and_unknown_shapes() {
    assert_eq!(
        SwiftMtOutputOptions::new(SwiftMtDirection::Input, Vec::new()),
        Err(SwiftMtOutputError::NoMessages)
    );
    assert_eq!(
        SwiftMtOutputOptions::new(
            SwiftMtDirection::Input,
            headers(SwiftMtOutputOptions::MAX_MESSAGES + 1)
        ),
        Err(SwiftMtOutputError::TooManyMessages)
    );
    for invalid in [
        r#"{"swift_mt_output":{"direction":"input","messages":[]}}"#,
        r#"{"swift_mt_output":{"direction":"sideways","messages":[{"basic_header":"F","application_header":"B"}]}}"#,
        r#"{"swift_mt_output":{"direction":"input","messages":[{"basic_header":"F"}]}}"#,
        r#"{"swift_mt_output":{"direction":"input","messages":[],"extra":1}}"#,
        r#"{"swift_mt_output":{"direction":"input","messages":[{"basic_header":"F","application_header":"B","priority":"U"}]}}"#,
    ] {
        assert!(
            serde_json::from_str::<FormatOptions>(invalid).is_err(),
            "{invalid}"
        );
    }
}
