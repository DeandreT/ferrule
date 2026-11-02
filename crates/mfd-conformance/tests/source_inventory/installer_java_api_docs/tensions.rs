//! Keep prose and annotation discrepancies separate from declared Java types.

use super::*;

const ORACLE: &str = include_str!(
    "../../../../../conformance/mfd-2026r2-source-inventory/oracles/installer-java-api-docs-tensions.json"
);

pub(super) fn validate(
    value: &Value,
    types: &BTreeMap<&str, &[Value]>,
    members: &BTreeMap<(&str, &str), &[Value]>,
    sources: &BTreeMap<&str, &[Value]>,
) -> Result<(), String> {
    let expected: Value = serde_json::from_str(ORACLE).map_err(|e| e.to_string())?;
    let tensions = &value["documentation_tensions"];
    require(tensions == &expected, "strict documentation tension oracle")?;
    let properties: Vec<_> = members
        .values()
        .filter(|m| m[0] == "M" && m[19].is_string())
        .collect();
    let owner_names: BTreeSet<_> = properties
        .iter()
        .map(|m| (m[1].as_str().unwrap(), m[19].as_str().unwrap()))
        .collect();
    require(
        properties.len() == 165 && owner_names.len() == 124,
        "standalone property labels",
    )?;

    let pointer = &tensions["pointer_storage"];
    let owner = string(&pointer["owner"])?;
    let ty = types.get(owner).ok_or("pointer owner")?;
    let bound = number(&sources[string(&ty[2])?][3])?;
    anchor(&pointer["prose_lines"], bound)?;
    hash(&pointer["normalized_text_sha256"])?;
    require(
        array(&ty[8])?.contains(&pointer["prose_lines"]),
        "pointer prose anchor",
    )?;
    let ctor = members
        .get(&(owner, string(&pointer["constructor_fragment"])?))
        .ok_or("pointer constructor")?;
    let accessor = members
        .get(&(owner, string(&pointer["accessor_fragment"])?))
        .ok_or("pointer accessor")?;
    require(
        ctor[13][0][1] == pointer["constructor_parameter_type"]
            && accessor[12] == pointer["accessor_return_type"]
            && pointer["native_width"].is_null(),
        "pointer prose is not native width or Java signature",
    )?;

    let mut annotation_count = 0;
    for entry in array(&tensions["iterator_annotations"])? {
        let owner = string(&entry["owner"])?;
        let ty = types.get(owner).ok_or("iterator owner")?;
        require(
            string(&ty[5])?.contains(string(&entry["declared_owner_route"])?),
            "actual generic declaration route",
        )?;
        let member = members
            .get(&(owner, string(&entry["fragment"])?))
            .ok_or("iterator member")?;
        let bound = number(&sources[string(&ty[2])?][3])?;
        for annotation in array(&entry["annotations"])? {
            annotation_count += 1;
            hash(&annotation["normalized_text_sha256"])?;
            anchor(&annotation["lines"], bound)?;
            require(
                array(&member[21])?.contains(&annotation["lines"]),
                "separate annotation source anchor",
            )?;
        }
    }
    require(
        annotation_count == 5,
        "iterator annotation fingerprint closure",
    )?;
    for entry in array(&tensions["add_notify"])? {
        let owner = string(&entry["owner"])?;
        let member = members
            .get(&(owner, string(&entry["fragment"])?))
            .ok_or("addNotify member")?;
        hash(&entry["normalized_text_sha256"])?;
        require(
            member[12] == entry["declared_return_type"]
                && member[12] == "void"
                && array(&member[20])?.contains(&entry["prose_lines"]),
            "addNotify prose does not change void declaration",
        )?;
    }
    Ok(())
}

#[test]
fn documented_tensions_and_correct_property_classifications_are_frozen() {
    super::validate(
        load("installer-java-api-docs.json"),
        load("installer-java-api-docs-sources.json"),
    )
    .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(ORACLE)),
        "e49d01ad243c238c5dc8920d72c1b08b03c286023911418cf6559ad58c95c249"
    );
}

#[test]
fn annotation_repairs_prose_return_promotion_and_native_width_inference_reject() {
    for (pointer, replacement) in [
        ("/property_labels/standalone_occurrences", json!(168)),
        ("/property_labels/distinct_owner_name_pairs", json!(126)),
        ("/pointer_storage/prose_storage_type", json!("long")),
        ("/pointer_storage/native_width", json!(64)),
        ("/pointer_storage/accessor_return_type", json!("int")),
        (
            "/iterator_annotations/0/annotations/0/printed_target",
            json!("java.util.Iterator<ELEMENT>"),
        ),
        (
            "/iterator_annotations/0/annotations/0/normalized_text_sha256",
            json!("0".repeat(64)),
        ),
        (
            "/iterator_annotations/0/annotations/0/lines",
            json!([229, 229]),
        ),
        (
            "/iterator_annotations/3/declared_owner_route",
            json!("implements java.util.Iterator<LIST>"),
        ),
        ("/add_notify/0/declared_return_type", json!("long")),
        (
            "/interpretation",
            json!("Independently verified runtime behavior"),
        ),
    ] {
        let mut value = load("installer-java-api-docs.json");
        *value["documentation_tensions"]
            .pointer_mut(pointer)
            .unwrap() = replacement;
        let error = super::validate(value, load("installer-java-api-docs-sources.json"))
            .expect_err("unsupported repair must reject");
        assert!(error.len() < 200);
    }
    let mut value = load("installer-java-api-docs.json");
    value["documentation_tensions"]["unreviewed"] = json!(true);
    let error = super::validate(value, load("installer-java-api-docs-sources.json"))
        .expect_err("unknown tension field must reject");
    assert!(error.len() < 200);
}
