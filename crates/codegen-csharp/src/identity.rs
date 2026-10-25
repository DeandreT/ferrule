//! Generated C# library identity: assembly name, root namespace and mapping
//! class name.
//!
//! [`Identity::default`] is the identity every library has always used and
//! renders byte-identical artifacts. A configured identity is validated before
//! rendering so that two generated libraries with distinct identities can be
//! referenced from one host project. The vendored runtime keeps its
//! `Ferrule.Runtime` namespace in every library; its types are distinct per
//! assembly, so a host that names runtime types of more than one library uses
//! an `extern alias` for each additional reference.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use codegen::{IdentityError, IdentityKind, IdentityViolation, ManifestSelection};

use crate::runtime;

const DEFAULT_ASSEMBLY_NAME: &str = "Ferrule.Generated";
const DEFAULT_ROOT_NAMESPACE: &str = "Ferrule.Generated";
const DEFAULT_MAPPING_CLASS: &str = "GeneratedMapping";

/// The longest accepted assembly name, namespace or class name.
pub const MAX_IDENTITY_BYTES: usize = 128;
/// Every configured mapping class name ends with this suffix, which no other
/// generated member or top-level type declares.
pub const MAPPING_CLASS_SUFFIX: &str = "Mapping";

/// C# reserved keywords, including the implementation-reserved `__` forms.
const KEYWORDS: [&str; 81] = [
    "__arglist",
    "__makeref",
    "__reftype",
    "__refvalue",
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

/// Root namespaces owned by ferrule and the platform. `Ferrule.Generated`
/// itself remains the default namespace.
const RESERVED_ROOTS: [&str; 4] = ["Ferrule", "Microsoft", "System", "global"];

/// Windows device names, which cannot name the project file.
const DEVICE_NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// The assembly, namespace and class one generated C# library is hosted under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    assembly_name: String,
    root_namespace: String,
    mapping_class: String,
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            assembly_name: DEFAULT_ASSEMBLY_NAME.to_owned(),
            root_namespace: DEFAULT_ROOT_NAMESPACE.to_owned(),
            mapping_class: DEFAULT_MAPPING_CLASS.to_owned(),
        }
    }
}

impl Identity {
    /// Validates a configured identity. Assembly names are dotted segments of
    /// ASCII letters, digits, `_` and `-`; namespace segments and the class are
    /// ASCII C# identifiers that are not keywords. A namespace may not start
    /// in a reserved root or with a name the vendored runtime resolves, and the
    /// class must end with [`MAPPING_CLASS_SUFFIX`] and differ from every
    /// namespace segment. Each default value is always accepted.
    pub fn new(
        assembly_name: &str,
        root_namespace: &str,
        mapping_class: &str,
    ) -> Result<Self, IdentityError> {
        validate_assembly_name(assembly_name)?;
        validate_namespace(root_namespace)?;
        validate_class(mapping_class, root_namespace)?;
        Ok(Self {
            assembly_name: assembly_name.to_owned(),
            root_namespace: root_namespace.to_owned(),
            mapping_class: mapping_class.to_owned(),
        })
    }

    pub fn assembly_name(&self) -> &str {
        &self.assembly_name
    }

    pub fn root_namespace(&self) -> &str {
        &self.root_namespace
    }

    pub fn mapping_class(&self) -> &str {
        &self.mapping_class
    }

    /// The generated project file, named after the assembly.
    pub(crate) fn project_path(&self) -> String {
        format!("{}.csproj", self.assembly_name)
    }
}

/// Settings of one C# generation request. The default renders the artifacts
/// of the corresponding free emitter functions exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub identity: Identity,
    pub manifest: ManifestSelection,
}

fn refuse(
    kind: IdentityKind,
    value: &str,
    violation: IdentityViolation,
) -> Result<(), IdentityError> {
    Err(IdentityError::new(kind, value, violation))
}

fn check_length(kind: IdentityKind, value: &str) -> Result<(), IdentityError> {
    if value.is_empty() {
        return refuse(kind, value, IdentityViolation::Empty);
    }
    if value.len() > MAX_IDENTITY_BYTES {
        return refuse(
            kind,
            value,
            IdentityViolation::TooLong {
                maximum: MAX_IDENTITY_BYTES,
            },
        );
    }
    Ok(())
}

/// One ASCII identifier segment; `extra` admits further non-leading characters.
fn check_segment(
    kind: IdentityKind,
    value: &str,
    segment: &str,
    extra: &[char],
) -> Result<(), IdentityError> {
    let Some(first) = segment.chars().next() else {
        return refuse(kind, value, IdentityViolation::EmptySegment);
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return refuse(
            kind,
            value,
            IdentityViolation::InvalidStart { character: first },
        );
    }
    if let Some(character) = segment.chars().find(|character| {
        !(character.is_ascii_alphanumeric() || *character == '_' || extra.contains(character))
    }) {
        return refuse(
            kind,
            value,
            IdentityViolation::InvalidCharacter { character },
        );
    }
    Ok(())
}

fn check_keyword(kind: IdentityKind, value: &str, segment: &str) -> Result<(), IdentityError> {
    if KEYWORDS.contains(&segment) {
        return refuse(
            kind,
            value,
            IdentityViolation::Keyword {
                keyword: segment.to_owned(),
            },
        );
    }
    Ok(())
}

fn validate_assembly_name(name: &str) -> Result<(), IdentityError> {
    let kind = IdentityKind::CSharpAssemblyName;
    if name == DEFAULT_ASSEMBLY_NAME {
        return Ok(());
    }
    check_length(kind, name)?;
    for segment in name.split('.') {
        check_segment(kind, name, segment, &['-'])?;
    }
    let first = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = DEVICE_NAMES.contains(&first.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            first
                .strip_prefix(prefix)
                .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
        });
    if device || name == "Ferrule.Runtime" || name.starts_with("Ferrule.Runtime.") {
        return refuse(
            kind,
            name,
            IdentityViolation::Reserved {
                reserved: name.to_owned(),
            },
        );
    }
    Ok(())
}

fn validate_namespace(namespace: &str) -> Result<(), IdentityError> {
    let kind = IdentityKind::CSharpNamespace;
    if namespace == DEFAULT_ROOT_NAMESPACE {
        return Ok(());
    }
    check_length(kind, namespace)?;
    for segment in namespace.split('.') {
        check_segment(kind, namespace, segment, &[])?;
        check_keyword(kind, namespace, segment)?;
    }
    let first = namespace.split('.').next().unwrap_or_default();
    if RESERVED_ROOTS.contains(&first) {
        return refuse(
            kind,
            namespace,
            IdentityViolation::Reserved {
                reserved: first.to_owned(),
            },
        );
    }
    // The vendored runtime resolves simple names through `Ferrule.Runtime`,
    // `Ferrule` and then the global namespace, which contains this first
    // segment, before its imported namespaces. A first segment equal to any
    // name it spells would hide that name from the runtime.
    if runtime_names().contains(first) {
        return refuse(
            kind,
            namespace,
            IdentityViolation::ShadowsRuntimeName {
                segment: first.to_owned(),
            },
        );
    }
    Ok(())
}

fn validate_class(class: &str, namespace: &str) -> Result<(), IdentityError> {
    let kind = IdentityKind::CSharpMappingClass;
    if class == DEFAULT_MAPPING_CLASS {
        return Ok(());
    }
    check_length(kind, class)?;
    check_segment(kind, class, class, &[])?;
    check_keyword(kind, class, class)?;
    if class.len() == MAPPING_CLASS_SUFFIX.len() || !class.ends_with(MAPPING_CLASS_SUFFIX) {
        return refuse(
            kind,
            class,
            IdentityViolation::MissingSuffix {
                suffix: MAPPING_CLASS_SUFFIX,
            },
        );
    }
    if namespace.split('.').any(|segment| segment == class) {
        return refuse(kind, class, IdentityViolation::ClassNamesNamespaceSegment);
    }
    Ok(())
}

/// Every identifier-shaped word in any vendored runtime source.
fn runtime_names() -> &'static BTreeSet<&'static str> {
    static NAMES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = BTreeSet::new();
        for source in runtime::vendored_sources() {
            let bytes = source.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                let start = index;
                if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
                    while index < bytes.len()
                        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                    {
                        index += 1;
                    }
                    names.insert(&source[start..index]);
                } else if bytes[index].is_ascii_digit() {
                    while index < bytes.len()
                        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                    {
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
        }
        names
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation(
        assembly: &str,
        namespace: &str,
        class: &str,
    ) -> (IdentityKind, IdentityViolation) {
        let error = Identity::new(assembly, namespace, class).unwrap_err();
        (error.kind, error.violation)
    }

    #[test]
    fn defaults_and_distinct_configured_identities_are_accepted() {
        let default =
            Identity::new("Ferrule.Generated", "Ferrule.Generated", "GeneratedMapping").unwrap();
        assert_eq!(default, Identity::default());
        assert_eq!(default.project_path(), "Ferrule.Generated.csproj");
        let orders = Identity::new("Acme.Orders-Mapping", "Acme.Orders", "OrderMapping").unwrap();
        assert_eq!(orders.assembly_name(), "Acme.Orders-Mapping");
        assert_eq!(orders.root_namespace(), "Acme.Orders");
        assert_eq!(orders.mapping_class(), "OrderMapping");
        assert_eq!(orders.project_path(), "Acme.Orders-Mapping.csproj");
        Identity::new(
            "Contoso.Invoices",
            "Contoso.Billing.Invoices",
            "InvoiceMapping",
        )
        .unwrap();
    }

    #[test]
    fn configured_identities_are_refused_with_typed_violations() {
        use IdentityKind::*;
        use IdentityViolation::*;
        let cases = [
            (("", "Acme", "OrderMapping"), (CSharpAssemblyName, Empty)),
            (
                ("Acme..Orders", "Acme", "OrderMapping"),
                (CSharpAssemblyName, EmptySegment),
            ),
            (
                ("Acme.9Orders", "Acme", "OrderMapping"),
                (CSharpAssemblyName, InvalidStart { character: '9' }),
            ),
            (
                ("Acme/Orders", "Acme", "OrderMapping"),
                (CSharpAssemblyName, InvalidCharacter { character: '/' }),
            ),
            (
                ("con.Orders", "Acme", "OrderMapping"),
                (
                    CSharpAssemblyName,
                    Reserved {
                        reserved: "con.Orders".into(),
                    },
                ),
            ),
            (
                ("Ferrule.Runtime", "Acme", "OrderMapping"),
                (
                    CSharpAssemblyName,
                    Reserved {
                        reserved: "Ferrule.Runtime".into(),
                    },
                ),
            ),
            (
                ("Acme", "Acme.Orders-X", "OrderMapping"),
                (CSharpNamespace, InvalidCharacter { character: '-' }),
            ),
            (
                ("Acme", "Acme.class", "OrderMapping"),
                (
                    CSharpNamespace,
                    Keyword {
                        keyword: "class".into(),
                    },
                ),
            ),
            (
                ("Acme", "Ferrule.Orders", "OrderMapping"),
                (
                    CSharpNamespace,
                    Reserved {
                        reserved: "Ferrule".into(),
                    },
                ),
            ),
            (
                ("Acme", "System.Orders", "OrderMapping"),
                (
                    CSharpNamespace,
                    Reserved {
                        reserved: "System".into(),
                    },
                ),
            ),
            (
                ("Acme", "Math.Orders", "OrderMapping"),
                (
                    CSharpNamespace,
                    ShadowsRuntimeName {
                        segment: "Math".into(),
                    },
                ),
            ),
            (
                ("Acme", &format!("A{}", ".B".repeat(64)), "OrderMapping"),
                (CSharpNamespace, TooLong { maximum: 128 }),
            ),
            (
                ("Acme", "Acme", "Orders"),
                (CSharpMappingClass, MissingSuffix { suffix: "Mapping" }),
            ),
            (
                ("Acme", "Acme", "Mapping"),
                (CSharpMappingClass, MissingSuffix { suffix: "Mapping" }),
            ),
            (
                ("Acme", "Acme.OrderMapping", "OrderMapping"),
                (CSharpMappingClass, ClassNamesNamespaceSegment),
            ),
            (
                ("Acme", "Acme", "Order.Mapping"),
                (CSharpMappingClass, InvalidCharacter { character: '.' }),
            ),
        ];
        for ((assembly, namespace, class), expected) in cases {
            assert_eq!(
                violation(assembly, namespace, class),
                expected,
                "{assembly} {namespace} {class}"
            );
        }
        assert_eq!(
            Identity::new("Acme", "Acme.class", "OrderMapping")
                .unwrap_err()
                .to_string(),
            "invalid generated C# root namespace \"Acme.class\": \"class\" is a language keyword"
        );
    }

    #[test]
    fn runtime_names_cover_every_vendored_runtime_source() {
        let names = runtime_names();
        for name in ["Math", "Encoding", "FerruleInstance", "Ferrule", "System"] {
            assert!(names.contains(name), "{name}");
        }
        for name in ["Acme", "Contoso"] {
            assert!(!names.contains(name), "{name}");
        }
    }
}
