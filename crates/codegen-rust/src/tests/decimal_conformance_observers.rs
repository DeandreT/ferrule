//! Compile the retained scalar observers, not the full conformance campaigns.
use super::*;

const OBSERVERS: [&str; 10] = [
    include_str!("../../../../conformance/basic-owned-filter/emitter.rs"),
    include_str!("../../../../conformance/basic-owned-filter/rust-caller.template.rs"),
    include_str!("../../../../conformance/basic-owned-nested-alias/emitter.rs"),
    include_str!("../../../../conformance/basic-owned-nested-alias/rust-caller.template.rs"),
    include_str!("../../../../conformance/basic-owned-occurrence/emitter.rs"),
    include_str!("../../../../conformance/basic-owned-occurrence/rust-caller.template.rs"),
    include_str!("../../../../conformance/basic-owned-sort/emitter.rs"),
    include_str!("../../../../conformance/basic-owned-sort/rust-caller.template.rs"),
    include_str!("../../../../conformance/basic-owned-windows/emitter.rs"),
    include_str!("../../../../conformance/basic-owned-windows/rust-caller.template.rs"),
];

#[test]
fn conformance_scalar_observers_compile_and_preserve_decimal_identity() {
    let directory = TempDir::new("decimal_conformance_observers");
    let root = directory.path().to_path_buf();
    // Retain the complete reached compiler inputs and outputs, including failures.
    std::mem::forget(directory);
    fs::create_dir(root.join("src")).unwrap();
    let ir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ir")
        .canonicalize()
        .unwrap();
    let manifest = format!(
        "[package]\nname = \"decimal_conformance_observers\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n\n[dependencies]\nir = {{ path = {} }}\nserde_json = {{ version = \"=1.0.150\", features = [\"preserve_order\"] }}\n",
        serde_json::to_string(&ir).unwrap()
    );
    fs::write(root.join("Cargo.toml"), manifest).unwrap();
    let mut source = String::from(
        "use ir::{Decimal96, Instance, Value};\nuse serde_json::{json, Value as Json};\n",
    );
    for (index, observer) in OBSERVERS.iter().enumerate() {
        let begin = observer.find("fn tree(").expect("retained tree observer");
        let remaining = &observer[begin..];
        let end = remaining.find("\nfn ").expect("observer end boundary");
        let body = remaining[..end].replacen("fn tree(", "pub fn tree(", 1);
        source.push_str(&format!(
            "mod observer_{index} {{ use super::*;\n{body}\n}}\n"
        ));
    }
    let calls = (0..OBSERVERS.len())
        .map(|index| format!("observer_{index}::tree"))
        .collect::<Vec<_>>()
        .join(", ");
    source.push_str(&format!(
        "fn main() {{ let observers: [fn(&Instance) -> Json; 10] = [{calls}];\n"
    ));
    source.push_str(
        r#"let cases = [
    (Value::decimal(Decimal96::parse_exact("9007199254740993.1").unwrap()), json!({"kind":"Scalar","type":"Decimal","value":{"$decimal96":"9007199254740993.1"}})),
    (Value::decimal(Decimal96::parse_exact("-0.0000000000000000000000000001").unwrap()), json!({"kind":"Scalar","type":"Decimal","value":{"$decimal96":"-0.0000000000000000000000000001"}})),
    (Value::decimal(Decimal96::ZERO), json!({"kind":"Scalar","type":"Decimal","value":{"$decimal96":"0"}})),
    (Value::Int(7), json!({"kind":"Scalar","type":"Int","value":7})),
    (Value::Float(0.25), json!({"kind":"Scalar","type":"Float","value":0.25})),
    (Value::Bool(true), json!({"kind":"Scalar","type":"Bool","value":true})),
    (Value::String("sample".into()), json!({"kind":"Scalar","type":"String","value":"sample"})),
    (Value::Null, json!({"kind":"Scalar","type":"Null","value":null})),
];
for (observer, observe) in observers.into_iter().enumerate() {
    for (case, (value, expected)) in cases.iter().enumerate() {
        let actual = observe(&Instance::Scalar(value.clone()));
        println!("OBSERVATION {}", json!({"observer":observer,"case":case,"actual":actual,"expected":expected}));
        assert_eq!(&actual, expected, "observer {observer}, case {case}");
    }
}
println!("PASS 80 scalar observations");
}
"#,
    );
    fs::write(root.join("src/main.rs"), source).unwrap();
    let result = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&root)
        .generated_host_output(&root)
        .unwrap();
    fs::write(root.join("stdout.txt"), &result.stdout).unwrap();
    fs::write(root.join("stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "conformance scalar observer host failed at {}:\n{}",
        root.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("PASS 80 scalar observations"));
}
