use ir::Value;

const PICTURE: &str = "[Y,4-4]-[M,2-2]-[D,2-2]T[H,2-2]:[m,2-2]:[s,2-2].[f,1-7]Z";

#[test]
fn explicit_millisecond_format_truncates_and_pads_without_changing_local_time() {
    for (input, expected) in [
        ("2026-01-02T00:01:02", "2026-01-02T00:01:02.000Z"),
        ("2026-01-02T03:04:05.1", "2026-01-02T03:04:05.100Z"),
        ("2026-01-02T03:04:05.12", "2026-01-02T03:04:05.120Z"),
        ("2026-01-02T03:04:05.1234567", "2026-01-02T03:04:05.123Z"),
        ("2026-01-02T03:04:05.0009999", "2026-01-02T03:04:05.000Z"),
        (" 2026-01-02T03:04:05 ", "2026-01-02T03:04:05.000Z"),
        ("2026-01-02T24:00:00", "2026-01-03T00:00:00.000Z"),
        ("2024-02-29T24:00:00.5", "2024-03-01T00:00:00.500Z"),
        ("2026-12-31T24:00:00", "2027-01-01T00:00:00.000Z"),
        ("2026-01-02T03:04:05+14:01", "2026-01-02T03:04:05.000Z"),
        (
            "2026-01-02T23:59:59.9999999-07:00",
            "2026-01-02T23:59:59.999Z",
        ),
        (
            "2026-01-02T03:04:05.1234567+05:30",
            "2026-01-02T03:04:05.123Z",
        ),
    ] {
        assert_eq!(
            functions::call(
                "format_datetime_milliseconds",
                &[Value::String(input.into()), Value::String(PICTURE.into())]
            )
            .unwrap(),
            Value::String(expected.into())
        );
    }
    assert_eq!(
        functions::call(
            "format_datetime",
            &[
                Value::String("2026-01-02T03:04:05.1234567".into()),
                Value::String(PICTURE.into())
            ]
        )
        .unwrap(),
        Value::String("2026-01-02T03:04:05.1234567Z".into())
    );
    assert!(functions::call("format_datetime_milliseconds", &[]).is_err());
    assert!(
        functions::call(
            "format_datetime_milliseconds",
            &[Value::String("bad".into()), Value::String(PICTURE.into())]
        )
        .is_err()
    );
}

#[test]
fn native_default_year_width_and_refusals_do_not_change_the_ordinary_formatter() {
    let arguments = [
        Value::String("0001-01-01T00:00:00".into()),
        Value::String("[Y]".into()),
    ];
    assert_eq!(
        functions::call("format_datetime_milliseconds", &arguments).unwrap(),
        Value::String("1".into())
    );
    assert_eq!(
        functions::call("format_datetime", &arguments).unwrap(),
        Value::String("0001".into())
    );
    for input in [
        "2026-01-02T24:01:00",
        "2026-01-02T24:00:01",
        "9999-12-31T24:00:00",
        "2026-01-02T03:04:05+99:99",
    ] {
        assert!(
            functions::call(
                "format_datetime_milliseconds",
                &[Value::String(input.into()), Value::String(PICTURE.into())]
            )
            .is_err()
        );
    }
    for input in [
        " 2026-01-02T03:04:05 ",
        "2026-01-02T24:00:00",
        "2026-01-02T03:04:05+14:01",
    ] {
        assert!(
            functions::call(
                "format_datetime",
                &[Value::String(input.into()), Value::String(PICTURE.into())]
            )
            .is_err()
        );
    }
}
