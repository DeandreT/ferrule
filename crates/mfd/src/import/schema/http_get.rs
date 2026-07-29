use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mapping::{
    ExternalHttpHeader, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions,
    FormatOptions, HttpGetOptions, HttpTimeoutSeconds,
};

use crate::resource::ResourceResolver;

use super::{
    ComponentFormat, SchemaComponent, collect_entry_ports, collect_json_ports, entry_key_sets,
    entry_tree_schema, is_default_output, json_entry_value_schema, merge_generic_xml_entries,
    normalize_xml_text_ports, read_xml_schema_file, record_entry_keys, resolve_resource_reference,
};

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    read_resolved(
        component,
        resources.mapping_path(),
        Some(resources),
        warnings,
    )
}

fn read_resolved(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    let call = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("wsdl")))
        .ok_or_else(|| "HTTP component has no call metadata".to_string())?;
    match call.attribute("httpmethod") {
        Some(method) if method.eq_ignore_ascii_case("GET") => {
            read_get(component, mfd_path, resources, warnings)
        }
        Some(method) if method.eq_ignore_ascii_case("POST") => {
            read_post(component, mfd_path, resources, warnings)
        }
        _ => Err("only HTTP GET and captured-response POST calls are supported".to_string()),
    }
}

/// Imports a requestless manual GET whose single response body is typed XML.
fn read_get(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    if component.attribute("kind") != Some("20") {
        return Err("only kind=20 HTTP call components are supported".to_string());
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or_else(|| "HTTP component has no data block".to_string())?;
    let call = data
        .children()
        .find(|node| node.has_tag_name("wsdl"))
        .ok_or_else(|| "HTTP component has no call metadata".to_string())?;
    if call.attribute("kind") != Some("call")
        || call.attribute("sourceMode") != Some("manual")
        || !call
            .attribute("httpmethod")
            .is_some_and(|method| method.eq_ignore_ascii_case("GET"))
    {
        return Err("only requestless manual GET calls are supported".to_string());
    }

    let url = call
        .attribute("url")
        .filter(|url| valid_http_url(url))
        .ok_or_else(|| {
            "manual GET URL must be an HTTP(S) URL without credentials or a fragment".to_string()
        })?;
    let timeout = match call.attribute("timeout") {
        Some(value) => value
            .parse::<u16>()
            .ok()
            .and_then(HttpTimeoutSeconds::new)
            .ok_or_else(|| {
                format!(
                    "HTTP timeout must be between 1 and {} seconds",
                    HttpTimeoutSeconds::MAX
                )
            })?,
        None => HttpTimeoutSeconds::default(),
    };

    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let response_documents = roots
        .iter()
        .flat_map(|root| root.descendants())
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-xml"))
        .filter_map(|entry| {
            entry
                .children()
                .find(|node| node.has_tag_name("document"))
                .map(|document| (entry, document))
        })
        .collect::<Vec<_>>();
    let [(document_entry, document)] = response_documents.as_slice() else {
        return Err("HTTP call must expose exactly one typed XML response body".to_string());
    };
    let response_root = roots
        .iter()
        .copied()
        .find(|root| root.descendants().any(|node| node == *document_entry))
        .ok_or_else(|| "typed XML response is outside a response root".to_string())?;
    for request_root in roots.iter().copied().filter(|root| *root != response_root) {
        let (inputs, _) = entry_key_sets(&request_root);
        if !inputs.is_empty() {
            return Err(
                "dynamic request URL, headers, or body inputs are not supported".to_string(),
            );
        }
    }

    let payload = document_entry
        .children()
        .find(|node| node.has_tag_name("entry"))
        .ok_or_else(|| "typed XML response has no payload root entry".to_string())?;
    let (payload_inputs, payload_outputs) = entry_key_sets(&payload);
    if !payload_inputs.is_empty() {
        return Err("typed XML responses cannot contain request input ports".to_string());
    }
    let (_, response_outputs) = entry_key_sets(&response_root);
    if response_outputs != payload_outputs {
        return Err("response status or header outputs are not supported".to_string());
    }
    if payload_outputs.is_empty() {
        return Err("typed XML response has no output ports".to_string());
    }
    if document
        .attribute("encoding")
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("UTF-8"))
    {
        return Err("only UTF-8 XML responses are supported".to_string());
    }

    let schema_file = document
        .attribute("schemafile")
        .ok_or_else(|| "typed XML response has no schema file".to_string())?;
    let root_name = document
        .attribute("root")
        .and_then(|root| root.rsplit('}').next())
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| payload.attribute("name").unwrap_or("root"));
    let schema =
        resolve_resource_reference(mfd_path, resources, schema_file, "HTTP response XML Schema")
            .and_then(|schema_path| {
                read_xml_schema_file(&schema_path, Some(root_name))
                    .map_err(|error| error.to_string())
            });
    let mut schema = match schema {
        Ok(schema) => schema,
        Err(error) => {
            warnings.push(format!(
                "HTTP component `{name}`: could not read response schema `{schema_file}` ({error}); falling back to the entry tree (no types or reliable repetition)"
            ));
            entry_tree_schema(&payload)
        }
    };
    merge_generic_xml_entries(&payload, &mut schema);

    let mut ports = BTreeMap::new();
    let mut output_count = 0usize;
    let mut input_count = 0usize;
    record_entry_keys(
        &payload,
        &[],
        &mut ports,
        &mut output_count,
        &mut input_count,
    );
    collect_entry_ports(
        &payload,
        &mut Vec::new(),
        &mut ports,
        &mut output_count,
        &mut input_count,
    );
    normalize_xml_text_ports(&schema, &mut ports);

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Xml,
        schema,
        input_instance: Some(url.to_string()),
        output_instance: None,
        options: FormatOptions {
            http_get: Some(HttpGetOptions::new(timeout)),
            xml_document: true,
            ..FormatOptions::default()
        },
        is_source: true,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys: payload_inputs,
        output_keys: payload_outputs,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

/// Imports a POST as a typed captured-response boundary. Request structure is
/// retained for inspection, but ferrule never sends the request or stores
/// header values.
fn read_post(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    if component.attribute("kind") != Some("20") {
        return Err("only kind=20 HTTP call components are supported".to_string());
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or_else(|| "HTTP component has no data block".to_string())?;
    let call = data
        .children()
        .find(|node| node.has_tag_name("wsdl"))
        .ok_or_else(|| "HTTP component has no call metadata".to_string())?;
    if call.attribute("kind") != Some("call") {
        return Err("HTTP POST metadata is not a call".to_string());
    }
    let mode = match call.attribute("sourceMode") {
        Some("manual") => ExternalHttpMode::Manual,
        Some("graphql") => ExternalHttpMode::Graphql,
        _ => return Err("HTTP POST source mode must be manual or GraphQL".to_string()),
    };
    let url = call
        .attribute("url")
        .filter(|url| valid_http_url(url))
        .ok_or_else(|| {
            "HTTP POST URL must be an HTTP(S) URL without credentials or a fragment".to_string()
        })?;
    let timeout = match call.attribute("timeout") {
        Some(value) => value
            .parse::<u16>()
            .ok()
            .and_then(HttpTimeoutSeconds::new)
            .ok_or_else(|| {
                format!(
                    "HTTP timeout must be between 1 and {} seconds",
                    HttpTimeoutSeconds::MAX
                )
            })?,
        None => HttpTimeoutSeconds::default(),
    };

    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let documents = roots
        .iter()
        .flat_map(|root| {
            root.descendants()
                .filter(|entry| {
                    entry.has_tag_name("entry") && entry.attribute("type") == Some("doc-json")
                })
                .filter_map(|entry| {
                    let document = entry
                        .children()
                        .find(|node| node.has_tag_name("document"))?;
                    let payload = entry.children().find(|node| node.has_tag_name("entry"))?;
                    Some((*root, entry, document, payload))
                })
        })
        .collect::<Vec<_>>();
    let response_documents = documents
        .iter()
        .filter(|(root, _, _, _)| !entry_key_sets(root).1.is_empty())
        .collect::<Vec<_>>();
    let [(response_root, _, response_document, response_payload)] = response_documents.as_slice()
    else {
        return Err("HTTP POST must expose exactly one connected JSON response body".to_string());
    };
    ensure_utf8(response_document)?;
    let (response_inputs, response_outputs) = entry_key_sets(response_payload);
    if !response_inputs.is_empty() || response_outputs.is_empty() {
        return Err("HTTP POST response body must expose output ports only".to_string());
    }
    if entry_key_sets(response_root).1 != response_outputs {
        return Err("HTTP POST response status or header outputs are not supported".to_string());
    }

    let request_documents = documents
        .iter()
        .filter(|(root, _, _, _)| !entry_key_sets(root).0.is_empty())
        .collect::<Vec<_>>();
    let (request_format, request_schema) = match request_documents.as_slice() {
        [] => (None, None),
        [(.., document, payload)] => {
            ensure_utf8(document)?;
            (
                Some(ExternalPayloadFormat::Json),
                Some(read_json_request_schema(
                    &name, document, payload, mfd_path, resources, warnings,
                )),
            )
        }
        _ => return Err("HTTP POST must expose at most one JSON request body".to_string()),
    };
    let request_inputs = roots
        .iter()
        .filter(|root| **root != *response_root)
        .flat_map(|root| entry_key_sets(root).0)
        .collect::<BTreeSet<_>>();

    // A captured response is intentionally the connected entry-tree
    // projection. Referenced schemas can be open or substantially broader;
    // importing them would claim fields the graph can never observe.
    let schema = json_entry_value_schema(&name, response_payload);
    let mut ports = BTreeMap::new();
    let mut output_count = 0usize;
    let mut input_count = 0usize;
    record_entry_keys(
        response_payload,
        &[],
        &mut ports,
        &mut output_count,
        &mut input_count,
    );
    collect_json_ports(
        response_payload,
        &mut Vec::new(),
        &mut ports,
        &mut output_count,
        &mut input_count,
        warnings,
    );
    if input_count != 0 || output_count == 0 {
        return Err("HTTP POST response JSON ports are invalid".to_string());
    }

    let headers = call
        .children()
        .filter(|node| node.has_tag_name("parameter") && node.attribute("style") == Some("header"))
        .map(|parameter| {
            ExternalHttpHeader::new(
                parameter.attribute("name").unwrap_or_default(),
                parameter.attribute("required") == Some("1"),
                parameter.attribute("mappable") == Some("1"),
            )
            .map_err(|error| format!("invalid HTTP POST header declaration: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let external_source = ExternalSourceOptions::http_post(
        mode,
        timeout,
        request_format,
        request_schema,
        ExternalPayloadFormat::Json,
        headers,
    )
    .map_err(|error| format!("invalid HTTP POST boundary: {error}"))?;

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Json,
        schema,
        input_instance: Some(url.to_string()),
        output_instance: None,
        options: FormatOptions {
            external_source: Some(external_source),
            ..FormatOptions::default()
        },
        is_source: true,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_keys: request_inputs,
        input_ancestors: BTreeMap::new(),
        output_keys: response_outputs,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

fn read_json_request_schema(
    component_name: &str,
    document: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> ir::SchemaNode {
    document
        .attribute("schemafile")
        .and_then(|relative| {
            let resolved = match resources {
                Some(resources) => resources.resolve_json_schema(relative),
                None => resolve_resource_reference(
                    mfd_path,
                    None,
                    relative,
                    "HTTP request JSON Schema",
                )
                .map(|path| {
                    let root = path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .to_path_buf();
                    (path, root)
                }),
            };
            let (path, root) = match resolved {
                Ok(resolved) => resolved,
                Err(error) => {
                    warnings.push(format!(
                        "HTTP component `{component_name}`: could not read request schema `{relative}` ({error}); falling back to the entry tree"
                    ));
                    return None;
                }
            };
            let imported = match resources {
                Some(_) => format_json::json_schema::import_with_root(&path, &root),
                None => format_json::json_schema::import(&path),
            };
            match imported {
                Ok(schema) => Some(schema),
                Err(error) => {
                    warnings.push(format!(
                        "HTTP component `{component_name}`: could not read request schema `{relative}` ({error}); falling back to the entry tree"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| json_entry_value_schema(component_name, payload))
}

fn ensure_utf8(document: &roxmltree::Node<'_, '_>) -> Result<(), String> {
    if document
        .attribute("encoding")
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("UTF-8"))
    {
        Err("only UTF-8 JSON request and response bodies are supported".to_string())
    } else {
        Ok(())
    }
}

fn valid_http_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest
        .split_once(['/', '?', '#'])
        .map_or(rest, |(authority, _)| authority);
    !authority.is_empty()
        && !authority.contains('@')
        && !url.contains('#')
        && url.is_ascii()
        && !url
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use ir::{ScalarType, SchemaKind};

    use super::{read, read_resolved, valid_http_url};
    use crate::resource::ResourceResolver;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Result<Self, std::io::Error> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-mfd-http-package-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn component(method: &str, request_entry: &str, response_extra: &str) -> String {
        format!(
            r#"<component name="call" library="webservice" kind="20">
                <properties/>
                <data>
                    <root><entry name="HTTPMessage">{request_entry}</entry></root>
                    <root rootindex="1">
                        <entry name="HTTPMessage">
                            {response_extra}
                            <entry name="HTTPBody">
                                <entry name="document" type="doc-xml">
                                    <document schemafile="missing.xsd" root="Feed" encoding="UTF-8"/>
                                    <entry name="Feed" outkey="10"><entry name="Value" outkey="11"/></entry>
                                </entry>
                            </entry>
                        </entry>
                    </root>
                    <wsdl kind="call" sourceMode="manual" url="https://example.test/feed" timeout="20" httpmethod="{method}"/>
                </data>
            </component>"#
        )
    }

    fn rejection(xml: &str) -> String {
        let document = roxmltree::Document::parse(xml).unwrap();
        match read_resolved(
            &document.root_element(),
            Path::new("/tmp/mapping.mfd"),
            None,
            &mut Vec::new(),
        ) {
            Ok(_) => panic!("component should be rejected"),
            Err(error) => error,
        }
    }

    #[test]
    fn static_url_validation_rejects_unsafe_or_non_http_forms() {
        assert!(valid_http_url("https://example.test/feed?limit=2"));
        assert!(valid_http_url("HTTPS://example.test/feed"));
        assert!(valid_http_url("http://127.0.0.1:8080/data"));
        assert!(!valid_http_url("file:///tmp/data.xml"));
        assert!(!valid_http_url("https://user:secret@example.test/data"));
        assert!(!valid_http_url("https://example.test/data#fragment"));
        assert!(!valid_http_url("https:///missing-host"));
    }

    #[test]
    fn unsupported_request_and_response_shapes_are_rejected_before_fallback() {
        let post = component("POST", "", "");
        assert!(rejection(&post).contains("connected JSON response"));

        let dynamic_request = component("GET", r#"<entry name="HTTPHeader" inpkey="4"/>"#, "");
        assert!(rejection(&dynamic_request).contains("dynamic request URL"));

        let response_metadata = component("GET", "", r#"<entry name="HTTPStatus" outkey="5"/>"#);
        assert!(rejection(&response_metadata).contains("status or header outputs"));
    }

    #[test]
    fn post_is_a_typed_captured_response_without_header_values() {
        let xml = r#"<component name="post" library="webservice" kind="20">
          <data>
            <root><entry name="HTTPMessage" inpkey="1"><entry name="Token" inpkey="2"/>
              <entry name="HTTPBody"><entry name="document" type="doc-json">
                <document encoding="UTF-8"/><entry name="root"><entry name="object">
                  <entry name="query" type="json-property"><entry name="string" inpkey="3"/></entry>
                </entry></entry>
              </entry></entry>
            </entry></root>
            <root rootindex="1"><entry name="HTTPMessage"><entry name="HTTPBody">
              <entry name="document" type="doc-json"><document encoding="UTF-8"/>
                <entry name="root"><entry name="object">
                  <entry name="answer" type="json-property"><entry name="string" outkey="10"/></entry>
                </entry></entry>
              </entry>
            </entry></entry></root>
            <wsdl kind="call" sourceMode="manual" url="https://example.test/api" timeout="20" httpmethod="POST">
              <parameter name="Token" value="must-not-be-retained" style="header" required="1" mappable="1"/>
            </wsdl>
          </data>
        </component>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let boundary = read_resolved(
            &document.root_element(),
            Path::new("/tmp/mapping.mfd"),
            None,
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(boundary.ports.get(&10), Some(&vec!["answer".to_string()]));
        assert!(boundary.input_keys.contains(&1));
        let encoded = serde_json::to_string(&boundary.options).unwrap();
        assert!(encoded.contains("Token"));
        assert!(!encoded.contains("must-not-be-retained"));
    }

    #[test]
    fn safe_parent_response_schema_resolves_after_package_relocation()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let original = temp.0.join("original");
        let maps = original.join("maps");
        let schemas = original.join("resources");
        std::fs::create_dir_all(&maps)?;
        std::fs::create_dir_all(&schemas)?;
        std::fs::write(maps.join("mapping.mfd"), "<mapping/>")?;
        std::fs::write(
            schemas.join("Feed.XSD"),
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Feed"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:int" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;

        let relocated = temp.0.join("relocated");
        std::fs::rename(&original, &relocated)?;
        let mapping = relocated.join("maps/mapping.mfd");
        let resolver = ResourceResolver::new(&mapping, Some(&relocated))?;
        let xml = component("GET", "", "").replace("missing.xsd", r"..\resources\feed.xsd");
        let document = roxmltree::Document::parse(&xml)?;
        let mut warnings = Vec::new();

        let imported = read(&document.root_element(), &resolver, &mut warnings)
            .map_err(std::io::Error::other)?;

        assert!(warnings.is_empty(), "{warnings:?}");
        let value = imported
            .schema
            .child("Value")
            .ok_or("HTTP response schema lost Value")?;
        assert!(value.repeating);
        assert!(matches!(
            value.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        Ok(())
    }

    #[test]
    fn post_request_schema_keeps_transitive_references_inside_package()
    -> Result<(), Box<dyn std::error::Error>> {
        let package = TempDir::new()?;
        let maps = package.0.join("maps");
        let schemas = package.0.join("resources");
        let definitions = schemas.join("definitions");
        std::fs::create_dir_all(&maps)?;
        std::fs::create_dir_all(&definitions)?;
        let mapping = maps.join("mapping.mfd");
        std::fs::write(&mapping, "<mapping/>")?;
        std::fs::write(
            schemas.join("request.schema.json"),
            r#"{"$ref":"definitions/request.json"}"#,
        )?;
        std::fs::write(
            definitions.join("request.json"),
            r#"{"type":"object","properties":{"query":{"type":"integer"}}}"#,
        )?;
        let resolver = ResourceResolver::new(&mapping, Some(&package.0))?;
        let xml = r#"<component name="post" library="webservice" kind="20">
          <data>
            <root><entry name="HTTPMessage"><entry name="HTTPBody">
              <entry name="document" type="doc-json">
                <document schemafile="../resources/request.schema.json" encoding="UTF-8"/>
                <entry name="root"><entry name="object">
                  <entry name="query" type="json-property"><entry name="integer" inpkey="3"/></entry>
                </entry></entry>
              </entry>
            </entry></entry></root>
            <root rootindex="1"><entry name="HTTPMessage"><entry name="HTTPBody">
              <entry name="document" type="doc-json"><document encoding="UTF-8"/>
                <entry name="root"><entry name="object">
                  <entry name="answer" type="json-property"><entry name="string" outkey="10"/></entry>
                </entry></entry>
              </entry>
            </entry></entry></root>
            <wsdl kind="call" sourceMode="manual" url="https://example.test/api"
              timeout="20" httpmethod="POST"/>
          </data>
        </component>"#;
        let document = roxmltree::Document::parse(xml)?;
        let mut warnings = Vec::new();

        let imported = read(&document.root_element(), &resolver, &mut warnings)
            .map_err(std::io::Error::other)?;

        assert!(warnings.is_empty(), "{warnings:?}");
        let boundary = imported
            .options
            .external_source
            .as_ref()
            .ok_or("HTTP POST lost its external source metadata")?;
        let mapping::ExternalSourceOrigin::HttpPost {
            request_schema: Some(request_schema),
            ..
        } = boundary.origin()
        else {
            return Err("HTTP POST lost its request schema".into());
        };
        let query = request_schema
            .child("query")
            .ok_or("HTTP request schema lost query")?;
        assert!(matches!(
            query.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn response_schema_symlink_escape_falls_back_with_a_package_warning()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let package = TempDir::new()?;
        let outside = TempDir::new()?;
        let maps = package.0.join("maps");
        let schemas = package.0.join("resources");
        std::fs::create_dir_all(&maps)?;
        std::fs::create_dir_all(&schemas)?;
        let mapping = maps.join("mapping.mfd");
        std::fs::write(&mapping, "<mapping/>")?;
        let external = outside.0.join("feed.xsd");
        std::fs::write(
            &external,
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Feed"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:int" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;
        symlink(&external, schemas.join("feed.xsd"))?;
        let resolver = ResourceResolver::new(&mapping, Some(&package.0))?;
        let xml = component("GET", "", "").replace("missing.xsd", "../resources/feed.xsd");
        let document = roxmltree::Document::parse(&xml)?;
        let mut warnings = Vec::new();

        let imported = read(&document.root_element(), &resolver, &mut warnings)
            .map_err(std::io::Error::other)?;

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("resolves outside package root"));
        let value = imported
            .schema
            .child("Value")
            .ok_or("HTTP fallback schema lost Value")?;
        assert!(!value.repeating);
        assert!(matches!(
            value.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
        Ok(())
    }
}
