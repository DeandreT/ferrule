use ir::ScalarType;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XmlFormatError {
    #[error("invalid XML scalar output: {0}")]
    InvalidScalarOutput(ir::XmlScalarOutputError),
    #[error(
        "XML output contains forbidden XML 1.0 character U+{codepoint:04X} at UTF-8 byte {byte_offset}"
    )]
    InvalidXmlCharacter { codepoint: u32, byte_offset: usize },
    #[error("unsupported structured XML input schema")]
    UnsupportedStructuredInputSchema,
    #[error("structured XML input exceeds {budget} limit {limit}")]
    StructuredInputLimit { budget: &'static str, limit: usize },
    #[error(
        "structured XML parser resource `{resource}` count {observed_count} exceeds limit {limit}"
    )]
    StructuredParserResourceLimit {
        resource: &'static str,
        observed_count: u64,
        limit: u64,
    },
    #[error("structured XML input requires names within the Basic Multilingual Plane")]
    StructuredInputPhysicalName,
    #[error("structured XML input does not support prefixed local `xmlns` attributes")]
    StructuredInputNamespaceAttribute,
    #[error("structured XML input does not support DTD declarations")]
    StructuredInputDtd,
    #[error("structured XML input requires UTF-8 declaration encoding")]
    StructuredInputEncoding,
    #[error("structured XML content `{name}` contains element children")]
    StructuredInputContent { name: String },
    #[error("observed XML root-view input requires UTF-8 declaration encoding")]
    RootViewEncoding,
    #[error("unsupported observed XML root-view read policy for `{name}`")]
    UnsupportedRootViewReadPolicy { name: String },
    #[error("observed XML root-view document exceeds {limit} bytes")]
    RootViewDocumentLimit { limit: usize },
    #[error("observed flat XML root `{name}` contains unproved content")]
    RootViewContent { name: String },
    #[error("invalid XML schema hints: {0}")]
    InvalidSchemaHints(ir::XmlSchemaHintsError),
    #[error("XML schema hints conflict with the root attributes: {0}")]
    SchemaHintCollision(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Parse(#[from] roxmltree::Error),
    #[error("xml write error: {0}")]
    Write(#[from] quick_xml::Error),
    #[error("expected root element `{expected}`, found `{found}`")]
    UnexpectedRoot { expected: String, found: String },
    #[error("missing required element `{0}`")]
    MissingElement(String),
    #[error("cannot parse `{value}` as {ty:?} for element `{name}`")]
    ScalarParse {
        name: String,
        ty: ScalarType,
        value: String,
    },
    #[error("element or attribute `{name}` requires fixed value `{expected}`, got `{got}`")]
    FixedValue {
        name: String,
        expected: String,
        got: String,
    },
    #[error("XSD default on `{name}` cannot be represented: {reason}")]
    UnsupportedSchemaDefault { name: String, reason: &'static str },
    #[error("XML node `{name}` has invalid optional element occurrence metadata")]
    InvalidXmlOptional { name: String },
    #[error("XML node `{name}` has invalid required attribute-use metadata")]
    InvalidXmlAttributeRequired { name: String },
    #[error("XML attribute `{name}` has unsupported use `{value}`")]
    UnsupportedXmlAttributeUse { name: String, value: String },
    #[error(
        "required XML attribute `{name}` cannot preserve flattened declaration-default provenance"
    )]
    UnsupportedXmlAttributeDefault { name: String },
    #[error("XML attribute `{name}` cannot preserve a fixed declaration reference")]
    UnsupportedXmlAttributeFixedReference { name: String },
    #[error("XML alternatives on `{group}` have incompatible attribute uses at `{field}`")]
    UnsupportedXmlAlternativeAttributeUse { group: String, field: String },
    #[error(
        "optional singular XML sequence with {element_count} members cannot preserve correlated absence"
    )]
    UnsupportedOptionalSequence { element_count: usize },
    #[error(
        "optional {compositor} compositor with {element_count} elements cannot preserve correlated absence or repetition"
    )]
    UnsupportedOptionalCompositor {
        compositor: String,
        element_count: usize,
    },
    #[error("XML alternatives on `{group}` cannot preserve compositor correlations")]
    UnsupportedXmlAlternativeCompositor { group: String },
    #[error("XML alternatives on `{group}` have incompatible optional occurrences at `{field}`")]
    UnsupportedXmlAlternativeOccurrence { group: String, field: String },
    #[error("XML node `{name}` has invalid declared default type `{value}`")]
    InvalidXmlDefaultType { name: String, value: String },
    #[error("XML node `{name}` cannot preserve a heterogeneous scalar union")]
    UnsupportedScalarUnion { name: String },
    #[error("element `{name}` expected {expected}, got {got}")]
    Shape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("element `{name}` expected {expected:?}, got {got}")]
    ValueType {
        name: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("element `{group}` has unexpected field `{field}`")]
    UnexpectedField { group: String, field: String },
    #[error("element `{group}` has duplicate field `{field}`")]
    DuplicateField { group: String, field: String },
    #[error("element `{group}` has invalid singular XML choice: {reason}")]
    InvalidXmlChoice { group: String, reason: String },
    #[error("element `{group}` has invalid mixed-content metadata: {reason}")]
    InvalidMixedContent { group: String, reason: String },
    #[error("element `{group}` cannot reconstruct its repeating XML sequence: {reason}")]
    AmbiguousRepeatingSequence { group: String, reason: String },
    #[error("element `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("XML root `{name}` does not support inactive type-member reads")]
    UnsupportedInactiveRootTypeMembers { name: String },
    #[error("element `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("element `{name}` has multiple expanded XML names and no retained occurrence identity")]
    AmbiguousXmlName { name: String },
    #[error("schema node `{node}` has invalid expanded XML name alternatives")]
    InvalidXmlNameAlternatives { node: String },
    #[error("element `{name}` uses xsi:nil but its schema is not nillable")]
    UnexpectedXmlNil { name: String },
    #[error("element `{name}` with xsi:nil cannot contain a value")]
    XmlNilWithContent { name: String },
    #[error("nilled group element `{name}` cannot be represented yet")]
    UnsupportedXmlNilGroup { name: String },
    #[error("element `{name}` has invalid xsi:nil value `{value}`")]
    InvalidXmlNil { name: String, value: String },
    #[error(
        "repeating xs:{compositor} with {element_count} non-unique element members cannot preserve occurrence identity"
    )]
    UnsupportedRepeatingParticle {
        compositor: String,
        element_count: usize,
    },
    #[error(
        "repeating xs:sequence contains nested xs:{compositor}, whose occurrence choices cannot be preserved"
    )]
    UnsupportedRepeatingSequenceCompositor { compositor: String },
    #[error(
        "repeating xs:sequence contains a nested repeating sequence with {element_count} members, whose tuple identity cannot be preserved"
    )]
    UnsupportedNestedRepeatingSequence { element_count: usize },
    #[error("xs:any wildcard cannot be represented: {reason}")]
    UnsupportedXmlWildcard { reason: &'static str },
    #[error("XSD 1.1 open content cannot be represented: {reason}")]
    UnsupportedXmlOpenContent { reason: &'static str },
    #[error("xs:any wildcard does not allow element `{name}` in namespace `{namespace}`")]
    XmlWildcardNamespaceMismatch { name: String, namespace: String },
    #[error("xs:any wildcard excludes element `{name}` in namespace `{namespace}`")]
    XmlWildcardExcludedName { name: String, namespace: String },
    #[error(
        "strict xs:any wildcard has no resolved declaration for element `{name}` in namespace `{namespace}`"
    )]
    UndeclaredStrictXmlWildcardElement { name: String, namespace: String },
    #[error(
        "generic xs:any element `{name}` in namespace `{namespace}` must use its typed mapping field"
    )]
    KnownXmlWildcardElementRequiresTypedField { name: String, namespace: String },
    #[error("single-occurrence xs:any wildcard matched more than one child element")]
    XmlWildcardCardinality,
    #[error(
        "extra occurrence of declared element `{name}` in namespace `{namespace}` cannot be kept by the xs:any wildcard: {reason}"
    )]
    UnrepresentableXmlWildcardSurplusElement {
        name: String,
        namespace: String,
        reason: &'static str,
    },
    #[error("xs:anyAttribute wildcard cannot be represented: {reason}")]
    UnsupportedXmlAttributeWildcard { reason: &'static str },
    #[error(
        "xs:anyAttribute wildcard does not allow attribute `{name}` in namespace `{namespace}`"
    )]
    XmlAttributeWildcardNamespaceMismatch { name: String, namespace: String },
    #[error("xs:anyAttribute wildcard excludes attribute `{name}` in namespace `{namespace}`")]
    XmlAttributeWildcardExcludedName { name: String, namespace: String },
    #[error(
        "strict xs:anyAttribute wildcard has no resolved declaration for attribute `{name}` in namespace `{namespace}`"
    )]
    UndeclaredStrictXmlWildcardAttribute { name: String, namespace: String },
    #[error("XSD expansion exceeds the {limit}-element materialization limit")]
    SchemaMaterializationLimit { limit: usize },
    #[error("XSD import exceeds the {limit}-level nesting and reference depth limit")]
    SchemaDepthLimit { limit: usize },
    #[error("named xs:{kind} `{name}` contains a reference cycle")]
    SchemaGroupCycle { kind: &'static str, name: String },
    #[error("named xs:{kind} `{name}` cannot be represented: {reason}")]
    UnsupportedSchemaGroup {
        kind: &'static str,
        name: String,
        reason: &'static str,
    },
    #[error("xs:complexContent restriction of `{base}` cannot be represented: {reason}")]
    UnsupportedComplexContentRestriction { base: String, reason: &'static str },
    #[error("xs:simpleContent restriction of `{base}` cannot be represented: {reason}")]
    UnsupportedSimpleContentRestriction { base: String, reason: &'static str },
    #[error("schema node `{node}` cannot be both XML text and an attribute")]
    ConflictingSchemaRoles { node: String },
    #[error("schema {kind} `{node}` cannot be serialized as XML {role}")]
    UnsupportedSchemaRole {
        node: String,
        role: &'static str,
        kind: &'static str,
    },
    #[error("schema {role} `{node}` cannot repeat")]
    RepeatingSchemaRole { node: String, role: &'static str },
    #[error("schema group `{group}` has {count} XML text fields; at most one is supported")]
    MultipleTextFields { group: String, count: usize },
    #[error("schema group `{group}` mixes XML text with child elements")]
    MixedContent { group: String },
    #[error("schema group `{group}` has alternatives that XSD export cannot preserve")]
    UnsupportedGroupAlternatives { group: String },
    #[error("schema group `{group}` has invalid XML repeating-sequence metadata")]
    InvalidRepeatingSequenceSchema { group: String },
    #[error(
        "schema XML name `{node}` uses namespace `{namespace}`, which cannot be emitted in a single XSD whose target namespace is `{target_namespace}`"
    )]
    UnsupportedNamespaceExport {
        node: String,
        namespace: String,
        target_namespace: String,
    },
    #[error("generated XSD root filename `{name}` must be one traversal-safe `.xsd` filename")]
    InvalidXsdArtifactName { name: String },
    #[error("cross-namespace XSD export exceeds the {limit}-artifact limit")]
    NamespaceArtifactLimit { limit: usize },
    #[error("cross-namespace XSD export exceeds the {limit}-reference limit")]
    NamespaceReferenceLimit { limit: usize },
    #[error(
        "conflicting {role} declarations for expanded XML name `{{{namespace}}}{name}` cannot share one schema set"
    )]
    ConflictingNamespaceDeclaration {
        role: &'static str,
        namespace: String,
        name: String,
    },
    #[error("cross-namespace XSD dependency cycle reaches {role} `{{{namespace}}}{name}`")]
    NamespaceDependencyCycle {
        role: &'static str,
        namespace: String,
        name: String,
    },
    #[error(
        "schema group `{group}` has sibling XML name `{name}` in both `{first_namespace}` and `{second_namespace}`; mapping fields require distinct local names"
    )]
    AmbiguousNamespaceSiblings {
        group: String,
        name: String,
        first_namespace: String,
        second_namespace: String,
    },
    #[error(
        "schema group `{group}` has alternatives whose xsi:type identity XML input cannot preserve"
    )]
    UnsupportedAlternativeRead { group: String },
    #[error("element `{name}` has invalid XML type origin: {source}")]
    InvalidXmlTypeOrigin {
        name: String,
        source: ir::XmlTypeOriginError,
    },
    #[error("element `{name}` has invalid xsi:type QName `{value}`")]
    InvalidXmlType { name: String, value: String },
    #[error("element `{name}` has undeclared xsi:type `{value}`")]
    UnknownXmlType { name: String, value: String },
    #[error("element `{name}` has invalid substitution member marker `{value}`")]
    InvalidSubstitutionMember { name: String, value: String },
    #[error("element `{name}` has undeclared substitution member `{value}`")]
    UnknownSubstitutionMember { name: String, value: String },
    #[error("substitution group `{head}` contains a derivation cycle at `{member}`")]
    SubstitutionGroupCycle { head: String, member: String },
    #[error("substitution group `{head}` contains conflicting declarations for `{member}`")]
    ConflictingSubstitutionMember { head: String, member: String },
    #[error("substitution group discovery exceeds the {limit}-declaration limit")]
    SubstitutionGroupLimit { limit: usize },
    #[error("substitution group `{head}` member `{member}` cannot be represented: {reason}")]
    UnsupportedSubstitutionGroup {
        head: String,
        member: String,
        reason: &'static str,
    },
    #[error("generic XML element item has no non-empty LocalName or NodeName field")]
    MissingGenericElementName,
    #[error("generic XML attribute item has no non-empty LocalName field")]
    MissingGenericAttributeName,
    #[error("recursive schema reference `{node}` has no unique concrete group anchor `{anchor}`")]
    UnsupportedRecursiveAnchor { node: String, anchor: String },
    #[error("XML recursion exceeds the {limit}-element depth limit")]
    RecursionLimit { limit: usize },
    #[error("invalid SOAP envelope: {0}")]
    InvalidSoapEnvelope(String),
    #[error("SOAP body cannot be projected into WSDL message schema `{schema}`")]
    IncompatibleSoapBody { schema: String },
}
