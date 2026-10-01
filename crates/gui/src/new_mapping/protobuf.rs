use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use mapping::{FormatOptions, ProtobufOptions, ProtobufSchemaFile};

/// A schema graph is loaded once; choosing a root changes only its projection.
pub(crate) struct ProtobufBoundaryDraft {
    pub(crate) schema_path: PathBuf,
    pub(crate) instance_path: String,
    pub(crate) root_message: String,
    pub(crate) root_messages: Vec<String>,
    pub(crate) selection_error: Option<String>,
    layout: format_protobuf::Layout,
    bundle: format_protobuf::SchemaBundle,
    schema: Option<ir::SchemaNode>,
}

impl ProtobufBoundaryDraft {
    pub(crate) fn from_schema(path: PathBuf) -> anyhow::Result<Self> {
        if !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("proto"))
        {
            bail!("Protocol Buffers schema must have a .proto extension");
        }
        let root_path = path
            .file_name()
            .and_then(|value| value.to_str())
            .context("Protocol Buffers schema filename must be valid UTF-8")?;
        let base = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let bundle = format_protobuf::SchemaBundle::read_relative(base, root_path)
            .context("loading Protocol Buffers schema and local imports")?;
        let layout = bundle.layout().context("parsing Protocol Buffers schema")?;
        let root_messages = layout
            .messages()
            .iter()
            .filter(|message| !message.is_map_entry())
            .map(|message| message.full_name().to_owned())
            .collect::<Vec<_>>();
        if root_messages.is_empty() {
            bail!("Protocol Buffers schema has no selectable message types");
        }
        Ok(Self {
            schema_path: path,
            instance_path: String::new(),
            root_message: String::new(),
            root_messages,
            selection_error: None,
            layout,
            bundle,
            schema: None,
        })
    }

    pub(crate) fn set_root_message(&mut self, root: String) {
        self.root_message = root;
        self.schema = None;
        self.selection_error = None;
        let result = (|| {
            if !self.root_messages.contains(&self.root_message) {
                bail!("choose a root message from this schema");
            }
            format_protobuf::to_ir_schema(&self.layout, &self.root_message)
                .context("the selected Protocol Buffers root cannot be mapped")
        })();
        match result {
            Ok(schema) => self.schema = Some(schema),
            Err(error) => self.selection_error = Some(format!("{error:#}")),
        }
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if self.root_message.is_empty() {
            bail!("choose a Protocol Buffers root message");
        }
        if let Some(error) = &self.selection_error {
            bail!("{error}");
        }
        if self.schema.is_none() {
            bail!("choose a supported Protocol Buffers root message");
        }
        if self.instance_path.as_bytes().contains(&0) {
            bail!("Protocol Buffers file path cannot contain a null character");
        }
        Ok(())
    }

    pub(crate) fn schema(&self) -> anyhow::Result<ir::SchemaNode> {
        self.validate()?;
        self.schema
            .clone()
            .context("choose a Protocol Buffers root message")
    }

    pub(crate) fn options(&self) -> anyhow::Result<FormatOptions> {
        self.validate()?;
        Ok(FormatOptions {
            protobuf: Some(ProtobufOptions {
                schema: self.bundle.root_source().to_owned(),
                root_message: self.root_message.clone(),
                schema_path: (!self.bundle.imports().is_empty())
                    .then(|| self.bundle.root_path().to_owned()),
                imports: self
                    .bundle
                    .imports()
                    .iter()
                    .map(|file| ProtobufSchemaFile {
                        path: file.path().to_owned(),
                        source: file.source().to_owned(),
                    })
                    .collect(),
            }),
            ..FormatOptions::default()
        })
    }

    pub(crate) fn instance_path(&self) -> Option<String> {
        let path = self.instance_path.trim();
        (!path.is_empty()).then(|| path.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> anyhow::Result<Self> {
            let directory = std::env::temp_dir().join(format!(
                "ferrule-gui-protobuf-draft-{}-{}",
                std::process::id(),
                NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&directory)?;
            Ok(Self(directory))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn root_selection_embeds_local_imports_and_survives_schema_removal() -> anyhow::Result<()> {
        let directory = TestDirectory::new()?;
        std::fs::create_dir_all(directory.0.join("types"))?;
        let schema_path = directory.0.join("messages.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; package demo;
import "types/address.proto";
message Person { string name = 1; shared.Address address = 2; }
message Unused { bool enabled = 1; }"#,
        )?;
        std::fs::write(
            directory.0.join("types/address.proto"),
            r#"syntax = "proto3"; package shared; message Address { string city = 1; }"#,
        )?;
        let mut draft = ProtobufBoundaryDraft::from_schema(schema_path.clone())?;
        assert!(draft.validate().is_err(), "root selection is required");
        assert!(draft.root_messages.iter().any(|name| name == "demo.Person"));
        draft.set_root_message("demo.Person".to_owned());
        let schema = draft.schema()?;
        assert_eq!(schema.name, "Person");
        assert!(
            schema
                .child("address")
                .and_then(|node| node.child("city"))
                .is_some()
        );
        let options = draft.options()?;
        let protobuf = options.protobuf.unwrap();
        assert_eq!(protobuf.root_message, "demo.Person");
        assert_eq!(protobuf.schema_path.as_deref(), Some("messages.proto"));
        assert_eq!(protobuf.imports.len(), 1);
        assert_eq!(protobuf.imports[0].path, "types/address.proto");
        std::fs::remove_file(schema_path)?;
        std::fs::remove_file(directory.0.join("types/address.proto"))?;
        let embedded: ProtobufOptions = serde_json::from_slice(&serde_json::to_vec(&protobuf)?)?;
        let layout = format_protobuf::Layout::parse_files(
            embedded.schema_path.as_deref().unwrap_or("schema.proto"),
            &embedded.schema,
            embedded
                .imports
                .iter()
                .map(|file| (file.path.as_str(), file.source.as_str())),
        )?;
        assert_eq!(
            format_protobuf::to_ir_schema(&layout, &embedded.root_message)?,
            schema
        );
        Ok(())
    }

    #[test]
    fn unsupported_or_unknown_roots_clear_the_previous_valid_projection() -> anyhow::Result<()> {
        let directory = TestDirectory::new()?;
        let schema_path = directory.0.join("messages.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; package demo;
message Good { string name = 1; }
message Recursive { Recursive next = 1; }"#,
        )?;
        let mut draft = ProtobufBoundaryDraft::from_schema(schema_path)?;
        draft.set_root_message("demo.Good".to_owned());
        assert!(draft.schema().is_ok());
        draft.set_root_message("demo.Recursive".to_owned());
        assert!(draft.schema().is_err());
        assert!(draft.options().is_err());
        assert!(
            draft
                .selection_error
                .as_deref()
                .is_some_and(|error| error.contains("recurs"))
        );
        draft.set_root_message("demo.Missing".to_owned());
        assert!(draft.schema().is_err());
        draft.set_root_message("demo.Good".to_owned());
        assert!(draft.validate().is_ok());
        assert!(draft.selection_error.is_none());
        Ok(())
    }

    #[test]
    fn imports_are_confined_to_the_selected_schema_directory() -> anyhow::Result<()> {
        let directory = TestDirectory::new()?;
        std::fs::create_dir_all(directory.0.join("schema"))?;
        std::fs::write(
            directory.0.join("outside.proto"),
            r#"syntax = "proto3"; message Outside { string name = 1; }"#,
        )?;
        let schema_path = directory.0.join("schema/messages.proto");
        std::fs::write(
            &schema_path,
            r#"syntax = "proto3"; import "../outside.proto"; message Item { Outside value = 1; }"#,
        )?;
        let error = match ProtobufBoundaryDraft::from_schema(schema_path) {
            Ok(_) => panic!("escaping imports must be rejected"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("path"));
        Ok(())
    }
}
