use super::*;

fn expanded(namespace: Option<&str>, name: &str) -> String {
    format!("{{{}}}{name}", namespace.unwrap_or(""))
}

fn xml_attr(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            c => c.to_string(),
        })
        .collect()
}

fn boundary_xml(
    schema: &SchemaNode,
    role: Role,
    uid: u32,
    allocated: &AllocatedPlan,
    selected: &str,
    schema_file: &str,
    instance_path: &str,
) -> String {
    let root_ns = namespace(&schema.xml_namespace);
    let slot = |ns: &Option<XmlNamespace>| if namespace(ns).is_some() { 2 } else { 0 };
    let headers = format!(
        "<namespace/><namespace uid=\"http://www.altova.com/mapforce\"/>{}",
        root_ns.as_deref().map_or_else(String::new, |uri| format!(
            "<namespace uid=\"{}\"/>",
            xml_attr(uri)
        ))
    );
    let field_xml = |field: &SchemaNode, keyed: bool| {
        let key = keyed
            .then(|| {
                allocated
                    .leaves
                    .get(&leaf(role, uid, schema, selected, field))
            })
            .flatten();
        let pin = key.map_or_else(String::new, |key| {
            format!(
                " {}=\"{key}\"",
                if role == Role::Source {
                    "outkey"
                } else {
                    "inpkey"
                }
            )
        });
        format!(
            "<entry name=\"{}\" ns=\"{}\" type=\"attribute\"{pin}/>",
            xml_attr(&field.name),
            slot(&field.xml_namespace)
        )
    };
    let base = schema
        .alternatives()
        .iter()
        .find(|a| Some(&a.name) == schema.xml_default_type.as_ref())
        .unwrap();
    let selected_members = schema
        .alternatives()
        .iter()
        .find(|a| a.name == selected)
        .unwrap();
    let members = |names: &[String], keyed: bool| {
        names
            .iter()
            .map(|name| field_xml(schema.child(name).unwrap(), keyed))
            .collect::<String>()
    };
    let root_name = xml_attr(&schema.name);
    let root_slot = slot(&schema.xml_namespace);
    let root_expanded = xml_attr(&expanded(root_ns.as_deref(), &schema.name));
    let raw_type = if selected.starts_with('{') {
        selected.into()
    } else {
        expanded(None, selected)
    };
    let io = format!(
        "{}instance=\"{}\"",
        if role == Role::Source {
            "input"
        } else {
            "output"
        },
        xml_attr(instance_path)
    );
    let file = xml_attr(schema_file);
    format!(
        "<component name=\"{root_name}\" uid=\"{uid}\" library=\"xml\" kind=\"14\"><data><root><header><namespaces>{headers}</namespaces></header><entry name=\"FileInstance\" ns=\"1\"><entry name=\"document\" ns=\"1\"><entry name=\"{root_name}\" ns=\"{root_slot}\" displayselectionmode=\"all\">{}</entry><entry name=\"{root_name}\" ns=\"{root_slot}\"><condition><expression><function name=\"equal\" library=\"core\"><expression><attribute name=\"type\" ns=\"http://www.w3.org/2001/XMLSchema-instance\"/></expression><expression><constant value=\"{}\" datatype=\"QName\"/></expression></function></expression></condition>{}</entry></entry></entry></root><document schema=\"{file}\" instanceroot=\"{root_expanded}\" {io}/></data></component>",
        members(&base.members, false),
        xml_attr(&raw_type),
        members(&selected_members.members, true)
    )
}
pub(super) fn mapping_xml(
    project: &Project,
    plan: &RootTypeViewPlan,
    allocated: &AllocatedPlan,
    source_schema: &str,
    target_schema: &str,
) -> String {
    let mut grouped = BTreeMap::<u32, Vec<u32>>::new();
    for (source, target) in &allocated.edges {
        grouped.entry(*source).or_default().push(*target);
    }
    let vertices = grouped
        .iter()
        .map(|(source, targets)| {
            format!(
                "<vertex vertexkey=\"{source}\"><edges>{}</edges></vertex>",
                targets
                    .iter()
                    .map(|target| format!("<edge vertexkey=\"{target}\"/>"))
                    .collect::<String>()
            )
        })
        .collect::<String>();
    format!(
        "<mapping version=\"22\"><component name=\"Mapping\" uid=\"101\"><structure><children>{}{}</children><graph directed=\"1\"><vertices>{vertices}</vertices></graph></structure></component></mapping>",
        boundary_xml(
            &project.source,
            Role::Source,
            211,
            allocated,
            &plan.canonical_type,
            source_schema,
            project.source_path.as_deref().unwrap(),
        ),
        boundary_xml(
            &project.target,
            Role::Target,
            307,
            allocated,
            &plan.canonical_type,
            target_schema,
            project.target_path.as_deref().unwrap(),
        )
    )
}
