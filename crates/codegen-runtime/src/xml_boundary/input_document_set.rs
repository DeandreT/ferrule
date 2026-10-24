//! Static named inputs and dynamic primary XML members retain exclusive phase ownership.
//! Owned libraries also read whole XML document sets: every supplied member
//! keeps its path and resolved location and owns its own input failures.
use super::{
    XmlBoundaryError, XmlBoundaryErrorKind, XmlDocumentExecutionError, XmlDocumentOutputOwner,
    XmlExecutionError, XmlInputSetBudget, XmlInputSource, check_document_size,
    parse_structured_xml, parse_structured_xml_bytes,
};
use crate::{
    BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicSourceRoots, DocumentMember, Instance,
    OwnedSourceLimits, RuntimeError,
};
use std::error::Error;
use std::fmt;

/// Exactly one execution phase, or no owner for a global refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmlInputDocumentOwner {
    Input(XmlInputSource),
    Member(XmlDocumentOutputOwner),
}

/// Original XML boundary allocation and its complete standard typed source chain.
#[derive(Debug)]
#[non_exhaustive]
pub struct XmlInputDocumentExecutionError {
    pub owner: Option<XmlInputDocumentOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlInputDocumentExecutionError {
    /// The static input collector forwards its exact owner and original boundary.
    /// This accepts no old output/request fields and is not a conversion from
    /// arbitrary XmlExecutionError values.
    pub fn from_input_boundary(
        input: Option<XmlInputSource>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        Self {
            owner: input.map(XmlInputDocumentOwner::Input),
            boundary,
        }
    }
}

impl From<XmlBoundaryError> for XmlInputDocumentExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
        }
    }
}

impl From<XmlDocumentExecutionError> for XmlInputDocumentExecutionError {
    fn from(error: XmlDocumentExecutionError) -> Self {
        Self {
            owner: error.member.map(XmlInputDocumentOwner::Member),
            boundary: error.boundary,
        }
    }
}

impl fmt::Display for XmlInputDocumentExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlInputDocumentOwner::Input(input)) => write!(f, "XML input {input:?}: ")?,
            Some(XmlInputDocumentOwner::Member(member)) => write!(
                f,
                "XML output {:?} member {} `{}`: ",
                member.target, member.index, member.path
            )?,
            None => {}
        }
        self.boundary.fmt(f)
    }
}

impl Error for XmlInputDocumentExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

/// One host-supplied member of an XML document-set source: the portable
/// path and optional resolved location a local file-set reader records, and
/// the member's XML text (`str`) or UTF-8 bytes (`[u8]`).
#[derive(Debug, PartialEq, Eq)]
pub struct XmlDocumentSetMember<'a, D: ?Sized = str> {
    pub path: &'a str,
    pub resolved_source_path: Option<&'a str>,
    pub document: &'a D,
}

/// One supplied named document-set source and its ordered members.
#[derive(Debug, PartialEq, Eq)]
pub struct NamedXmlDocumentSet<'a, D: ?Sized = str> {
    pub name: &'a str,
    pub members: &'a [XmlDocumentSetMember<'a, D>],
}

// Both hold only shared references, so they copy for every document type.
impl<D: ?Sized> Clone for XmlDocumentSetMember<'_, D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D: ?Sized> Copy for XmlDocumentSetMember<'_, D> {}
impl<D: ?Sized> Clone for NamedXmlDocumentSet<'_, D> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<D: ?Sized> Copy for NamedXmlDocumentSet<'_, D> {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for str {}
    impl Sealed for [u8] {}
}

/// Member text or UTF-8 bytes, read through the closed structured XML profile.
pub trait XmlDocumentSetText: sealed::Sealed {
    #[doc(hidden)]
    fn byte_len(&self) -> usize;
    #[doc(hidden)]
    fn parse_structured(&self, descriptor: &str) -> Result<Instance, XmlBoundaryError>;
}

impl XmlDocumentSetText for str {
    fn byte_len(&self) -> usize {
        self.len()
    }

    fn parse_structured(&self, descriptor: &str) -> Result<Instance, XmlBoundaryError> {
        parse_structured_xml(descriptor, self)
    }
}

impl XmlDocumentSetText for [u8] {
    fn byte_len(&self) -> usize {
        self.len()
    }

    fn parse_structured(&self, descriptor: &str) -> Result<Instance, XmlBoundaryError> {
        parse_structured_xml_bytes(descriptor, self)
    }
}

/// The supplied member a failure belongs to: its source, its zero-based index
/// in that source's supplied list, and its supplied path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDocumentSetMemberOwner {
    pub source: XmlInputSource,
    pub index: usize,
    pub path: String,
}

/// A refusal or failure of the owned XML document-set entry points. Name
/// admission, member-count and mapping failures belong to no member.
#[derive(Debug)]
#[non_exhaustive]
pub struct XmlDocumentSetInputError {
    pub member: Option<XmlDocumentSetMemberOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlDocumentSetInputError {
    fn member(
        source: XmlInputSource,
        index: usize,
        path: &str,
        boundary: XmlBoundaryError,
    ) -> Self {
        Self {
            member: Some(XmlDocumentSetMemberOwner {
                source,
                index,
                path: path.to_owned(),
            }),
            boundary: Box::new(boundary),
        }
    }
}

impl From<RuntimeError> for XmlDocumentSetInputError {
    fn from(error: RuntimeError) -> Self {
        Self {
            member: None,
            boundary: Box::new(XmlBoundaryError::from(error)),
        }
    }
}

impl From<XmlExecutionError> for XmlDocumentSetInputError {
    fn from(error: XmlExecutionError) -> Self {
        Self {
            member: None,
            boundary: error.boundary,
        }
    }
}

impl fmt::Display for XmlDocumentSetInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(member) = &self.member {
            write!(
                f,
                "XML input {:?} member {} `{}`: ",
                member.source, member.index, member.path
            )?;
        }
        self.boundary.fmt(f)
    }
}

impl Error for XmlDocumentSetInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

const MEMBER_SHAPE: &str =
    "XML document-set members require a non-empty path and a non-empty resolved location";

/// Admits supplied source names exactly as the typed Basic entry points do,
/// before any member is read. Admission never inspects a source tree, so
/// empty placeholders stand in for the sets that are parsed afterwards.
pub fn admit_xml_document_set_names<'n>(
    catalogue: &'static [BasicNamedSourceDeclaration],
    names: impl IntoIterator<Item = &'n str>,
    limits: OwnedSourceLimits,
) -> Result<(), XmlDocumentSetInputError> {
    let empty = Instance::DocumentSet(Vec::new());
    let supplied = names.into_iter().map(|name| BasicNamedSourceInput {
        name,
        instance: &empty,
    });
    BasicSourceRoots::try_from_named_inputs(&empty, catalogue, supplied, limits)?;
    Ok(())
}

/// Admits every member of every supplied set before any member is parsed:
/// the total member count, then each member's path and document size in
/// supplied order, then the aggregate bytes of the whole input set.
pub fn preflight_xml_document_sets<D: XmlDocumentSetText + ?Sized>(
    sets: &[(XmlInputSource, &[XmlDocumentSetMember<'_, D>])],
) -> Result<(), XmlDocumentSetInputError> {
    let count = sets.iter().fold(0_usize, |count, (_, members)| {
        count.saturating_add(members.len())
    });
    let mut budget = XmlInputSetBudget::new(count)?;
    for &(source, members) in sets {
        for (index, member) in members.iter().enumerate() {
            if member.path.is_empty() || member.resolved_source_path == Some("") {
                return Err(XmlDocumentSetInputError::member(
                    source,
                    index,
                    member.path,
                    XmlBoundaryError::detail(XmlBoundaryErrorKind::Input, MEMBER_SHAPE),
                ));
            }
            check_document_size(member.document.byte_len()).map_err(|error| {
                XmlDocumentSetInputError::member(source, index, member.path, error)
            })?;
        }
    }
    for &(source, members) in sets {
        for (index, member) in members.iter().enumerate() {
            budget
                .charge(source, member.document.byte_len())
                .map_err(|error| {
                    XmlDocumentSetInputError::member(source, index, member.path, *error.boundary)
                })?;
        }
    }
    Ok(())
}

/// Parses one supplied set, member by member in supplied order, into the
/// document set a local file-set reader produces for the same files.
pub fn parse_xml_document_set<D: XmlDocumentSetText + ?Sized>(
    descriptor: &str,
    source: XmlInputSource,
    members: &[XmlDocumentSetMember<'_, D>],
) -> Result<Instance, XmlDocumentSetInputError> {
    let mut documents = Vec::with_capacity(members.len());
    for (index, member) in members.iter().enumerate() {
        let owned = |error| XmlDocumentSetInputError::member(source, index, member.path, error);
        let value = member
            .document
            .parse_structured(descriptor)
            .map_err(owned)?;
        let document = match member.resolved_source_path {
            Some(resolved) => DocumentMember::new_source(member.path, resolved, value),
            None => DocumentMember::new(member.path, value),
        }
        .ok_or_else(|| {
            owned(XmlBoundaryError::detail(
                XmlBoundaryErrorKind::Input,
                MEMBER_SHAPE,
            ))
        })?;
        documents.push(document);
    }
    Ok(Instance::DocumentSet(documents))
}

#[cfg(test)]
mod tests {
    use super::super::{
        XmlBoundaryErrorKind, XmlDocumentSetBudget, XmlInputSetBudget, XmlInputSetResourceError,
        XmlOutputSetResourceError, XmlOutputTarget, parse_structured_xml,
        parse_structured_xml_bytes,
    };
    use super::*;

    #[test]
    fn original_named_utf8_boundary_allocation_and_decoder_cause_remain_exact() {
        let original = Box::new(parse_structured_xml_bytes("{}", &[0xff]).unwrap_err());
        let allocation = original.as_ref() as *const XmlBoundaryError;
        let cause = original.source().unwrap() as *const dyn Error;
        assert!(original.source().unwrap().is::<std::str::Utf8Error>());
        let error = XmlInputDocumentExecutionError::from_input_boundary(
            Some(XmlInputSource::Named {
                index: 1,
                name: "beta",
            }),
            original,
        );
        assert_eq!(
            error.owner,
            Some(XmlInputDocumentOwner::Input(XmlInputSource::Named {
                index: 1,
                name: "beta"
            }))
        );
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        assert!(std::ptr::eq(
            error
                .source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .unwrap(),
            allocation
        ));
        assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
    }

    #[test]
    fn schema_ownership_is_preserved_by_phase_instead_of_blanket_category() {
        let input = XmlInputDocumentExecutionError::from_input_boundary(
            Some(XmlInputSource::Primary),
            Box::new(parse_structured_xml("{", "<Input/>").unwrap_err()),
        );
        assert_eq!(input.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert_eq!(
            input.owner,
            Some(XmlInputDocumentOwner::Input(XmlInputSource::Primary))
        );
        let serializer = XmlDocumentExecutionError::serialization(
            3,
            "same.xml",
            parse_structured_xml("{", "<Input/>").unwrap_err(),
        );
        let original = serializer.boundary.as_ref() as *const XmlBoundaryError;
        let output = XmlInputDocumentExecutionError::from(serializer);
        assert_eq!(output.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert!(output.owner.is_none());
        assert!(std::ptr::eq(output.boundary.as_ref(), original));
    }

    #[test]
    fn independent_input_and_output_count_errors_keep_global_ownership_and_typed_causes() {
        let original = XmlInputSetBudget::new(4097).unwrap_err();
        let input =
            XmlInputDocumentExecutionError::from_input_boundary(original.input, original.boundary);
        assert!(input.owner.is_none());
        assert_eq!(input.boundary.kind, XmlBoundaryErrorKind::Input);
        assert!(
            input
                .boundary
                .source()
                .unwrap()
                .is::<XmlInputSetResourceError>()
        );
        let original = XmlDocumentSetBudget::new(4097).err().unwrap();
        let output = XmlInputDocumentExecutionError::from(original);
        assert!(output.owner.is_none());
        assert_eq!(output.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(
            output
                .boundary
                .source()
                .unwrap()
                .is::<XmlOutputSetResourceError>()
        );
        assert!(XmlDocumentSetBudget::new(0).is_ok());
    }

    #[test]
    fn member_conversion_keeps_final_index_path_and_original_boundary_cause() {
        // Counter-only control; no actual 256 MiB document is constructed.
        let mut budget = XmlDocumentSetBudget::new(5).unwrap();
        for index in 0..4 {
            budget.charge(index, "same.xml", 64 * 1024 * 1024).unwrap();
        }
        let original = budget.charge(4, "../雪😀.xml", 1).unwrap_err();
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let output = XmlInputDocumentExecutionError::from(original);
        let Some(XmlInputDocumentOwner::Member(member)) = output.owner else {
            panic!("expected member");
        };
        assert_eq!((member.index, member.path.as_str()), (4, "../雪😀.xml"));
        assert_eq!(member.target, XmlOutputTarget::Primary);
        assert!(std::ptr::eq(output.boundary.as_ref(), allocation));
        assert!(
            output
                .boundary
                .source()
                .unwrap()
                .is::<XmlOutputSetResourceError>()
        );
    }

    fn member_schema() -> ir::SchemaNode {
        ir::SchemaNode::group(
            "Input",
            vec![
                ir::SchemaNode::scalar("Title", ir::ScalarType::String),
                ir::SchemaNode::group(
                    "Rows",
                    vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
                )
                .repeating(),
            ],
        )
    }

    fn member<'a>(
        path: &'a str,
        resolved: Option<&'a str>,
        document: &'a str,
    ) -> XmlDocumentSetMember<'a> {
        XmlDocumentSetMember {
            path,
            resolved_source_path: resolved,
            document,
        }
    }

    const ONE: &str = "<Input><Title>One</Title><Rows><Value>a</Value></Rows></Input>";
    const TWO: &str = "<Input><Title>Two</Title></Input>";

    #[test]
    fn members_keep_paths_resolved_locations_and_reader_values_in_order() {
        let schema = member_schema();
        let descriptor =
            codegen_schema::encode(&schema, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let members = [
            member("b/one.xml", Some("/in/b/one.xml"), ONE),
            member("two.xml", None, TWO),
        ];
        let expected = Instance::DocumentSet(vec![
            DocumentMember::new_source(
                "b/one.xml",
                "/in/b/one.xml",
                format_xml::from_str(ONE, &schema).unwrap(),
            )
            .unwrap(),
            DocumentMember::new("two.xml", format_xml::from_str(TWO, &schema).unwrap()).unwrap(),
        ]);
        let parsed =
            parse_xml_document_set(&descriptor, XmlInputSource::Primary, &members).unwrap();
        assert_eq!(parsed, expected);
        let Instance::DocumentSet(documents) = &parsed else {
            panic!("document set");
        };
        assert_eq!(documents[0].source_path(), "/in/b/one.xml");
        assert_eq!(documents[1].source_path(), "two.xml");
        let bytes = members.map(|member| XmlDocumentSetMember {
            path: member.path,
            resolved_source_path: member.resolved_source_path,
            document: member.document.as_bytes(),
        });
        assert_eq!(
            parse_xml_document_set(&descriptor, XmlInputSource::Primary, &bytes).unwrap(),
            expected
        );
        let empty: [XmlDocumentSetMember<'_>; 0] = [];
        assert_eq!(
            parse_xml_document_set(&descriptor, XmlInputSource::Primary, &empty).unwrap(),
            Instance::DocumentSet(Vec::new())
        );
    }

    #[test]
    fn each_member_owns_its_own_parse_and_utf8_failure() {
        let descriptor =
            codegen_schema::encode(&member_schema(), crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let named = XmlInputSource::Named {
            index: 0,
            name: "Ref",
        };
        let members = [
            member("ok.xml", None, ONE),
            member("bad.xml", None, "<Input>"),
        ];
        let error = parse_xml_document_set(&descriptor, named, &members).unwrap_err();
        assert_eq!(
            error.member,
            Some(XmlDocumentSetMemberOwner {
                source: named,
                index: 1,
                path: "bad.xml".into()
            })
        );
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Input);
        assert!(error.to_string().starts_with("XML input Named"));
        let bytes = [XmlDocumentSetMember {
            path: "bytes.xml",
            resolved_source_path: None,
            document: &[0xff_u8][..],
        }];
        let error = parse_xml_document_set(&descriptor, named, &bytes).unwrap_err();
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Utf8);
        assert_eq!(error.member.unwrap().path, "bytes.xml");
    }

    #[test]
    fn preflight_refuses_member_shapes_and_counts_before_parsing() {
        let named = XmlInputSource::Named {
            index: 0,
            name: "Ref",
        };
        // Unparseable text proves nothing is parsed before the refusal.
        let primary = [member("p.xml", None, "<")];
        let invalid = [member("ok.xml", None, "<"), member("", None, "<")];
        let sets: [(XmlInputSource, &[XmlDocumentSetMember<'_>]); 2] =
            [(XmlInputSource::Primary, &primary), (named, &invalid)];
        let error = preflight_xml_document_sets(&sets).unwrap_err();
        assert_eq!(
            error.member,
            Some(XmlDocumentSetMemberOwner {
                source: named,
                index: 1,
                path: String::new()
            })
        );
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Input);
        let resolved = [member("ok.xml", Some(""), "<")];
        let error =
            preflight_xml_document_sets(&[(XmlInputSource::Primary, &resolved[..])]).unwrap_err();
        assert_eq!(error.member.unwrap().index, 0);
        assert!(preflight_xml_document_sets(&sets[..1]).is_ok());
        let many = vec![member("m.xml", None, ""); 4097];
        let error =
            preflight_xml_document_sets(&[(XmlInputSource::Primary, &many[..])]).unwrap_err();
        assert!(error.member.is_none());
        assert!(
            error
                .boundary
                .source()
                .unwrap()
                .is::<XmlInputSetResourceError>()
        );
    }

    #[test]
    fn names_are_admitted_like_the_typed_entry_points_before_any_member() {
        static CATALOGUE: [BasicNamedSourceDeclaration; 1] = [BasicNamedSourceDeclaration {
            name: "Ref",
            mode: crate::BasicNamedSourceMode::Optional,
        }];
        let limits = OwnedSourceLimits {
            max_roots: 8,
            max_active_frames: 8,
            max_path_segments: 16,
            max_metadata_bytes: 65_536,
            max_walk_contexts: 64,
        };
        admit_xml_document_set_names(&CATALOGUE, [], limits).unwrap();
        admit_xml_document_set_names(&CATALOGUE, ["Ref"], limits).unwrap();
        for (names, expected) in [
            (
                vec!["Other"],
                RuntimeError::UnexpectedNamedSource {
                    name: "Other".into(),
                },
            ),
            (
                vec!["Ref", "Ref"],
                RuntimeError::DuplicateNamedSource { name: "Ref" },
            ),
        ] {
            let error = admit_xml_document_set_names(&CATALOGUE, names, limits).unwrap_err();
            assert!(error.member.is_none());
            assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Mapping);
            assert_eq!(
                error
                    .boundary
                    .source()
                    .unwrap()
                    .downcast_ref::<RuntimeError>(),
                Some(&expected)
            );
        }
    }
}
