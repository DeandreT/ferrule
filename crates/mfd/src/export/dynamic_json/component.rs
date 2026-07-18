use std::fmt::Write as _;
use std::path::Path;

use ir::SchemaNode;

use super::super::schema::{GeneratedSibling, RenderedSchemaComponent, xml_escape};

pub(super) enum JsonSide {
    Source,
    Target { default_output: bool },
}

pub(super) struct JsonComponentArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) entries: String,
    pub(super) side: JsonSide,
    pub(super) instance_path: Option<&'a str>,
    pub(super) json_lines: bool,
    pub(super) mfd_path: &'a Path,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
}

pub(super) fn render_json_component(args: JsonComponentArgs<'_>) -> RenderedSchemaComponent {
    let stem = args
        .mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let dir = args.mfd_path.parent().unwrap_or(Path::new("."));
    let schema_file = format!("{stem}-{}.schema.json", args.sibling_suffix);
    let (header, view, instance_attr) = match args.side {
        JsonSide::Source => ("", "<view rbx=\"300\" rby=\"400\"/>", "inputinstance"),
        JsonSide::Target { default_output } => (
            if default_output {
                "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
            } else {
                ""
            },
            "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
            "outputinstance",
        ),
    };
    let instance = args
        .instance_path
        .map(|path| format!(" {instance_attr}=\"{}\"", xml_escape(path)))
        .unwrap_or_default();
    let json_lines = if args.json_lines
        || args
            .instance_path
            .and_then(|path| Path::new(path).extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("jsonl") || extension.eq_ignore_ascii_case("ndjson")
            }) {
        " jsonlines=\"1\""
    } else {
        ""
    };
    let mut xml = String::new();
    let _ = write!(
        xml,
        "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{}\" kind=\"31\">\n\
         \t\t\t\t\t{header}{view}\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"root\" expanded=\"1\">\n\
         {}\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<json schema=\"{}\"{instance}{json_lines}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        args.entries,
        xml_escape(&schema_file),
    );
    RenderedSchemaComponent {
        xml,
        siblings: vec![GeneratedSibling {
            path: dir.join(&schema_file),
            contents: format_json::json_schema::export(args.schema),
        }],
    }
}
