use codegen_runtime::{MapForceEdiScalar, MapForceEdiScalarKind, format_mapforce_edi_scalar};
use ir::Value;

#[test]
fn mapped_edi_scalar_profile_matches_literal_cases() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../conformance/mapforce-edi-scalars.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let field = MapForceEdiScalar {
            kind: serde_json::from_value(case["kind"].clone()).unwrap(),
            min_chars: case["min"].as_u64().unwrap() as u32,
            max_chars: case["max"].as_u64().unwrap() as u32,
        };
        let result = format_mapforce_edi_scalar(
            field,
            &Value::String(case["input"].as_str().unwrap().into()),
        );
        if case["error"] == true {
            assert!(result.is_err(), "{case}");
        } else {
            assert_eq!(result.unwrap(), case["output"].as_str().unwrap(), "{case}");
        }
    }
}

#[test]
fn bounds_and_scalar_domain_stay_explicit() {
    let field = MapForceEdiScalar {
        kind: MapForceEdiScalarKind::Text,
        min_chars: 2,
        max_chars: 2,
    };
    assert_eq!(format_mapforce_edi_scalar(field, &Value::Null).unwrap(), "");
    assert!(format_mapforce_edi_scalar(field, &Value::Float(f64::INFINITY)).is_err());
    assert_eq!(
        format_mapforce_edi_scalar(field, &Value::Float(1.25)).unwrap(),
        "1.25"
    );
    assert!(
        format_mapforce_edi_scalar(
            MapForceEdiScalar {
                min_chars: 3,
                ..field
            },
            &Value::String("X".into())
        )
        .is_err()
    );
}
