//! Schemas past the import depth limit are refused with a typed error instead
//! of overflowing the stack. Each check runs on a thread with the 8 MiB stack
//! of a default main thread, in whatever build the tests use.

use std::fmt::Write as _;

use super::super::super::*;

#[derive(Clone, Copy, Debug)]
enum Shape {
    /// `Input` nests anonymous complex types.
    Nested,
    /// `Input` reaches `xs:string` through named types, each holding one
    /// element of the next.
    NamedTypes,
    /// Each named type extends the next.
    Extensions,
    /// Each named model group references the next.
    ModelGroups,
    /// Each named attribute group references the next.
    AttributeGroups,
    /// Each global element holds a reference to the next.
    ElementReferences,
    /// Each named type restricts the next.
    Restrictions,
    /// `Input` nests choices.
    Choices,
}

impl Shape {
    const ALL: [Self; 8] = [
        Self::Nested,
        Self::NamedTypes,
        Self::Extensions,
        Self::ModelGroups,
        Self::AttributeGroups,
        Self::ElementReferences,
        Self::Restrictions,
        Self::Choices,
    ];

    /// Ten times the depth at which the unbounded import overflowed an
    /// 8 MiB stack in a release build.
    fn past_overflow(self) -> usize {
        10 * match self {
            Self::Nested => 660,
            Self::NamedTypes => 600,
            Self::Extensions | Self::Restrictions => 1_200,
            Self::ModelGroups => 1_900,
            Self::AttributeGroups => 4_800,
            Self::ElementReferences => 480,
            Self::Choices => 2_500,
        }
    }

    /// The deepest chain within [`MAX_IMPORT_DEPTH`], from the levels each
    /// link and the chain's ends take.
    fn deepest_within_limit(self) -> usize {
        let (per_link, fixed) = match self {
            // Complex type content, sequence and element per link; the root.
            Self::Nested => (3, 1),
            // Declaration, content, sequence and element per type; the root
            // and the last type's four levels.
            Self::NamedTypes => (4, 5),
            // Declaration and content per base; the root, the last type's
            // declaration, content and sequence, and its element.
            Self::Extensions | Self::Restrictions => (2, 5),
            // Group and sequence per reference; the root, its content and
            // sequence, the last group's two levels and its element.
            Self::ModelGroups => (2, 6),
            // One per reference; the root, its content and the last group.
            Self::AttributeGroups => (1, 3),
            // Particle, declaration, content and sequence per element; the
            // root, its content and sequence, and the last two levels.
            Self::ElementReferences => (4, 5),
            // One per choice; the root, its content and the element.
            Self::Choices => (1, 3),
        };
        (MAX_IMPORT_DEPTH - fixed) / per_link
    }

    fn schema(self, depth: usize) -> String {
        let mut xsd = String::from(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">"#);
        match self {
            Self::Nested => {
                xsd.push_str(r#"<xs:element name="Input">"#);
                for index in 0..depth {
                    write!(
                        xsd,
                        r#"<xs:complexType><xs:sequence><xs:element name="L{index}">"#
                    )
                    .unwrap();
                }
                xsd.push_str(
                    r#"<xs:simpleType><xs:restriction base="xs:string"/></xs:simpleType>"#,
                );
                xsd.push_str(&"</xs:element></xs:sequence></xs:complexType>".repeat(depth));
                xsd.push_str("</xs:element>");
            }
            Self::NamedTypes => {
                xsd.push_str(r#"<xs:element name="Input" type="t0"/>"#);
                for index in 0..depth {
                    write!(
                        xsd,
                        r#"<xs:complexType name="t{index}"><xs:sequence><xs:element name="Value" type="t{}"/></xs:sequence></xs:complexType>"#,
                        index + 1
                    )
                    .unwrap();
                }
                write!(xsd, r#"<xs:complexType name="t{depth}"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType>"#).unwrap();
            }
            Self::Extensions | Self::Restrictions => {
                xsd.push_str(r#"<xs:element name="Input" type="t0"/>"#);
                for index in 0..depth {
                    let derivation = match self {
                        Self::Extensions => format!(r#"<xs:extension base="t{}"/>"#, index + 1),
                        _ => format!(
                            r#"<xs:restriction base="t{}"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:restriction>"#,
                            index + 1
                        ),
                    };
                    write!(
                        xsd,
                        r#"<xs:complexType name="t{index}"><xs:complexContent>{derivation}</xs:complexContent></xs:complexType>"#
                    )
                    .unwrap();
                }
                write!(xsd, r#"<xs:complexType name="t{depth}"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType>"#).unwrap();
            }
            Self::ModelGroups => {
                xsd.push_str(r#"<xs:element name="Input"><xs:complexType><xs:sequence><xs:group ref="g0"/></xs:sequence></xs:complexType></xs:element>"#);
                for index in 0..depth {
                    write!(
                        xsd,
                        r#"<xs:group name="g{index}"><xs:sequence><xs:group ref="g{}"/></xs:sequence></xs:group>"#,
                        index + 1
                    )
                    .unwrap();
                }
                write!(xsd, r#"<xs:group name="g{depth}"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:group>"#).unwrap();
            }
            Self::AttributeGroups => {
                xsd.push_str(r#"<xs:element name="Input"><xs:complexType><xs:attributeGroup ref="a0"/></xs:complexType></xs:element>"#);
                for index in 0..depth {
                    write!(
                        xsd,
                        r#"<xs:attributeGroup name="a{index}"><xs:attributeGroup ref="a{}"/></xs:attributeGroup>"#,
                        index + 1
                    )
                    .unwrap();
                }
                write!(xsd, r#"<xs:attributeGroup name="a{depth}"><xs:attribute name="value" type="xs:string"/></xs:attributeGroup>"#).unwrap();
            }
            Self::ElementReferences => {
                xsd.push_str(r#"<xs:element name="Input"><xs:complexType><xs:sequence><xs:element ref="e0"/></xs:sequence></xs:complexType></xs:element>"#);
                for index in 0..depth {
                    write!(
                        xsd,
                        r#"<xs:element name="e{index}"><xs:complexType><xs:sequence><xs:element ref="e{}"/></xs:sequence></xs:complexType></xs:element>"#,
                        index + 1
                    )
                    .unwrap();
                }
                write!(xsd, r#"<xs:element name="e{depth}" type="xs:string"/>"#).unwrap();
            }
            Self::Choices => {
                xsd.push_str(r#"<xs:element name="Input"><xs:complexType>"#);
                xsd.push_str(&"<xs:choice>".repeat(depth));
                xsd.push_str(r#"<xs:element name="Value" type="xs:string"/>"#);
                xsd.push_str(&"</xs:choice>".repeat(depth));
                xsd.push_str("</xs:complexType></xs:element>");
            }
        }
        xsd.push_str("</xs:schema>");
        xsd
    }

    fn import(self, depth: usize) -> Result<SchemaNode, XmlFormatError> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_depth_{self:?}_{depth}_{}.xsd",
            std::process::id()
        ));
        std::fs::write(&path, self.schema(depth)).unwrap();
        on_main_thread_stack(move || {
            let result = import(&path);
            std::fs::remove_file(&path).unwrap();
            result
        })
    }
}

fn on_main_thread_stack<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap()
}

fn is_depth_limit(result: &Result<SchemaNode, XmlFormatError>) -> bool {
    matches!(
        result,
        Err(XmlFormatError::SchemaDepthLimit {
            limit: MAX_IMPORT_DEPTH
        })
    )
}

#[test]
fn schemas_far_past_the_overflow_depth_are_refused() {
    for shape in Shape::ALL {
        let depth = shape.past_overflow();
        let result = shape.import(depth);
        assert!(is_depth_limit(&result), "{shape:?} at {depth}: {result:?}");
    }
}

#[test]
fn the_deepest_schemas_within_the_limit_still_import() {
    for shape in Shape::ALL {
        let depth = shape.deepest_within_limit();
        let result = shape.import(depth);
        assert!(result.is_ok(), "{shape:?} at {depth}: {result:?}");
        let result = shape.import(depth + 1);
        assert!(
            is_depth_limit(&result),
            "{shape:?} at {}: {result:?}",
            depth + 1
        );
    }
}

#[test]
fn the_depth_limit_is_reported_before_other_refusals() {
    let optional_pair = r#"<xs:sequence minOccurs="0"><xs:element name="A" type="xs:string"/><xs:element name="B" type="xs:string"/></xs:sequence>"#;
    let schema = |depth: usize| {
        let mut xsd = String::from(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence>"#,
        );
        xsd.push_str(optional_pair);
        xsd.push_str(r#"<xs:element name="Deep">"#);
        xsd.push_str(&"<xs:complexType><xs:sequence><xs:element name=\"Level\">".repeat(depth));
        xsd.push_str(&"</xs:element></xs:sequence></xs:complexType>".repeat(depth));
        xsd.push_str("</xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>");
        xsd
    };
    let import_text = |xsd: String| {
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_depth_precedence_{}_{}.xsd",
            xsd.len(),
            std::process::id()
        ));
        std::fs::write(&path, xsd).unwrap();
        on_main_thread_stack(move || {
            let result = import(&path);
            std::fs::remove_file(&path).unwrap();
            result
        })
    };
    // The optional pair alone is refused for its own reason.
    assert!(matches!(
        import_text(schema(1)),
        Err(XmlFormatError::UnsupportedOptionalSequence { element_count: 2 })
    ));
    let result = import_text(schema(MAX_IMPORT_DEPTH));
    assert!(is_depth_limit(&result), "{result:?}");
}

#[test]
fn named_type_and_byte_reader_imports_share_the_limit() {
    let shape = Shape::NamedTypes;
    let xsd = shape.schema(shape.past_overflow());
    let directory = std::env::temp_dir().join(format!(
        "ferrule_xsd_depth_entry_points_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("chain.xsd");
    std::fs::write(&path, &xsd).unwrap();
    let results = on_main_thread_stack(move || {
        struct Bytes(std::sync::Arc<[u8]>);
        impl XmlSchemaByteReader for Bytes {
            fn read(&self, _resource: &Path) -> std::io::Result<std::sync::Arc<[u8]>> {
                Ok(self.0.clone())
            }
        }
        let root = std::fs::canonicalize(&directory).unwrap();
        let canonical = root.join("chain.xsd");
        let bytes: std::rc::Rc<dyn XmlSchemaByteReader> =
            std::rc::Rc::new(Bytes(xsd.into_bytes().into()));
        let results = [
            import_type(&path, "t0"),
            import_root_with_resource_root(&path, Some("Input"), &directory),
            import_root_with_byte_reader(&canonical, Some("Input"), &root, bytes.clone()),
            import_type_with_byte_reader(&canonical, "t0", &root, bytes),
        ];
        std::fs::remove_dir_all(&directory).unwrap();
        results
    });
    for result in &results {
        assert!(is_depth_limit(result), "{result:?}");
    }
}
