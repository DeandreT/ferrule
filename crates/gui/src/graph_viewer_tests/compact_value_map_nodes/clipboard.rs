use super::*;

#[test]
fn compact_value_map_real_keyboard_clipboard_and_disabled_popup_preserve_string_on_change_only() {
    let control = |output: &egui::FullOutput, role, label: &str| {
        let update = output.platform_output.accesskit_update.as_ref().unwrap();
        let nodes = update
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                (node.role() == role && node.label() == Some(label)).then_some(node)
            })
            .collect::<Vec<_>>();
        eprintln!("Compact ValueMap actual labelled control {label}: {nodes:?}");
        assert_eq!(nodes.len(), 1, "one actual labelled control: {label}");
        let node = nodes[0];
        assert!(!node.is_disabled(), "actual enabled control: {label}");
        let bounds = node.bounds().unwrap();
        let rect = egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        );
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))
                .contains_rect(rect),
            "control fully visible: {label} {rect:?}"
        );
        rect.center()
    };
    let original_table = vec![
        (Value::Int(123), Value::Bool(true)),
        (Value::String("second".into()), Value::Float(-0.0)),
    ];
    let (mut fx, mut snarl) = map_fixture(
        Some(ScalarType::Int),
        original_table.clone(),
        Some(Value::Null),
    );
    let before = state(&fx, &snarl);
    let wires = snarl.wires().collect::<Vec<_>>();
    let context = egui::Context::default();
    context.enable_accesskit();
    crate::icons::install(&context);
    let disabled = open(&mut fx, &mut snarl, &context, false);
    assert!(positions(&disabled, "Default").is_empty());
    assert_eq!(state(&fx, &snarl), before);
    let (_, pencil) = properties(&mut fx, &mut snarl, &context, true, Vec::new());
    context.memory_mut(|memory| memory.request_focus(pencil));
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key(egui::Key::Enter)],
    );
    let opened = settle(&mut fx, &mut snarl, &context, true);
    assert!(!positions(&opened, "Default").is_empty());
    assert_eq!(state(&fx, &snarl), before);
    let locked = settle(&mut fx, &mut snarl, &context, false);
    assert!(
        !positions(&locked, "Default").is_empty(),
        "already-open editor remains inspectable"
    );
    click(
        &mut fx,
        &mut snarl,
        &context,
        false,
        positions(&locked, "123")[0],
    );
    properties(
        &mut fx,
        &mut snarl,
        &context,
        false,
        vec![
            key_with(egui::Key::A, command()),
            egui::Event::Paste("blocked".into()),
        ],
    );
    let locked = settle(&mut fx, &mut snarl, &context, false);
    click(
        &mut fx,
        &mut snarl,
        &context,
        false,
        positions(&locked, "Default")[0],
    );
    let locked = settle(&mut fx, &mut snarl, &context, false);
    let plus = char::from(lucide_icons::Icon::Plus).to_string();
    click(
        &mut fx,
        &mut snarl,
        &context,
        false,
        positions(&locked, &plus)[0],
    );
    settle(&mut fx, &mut snarl, &context, false);
    assert_eq!(
        state(&fx, &snarl),
        before,
        "disabled detached popup edited table/default/input type"
    );
    let output = settle(&mut fx, &mut snarl, &context, true);
    let point = control(&output, egui::accesskit::Role::ComboBox, "Entry 1 key type");
    click(&mut fx, &mut snarl, &context, true, point);
    let output = settle(&mut fx, &mut snarl, &context, true);
    assert!(
        egui::Popup::is_any_open(&context),
        "real cell type choices opened"
    );
    let point = control(&output, egui::accesskit::Role::Button, "string");
    click(&mut fx, &mut snarl, &context, true, point);
    let output = settle(&mut fx, &mut snarl, &context, true);
    assert!(
        !egui::Popup::is_any_open(&context),
        "cell type choice closed"
    );
    let point = control(
        &output,
        egui::accesskit::Role::TextInput,
        "Entry 1 key value",
    );
    click(&mut fx, &mut snarl, &context, true, point);
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![
            key_with(egui::Key::A, command()),
            egui::Event::Paste("資料📦".into()),
        ],
    );
    let output = settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(
        table(&fx)[0].0,
        Value::Int(123),
        "typed value retained before Apply"
    );
    assert_eq!(
        state(&fx, &snarl),
        before,
        "draft does not commit any graph or layout change"
    );
    let point = control(&output, egui::accesskit::Role::Button, "Apply Entry 1 key");
    click(&mut fx, &mut snarl, &context, true, point);
    assert_eq!(table(&fx)[0].0, Value::String("資料📦".into()));
    let output = settle(&mut fx, &mut snarl, &context, true);
    let point = control(
        &output,
        egui::accesskit::Role::TextInput,
        "Entry 1 key value",
    );
    click(&mut fx, &mut snarl, &context, true, point);
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key_with(egui::Key::A, command())],
    );
    let copied = properties(&mut fx, &mut snarl, &context, true, vec![egui::Event::Copy]).0;
    assert!(
        copied
            .platform_output
            .commands
            .contains(&egui::OutputCommand::CopyText("資料📦".into()))
    );
    let cut = properties(&mut fx, &mut snarl, &context, true, vec![egui::Event::Cut]).0;
    assert!(
        cut.platform_output
            .commands
            .contains(&egui::OutputCommand::CopyText("資料📦".into()))
    );
    assert_eq!(table(&fx)[0].0, Value::String(String::new()));
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![egui::Event::Paste("new\nline\r貨".into())],
    );
    assert_eq!(table(&fx)[0].0, Value::String("new line 貨".into()));
    let focused = context
        .memory(|memory| memory.focused())
        .expect("actual text editor owns keyboard input");
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key(egui::Key::Tab)],
    );
    settle(&mut fx, &mut snarl, &context, true);
    let next = context
        .memory(|memory| memory.focused())
        .expect("Tab retains real widget focus");
    assert_ne!(next, focused);
    assert!(context.read_response(next).is_some());
    assert_eq!(table(&fx)[0].1, original_table[0].1);
    assert_eq!(table(&fx)[1], original_table[1]);
    assert_eq!(default(&fx), &Some(Value::Null));
    assert!(matches!(
        fx.graph.nodes[&0],
        Node::ValueMap {
            input_type: Some(ScalarType::Int),
            input: 7,
            ..
        }
    ));
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key(egui::Key::Escape)],
    );
    let closed = settle(&mut fx, &mut snarl, &context, true);
    assert!(positions(&closed, "Default").is_empty());
    let reopened = open(&mut fx, &mut snarl, &context, true);
    assert!(!positions(&reopened, "new line 貨").is_empty());
    assert_eq!(snarl.wires().collect::<Vec<_>>(), wires);
}
