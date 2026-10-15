use super::*;
use ir::ScalarType;

const WIRE: &str = "ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*U*00401*000000123*0*T*:~\nGS*PO*ALPHA*BETA*20300615*0945*7*X*004010~\nST*850*0007~\nNTE*one~\nNTE*two~\nSE*4*0007~\nGE*1*7~\nIEA*1*000000123~\n";

fn fields(id: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        id,
        (1..=count)
            .map(|at| SchemaNode::scalar(format!("E{at:02}"), ScalarType::String))
            .collect(),
    )
}
fn schema(root: &str) -> SchemaNode {
    let mut isa = fields("ISA", 16);
    if let SchemaKind::Group { children, .. } = &mut isa.kind {
        children[0].name = "ST".into();
        children[10].name = "ProtocolMarker".into();
    }
    let mut note = fields("NTE", 1).repeating();
    if let SchemaKind::Group { children, .. } = &mut note.kind {
        children.push(fields("ST", 2));
    }
    SchemaNode::group(
        root,
        vec![
            isa,
            fields("GS", 8),
            fields("ST", 2),
            note,
            fields("SE", 2),
            fields("GE", 2),
            fields("IEA", 2),
        ],
    )
}

#[test]
fn document_cursor_excludes_roots_scalars_and_positional_composites() {
    for root in ["Envelope", "ISA", "ST"] {
        let schema = schema(root);
        let ids: Vec<_> = SchemaCursor::root(&schema)
            .segments()
            .map(|node| node.segment_id().unwrap())
            .collect();
        assert_eq!(ids, ["ISA", "GS", "ST", "NTE", "SE", "GE", "IEA"]);
        let value = from_str(WIRE, &schema, false).unwrap();
        assert_eq!(
            to_document_string_with_syntax(
                &schema,
                &value,
                Separators::default(),
                Some("00401"),
                None
            )
            .unwrap(),
            WIRE
        );
    }
}

#[test]
fn owned_root_lookup_preserves_native_error_field_labels() {
    let schema = schema("ISA");
    let mut value = from_str(WIRE, &schema, false).unwrap();
    let Instance::Group(root) = &mut value else {
        panic!("root")
    };
    let Instance::Group(isa) = &mut root.iter_mut().find(|(name, _)| name == "ISA").unwrap().1
    else {
        panic!("ISA")
    };
    isa.iter_mut()
        .find(|(name, _)| name == "ProtocolMarker")
        .unwrap()
        .1 = Instance::Scalar(Value::String("!".into()));
    assert!(
        matches!(to_document_string_with_syntax(&schema,&value,Separators::default(),Some("00401"),None),Err(EdiFormatError::InvalidEnvelopeElement{element,value,..}) if element=="ProtocolMarker" && value=="!")
    );
}

#[test]
fn legacy_bare_isa_lookup_and_fragment_tokenizing_remain_unchanged() {
    let schema = schema("Envelope");
    let value = from_str(WIRE, &schema, false).unwrap();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("schema")
    };
    let isa = value.field("ISA").unwrap();
    assert!(find_schema_instance(&children[0], isa, "ISA").is_some());
    let options = write_options(&children[0], isa, WRITE_OPTIONS).unwrap();
    assert_eq!(options.repetition, None);
    assert_eq!(tokenize(WIRE.lines().next().unwrap()).unwrap().len(), 1);
    let fragment_schema = SchemaNode::group("Fragment", vec![children[0].clone()]);
    let fragment = Instance::Group(vec![("ISA".into(), isa.clone())].into());
    assert_eq!(
        to_string_with_syntax(
            &fragment_schema,
            &fragment,
            Separators::default(),
            Some("00401")
        )
        .unwrap(),
        format!("{}\n", WIRE.lines().next().unwrap())
    );
    assert_eq!(
        find_document_instance(SchemaCursor::root(&schema), &value, "ISA")
            .unwrap()
            .0
            .name,
        "ISA"
    );
}
