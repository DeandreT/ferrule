//! Shared invariants for the MFD byte-package admission fuzz target.
//!
//! Every input is a [`program`] that builds a candidate package from a
//! self-authored [`templates`] entry. The candidate goes through the public
//! byte-only boundary in `crates/mfd`: `MfdPackage::new` admits or refuses it,
//! and `import_package` imports an admitted package. Anything other than the
//! outcomes below, including a panic, is a finding:
//!
//! - Admission succeeds exactly when the candidate obeys the documented path,
//!   alias, entry-count and byte rules ([`admission_violations`]), and a
//!   refusal names one of the rules the candidate breaks. The result does not
//!   depend on entry order, and an admitted package returns exactly the
//!   supplied entries.
//! - Import returns an imported project or an `MfdError`, makes no host
//!   filesystem or network system call ([`sandbox`]), and is deterministic.
//!
//! [`known_limit_exemption`] skips the import of packages that reach a known
//! unbounded recursion.

use std::collections::BTreeSet;

use mfd::{
    MAX_PACKAGE_BYTES, MAX_PACKAGE_ENTRIES, MAX_PACKAGE_FILE_BYTES, MAX_PACKAGE_PATH_BYTES,
    MfdError, MfdPackage, PackageError, import_package,
};

pub mod program;
pub mod sandbox;
pub mod seeds;
pub mod templates;

pub use program::{Candidate, decode};

/// Largest input the fuzz target examines. Larger inputs are ignored so a
/// run's per-input cost stays proportional to the program limits. The
/// README's runs pass the same bound as `-max_len=65536`.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// Known limit ([#1167](https://github.com/DeandreT/ferrule/issues/1167)):
/// these walk a design component's `<entry>` tree recursively, one call per
/// nesting level, without a depth limit:
///
/// - `mfd::import::schema::typed_xml_entry_tree_schema` (entries carrying
///   `ferrule-kind` metadata),
/// - `mfd::import::schema::xml_ports::reconcile_children` (XML components),
/// - `mfd::import::schema::collect_json_ports_with_declarations` (JSON
///   components), and `mfd::import::target_node_default::collect`.
///
/// The shallowest overflow on the fuzz build's 8 MiB worker is 1,219 nested
/// typed entries; the harness skips designs whose component entry trees nest
/// deeper than this limit (README, Known limits).
pub const MAX_EXEMPT_ENTRY_DEPTH: usize = 512;

/// Known limit ([#1168](https://github.com/DeandreT/ferrule/issues/1168)):
/// `format_xml::xsd` imports a schema by recursive descent with no depth
/// limit: `parse_element`, `parse_complex_type` and
/// `collect_sequence_contents` per nested particle, `resolve_complex_type`
/// and `parse_complex_type_declaration` per named type reference or
/// extension base, and `groups::parse_model_group` per group reference.
/// Active element and complex type declarations are not re-entered and
/// group reference cycles are refused, so the depth grows with the number of
/// schema components in the package's schemas, whether they nest or form a
/// reference chain; nesting depth alone does not bound it.
///
/// Below the `xs:schema` root the importer matches elements by local name in
/// any namespace, so [`known_limit_exemption`] counts every element named in
/// [`XSD_COMPONENTS`] outside annotations. Each recursion level takes at
/// least three counted components, since a complex type descends only
/// through a compositor, group or complex content. The shallowest overflow
/// on the fuzz build's 8 MiB worker is a 448-type named chain of 1,345
/// counted components; this limit leaves a margin of about 2.6 times.
pub const MAX_EXEMPT_XSD_ELEMENTS: usize = 512;

/// Local names of the schema components the XSD importer descends through.
pub const XSD_COMPONENTS: &[&str] = &[
    "all",
    "alternative",
    "any",
    "anyAttribute",
    "assert",
    "attribute",
    "attributeGroup",
    "choice",
    "complexContent",
    "complexType",
    "defaultOpenContent",
    "element",
    "extension",
    "group",
    "import",
    "include",
    "list",
    "openContent",
    "override",
    "redefine",
    "restriction",
    "sequence",
    "simpleContent",
    "simpleType",
    "union",
];

const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

/// Why [`known_limit_exemption`] skipped an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownLimit {
    /// The design's component entry trees nest `depth` `<entry>` elements.
    EntryTree { depth: usize },
    /// The package's XSD documents hold `elements` schema components.
    Schemas { elements: usize },
}

/// Result of checking one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub admission: Result<usize, PackageError>,
    /// `None` when admission refused the package.
    pub import: Option<Import>,
}

/// How an admitted package imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Import {
    Imported {
        warnings: Vec<String>,
    },
    /// A package refusal, kept apart from other errors because the package
    /// boundary must report it instead of falling back to host resources.
    Refused(PackageError),
    /// Any other `MfdError`, by variant name and message.
    Failed {
        variant: String,
        message: String,
    },
    /// Not imported: the package reaches a known unbounded recursion.
    Exempt(KnownLimit),
}

impl Outcome {
    /// A short label used by the seed expectations: `refused <error>`,
    /// `imported`, `imported with warnings`, `package <error>` or
    /// `error <variant>`.
    pub fn label(&self) -> String {
        match (&self.admission, &self.import) {
            (Err(error), _) => format!("refused {error:?}"),
            (Ok(_), Some(Import::Imported { warnings })) if warnings.is_empty() => {
                "imported".into()
            }
            (Ok(_), Some(Import::Imported { .. })) => "imported with warnings".into(),
            (Ok(_), Some(Import::Refused(error))) => format!("package {error:?}"),
            (Ok(_), Some(Import::Failed { variant, .. })) => format!("error {variant}"),
            (Ok(_), Some(Import::Exempt(KnownLimit::EntryTree { .. }))) => {
                "exempt deep entry tree".into()
            }
            (Ok(_), Some(Import::Exempt(KnownLimit::Schemas { .. }))) => "exempt large XSD".into(),
            (Ok(_), None) => "admitted".into(),
        }
    }
}

/// Decodes and checks one input.
pub fn check(bytes: &[u8]) -> Outcome {
    check_candidate(&decode(bytes))
}

/// Checks one candidate package. Panics with a descriptive message when an
/// invariant fails, which is how libFuzzer records a finding.
pub fn check_candidate(candidate: &Candidate) -> Outcome {
    let violations = admission_violations(candidate);
    let admitted = admit(candidate, false);
    let reversed = admit(candidate, true);
    assert_eq!(
        admitted.is_ok(),
        reversed.is_ok(),
        "admission depends on entry order: {admitted:?} vs {reversed:?}"
    );
    let package = match admitted {
        Err(error) => {
            assert!(
                violations.contains(&error),
                "admission refused with {error:?}, but the candidate only breaks {violations:?}"
            );
            if let Err(again) = reversed {
                assert!(
                    violations.contains(&again),
                    "reversed admission refused with {again:?}, but the candidate only breaks \
                     {violations:?}"
                );
            }
            return Outcome {
                admission: Err(error),
                import: None,
            };
        }
        Ok(package) => package,
    };
    assert!(
        violations.is_empty(),
        "admission accepted a candidate that breaks {violations:?}"
    );
    let Ok(reversed) = reversed else {
        unreachable!("both admissions succeeded")
    };
    check_admitted(candidate, &package);
    assert_eq!(
        entries_of(&package),
        entries_of(&reversed),
        "admitted entries depend on entry order"
    );

    if let Some(exemption) = known_limit_exemption(&package) {
        return Outcome {
            admission: Ok(package.entries().len()),
            import: Some(Import::Exempt(exemption)),
        };
    }

    let ((first, second), access) =
        sandbox::monitor(|| (import_package(&package), import_package(&reversed)));
    assert!(
        access.none(),
        "import made {} host system call(s), first {} ({:?})",
        access.calls,
        access.first.map_or("unknown", sandbox::syscall_name),
        access.first
    );
    let first = summarize(first);
    let second = summarize(second);
    assert_eq!(first, second, "import is not deterministic");
    Outcome {
        admission: Ok(package.entries().len()),
        import: Some(first.0),
    }
}

fn admit(candidate: &Candidate, reversed: bool) -> Result<MfdPackage, PackageError> {
    let entries = candidate
        .entries
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_slice()));
    if reversed {
        MfdPackage::new(&candidate.design, entries.rev())
    } else {
        MfdPackage::new(&candidate.design, entries)
    }
}

/// Every admission rule from `docs/mfd-interop.md` the candidate breaks.
pub fn admission_violations(candidate: &Candidate) -> Vec<PackageError> {
    let mut violations = Vec::new();
    let mut insert = |error| violations.push(error);
    if !documented_path(&candidate.design) {
        insert(PackageError::InvalidPath);
    }
    let mut folded = BTreeSet::new();
    for (path, contents) in &candidate.entries {
        if !documented_path(path) {
            insert(PackageError::InvalidPath);
        } else if !folded.insert(path.to_ascii_lowercase()) {
            insert(PackageError::DuplicatePath);
        }
        if contents.len() > MAX_PACKAGE_FILE_BYTES {
            insert(PackageError::LimitExceeded);
        }
    }
    if candidate.entries.len() > MAX_PACKAGE_ENTRIES || candidate.total_bytes() > MAX_PACKAGE_BYTES
    {
        insert(PackageError::LimitExceeded);
    }
    if !candidate
        .entries
        .iter()
        .any(|(path, _)| *path == candidate.design)
    {
        insert(PackageError::MissingEntry);
    }
    let mut unique = Vec::new();
    for error in violations {
        if !unique.contains(&error) {
            unique.push(error);
        }
    }
    unique
}

/// Slash-separated relative paths of at most 256 UTF-8 bytes without empty,
/// dot or parent segments, drive prefixes or URLs (any `:`), backslashes,
/// control characters, or segments ending in a space or dot.
pub fn documented_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PACKAGE_PATH_BYTES
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.ends_with([' ', '.'])
        })
}

/// The known limit a package reaches, if any (see [`MAX_EXEMPT_ENTRY_DEPTH`]
/// and [`MAX_EXEMPT_XSD_ELEMENTS`]).
///
/// Entries are decoded and parsed the way the importer reads them: the
/// design as strict UTF-8, schemas with format_xml's schema decoder, both
/// with default `roxmltree` options, which parse iteratively. An entry
/// neither can read is not measured, since the import cannot descend into
/// it either.
pub fn known_limit_exemption(package: &MfdPackage) -> Option<KnownLimit> {
    let design = package
        .get(package.design_path())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok());
    if let Some(design) = design
        && let Ok(document) = roxmltree::Document::parse(&design)
    {
        let depth = component_entry_depth(document.root_element());
        if depth > MAX_EXEMPT_ENTRY_DEPTH {
            return Some(KnownLimit::EntryTree { depth });
        }
    }
    let mut elements = 0;
    for (_, bytes) in package.entries() {
        let Ok(text) = format_xml::xsd::decode_schema_bytes(bytes, u64::MAX) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let root = document.root_element();
        if root.tag_name().name() == "schema" && root.tag_name().namespace() == Some(XSD_NAMESPACE)
        {
            elements += schema_components(root);
        }
    }
    (elements > MAX_EXEMPT_XSD_ELEMENTS).then_some(KnownLimit::Schemas { elements })
}

/// The most `<entry>` elements on one path inside a `<component>`, counted
/// without recursion. Entries elsewhere, such as under `<resources>`, are
/// not walked by the entry-tree importers.
fn component_entry_depth(root: roxmltree::Node<'_, '_>) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root, 0usize, false)];
    while let Some((node, depth, in_component)) = stack.pop() {
        let name = node.tag_name().name();
        let in_component = in_component || name == "component";
        let depth = depth + usize::from(in_component && name == "entry");
        deepest = deepest.max(depth);
        stack.extend(
            node.children()
                .filter(|child| child.is_element())
                .map(|child| (child, depth, in_component)),
        );
    }
    deepest
}

/// Elements below `root` named in [`XSD_COMPONENTS`], in any namespace,
/// outside `annotation` subtrees, counted without recursion.
fn schema_components(root: roxmltree::Node<'_, '_>) -> usize {
    let mut count = 0;
    let mut stack: Vec<_> = root.children().filter(|child| child.is_element()).collect();
    while let Some(node) = stack.pop() {
        let name = node.tag_name().name();
        if name == "annotation" {
            continue;
        }
        count += usize::from(XSD_COMPONENTS.contains(&name));
        stack.extend(node.children().filter(|child| child.is_element()));
    }
    count
}

fn entries_of(package: &MfdPackage) -> Vec<(String, Vec<u8>)> {
    package
        .entries()
        .map(|(path, contents)| (path.to_owned(), contents.to_vec()))
        .collect()
}

/// An admitted package holds exactly the supplied entries within the limits,
/// sorted by path, with case-distinct lookup.
fn check_admitted(candidate: &Candidate, package: &MfdPackage) {
    assert_eq!(package.design_path(), candidate.design);
    let mut expected = candidate.entries.clone();
    expected.sort();
    assert_eq!(
        entries_of(package),
        expected,
        "admitted entries differ from the supplied entries"
    );
    assert!(package.entries().len() <= MAX_PACKAGE_ENTRIES);
    let mut total = 0;
    for (path, contents) in package.entries() {
        assert!(path.len() <= MAX_PACKAGE_PATH_BYTES);
        assert!(contents.len() <= MAX_PACKAGE_FILE_BYTES);
        total += contents.len();
        match package.get(path) {
            Ok(found) => assert_eq!(&*found, contents, "get({path:?}) returned other bytes"),
            Err(error) => panic!("get({path:?}) of an admitted entry failed: {error:?}"),
        }
        for alias in [path.to_ascii_uppercase(), path.to_ascii_lowercase()] {
            if alias != path {
                assert_eq!(
                    package.get(&alias).err(),
                    Some(PackageError::MissingEntry),
                    "case alias {alias:?} of {path:?} resolved"
                );
            }
        }
    }
    assert!(total <= MAX_PACKAGE_BYTES);
    for invalid in ["", "../x", "/x", "a\\b", "C:/x", "a//b"] {
        assert_eq!(
            package.get(invalid).err(),
            Some(PackageError::InvalidPath),
            "get({invalid:?}) did not refuse the path"
        );
    }
}

/// A comparable import result: the outcome plus the serialized project.
#[derive(Debug, PartialEq, Eq)]
struct Summary(Import, Option<Result<String, String>>, Option<String>);

fn summarize(result: Result<mfd::Imported, MfdError>) -> Summary {
    match result {
        Ok(imported) => Summary(
            Import::Imported {
                warnings: imported.warnings,
            },
            Some(serde_json::to_string(&imported.project).map_err(|error| error.to_string())),
            Some(imported.mapping_path.display().to_string()),
        ),
        Err(MfdError::Package(error)) => Summary(Import::Refused(error), None, None),
        Err(error) => {
            let debug = format!("{error:?}");
            let variant = debug
                .split(|character: char| !character.is_alphanumeric())
                .next()
                .unwrap_or_default()
                .to_owned();
            Summary(
                Import::Failed {
                    variant,
                    message: error.to_string(),
                },
                None,
                None,
            )
        }
    }
}
