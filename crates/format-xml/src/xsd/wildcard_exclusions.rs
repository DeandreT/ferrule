//! XSD 1.1 wildcard exclusions: `notNamespace` and `notQName`.
//!
//! Import resolves every exclusion to exact expanded names. Explicit QNames
//! and `##defined` resolve where the wildcard is parsed. `##definedSibling`
//! depends on the governing complex type, so until that type is complete the
//! generic `element()` group carries a mark: its nested recursive `element()`
//! occurrence, which otherwise never has a processing mode, carries `strict`.
//! [`resolve`] then adds the type's declared elements and clears the mark.
//! Named complex types keep the mark in their cached content, so each
//! extension resolves a base's wildcard against its own elements.

use std::collections::BTreeMap;

use ir::{
    SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XmlAlternativeKind, XmlExpandedName, XmlNamespace,
    XmlWildcardExcludedNames, XmlWildcardNamespaceConstraint, XmlWildcardNamespaceList,
    XmlWildcardProcessContents,
};
use roxmltree::Node;

use super::{ParseState, ParsedComplexType, wildcard_ports};
use crate::XmlFormatError;

/// Most `notNamespace` tokens one wildcard may list.
const MAX_NOT_NAMESPACE_MEMBERS: usize = 64;
/// Most `notQName` tokens one wildcard may list, repeats included. It bounds
/// the work for a value that repeats a few names many times.
const MAX_NOT_QNAME_TOKENS: usize = 4 * ir::MAX_XML_WILDCARD_EXCLUDED_NAMES;
const VERSIONING_NAMESPACE: &str = "http://www.w3.org/2007/XMLSchema-versioning";
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WildcardKind {
    Element,
    Attribute,
}

impl WildcardKind {
    pub(super) fn error(self, reason: &'static str) -> XmlFormatError {
        match self {
            Self::Element => XmlFormatError::UnsupportedXmlWildcard { reason },
            Self::Attribute => XmlFormatError::UnsupportedXmlAttributeWildcard { reason },
        }
    }
}

/// How unprefixed names and `##targetNamespace` resolve in the document that
/// declares a wildcard.
pub(super) struct NamespaceContext<'a> {
    /// The document's target namespace, or the one a chameleon include adopted.
    pub(super) target: Option<&'a str>,
    /// The namespace a chameleon include adopted, for namespace-less QNames.
    pub(super) adopted: Option<&'a str>,
    /// The document has no target namespace of its own and is reached through
    /// more than one namespace context, so neither of the above is knowable.
    pub(super) ambiguous: bool,
}

impl<'a> NamespaceContext<'a> {
    pub(super) fn new(
        schema: &'a Node<'_, '_>,
        schema_path: &std::path::Path,
        state: &'a ParseState,
    ) -> Self {
        let own = schema.attribute("targetNamespace");
        let adopted = own
            .is_none()
            .then(|| state.substitutions.effective_namespace(schema_path))
            .flatten();
        Self {
            target: own.or(adopted),
            adopted,
            ambiguous: own.is_none() && state.substitutions.namespace_contexts(schema_path) > 1,
        }
    }
}

/// The unqualified `notNamespace` and `notQName` attributes of one wildcard.
pub(super) struct ExclusionSyntax<'a> {
    not_namespace: Option<&'a str>,
    not_qname: Option<&'a str>,
}

impl<'a> ExclusionSyntax<'a> {
    /// Reads the exclusion attributes, refusing the forms the importer cannot
    /// represent. `None` means the wildcard has no exclusions.
    pub(super) fn read(
        wildcard: &Node<'a, '_>,
        kind: WildcardKind,
    ) -> Result<Option<Self>, XmlFormatError> {
        if wildcard.attributes().any(|attribute| {
            attribute.namespace().is_some()
                && matches!(attribute.name(), "notNamespace" | "notQName")
        }) {
            return Err(kind.error(
                "notNamespace and notQName attributes in a foreign namespace are not supported",
            ));
        }
        let syntax = Self {
            not_namespace: wildcard.attribute("notNamespace"),
            not_qname: wildcard.attribute("notQName"),
        };
        if syntax.not_namespace.is_none() && syntax.not_qname.is_none() {
            return Ok(None);
        }
        if wildcard.ancestors().any(|node| {
            node.is_element()
                && node
                    .attributes()
                    .any(|attribute| attribute.namespace() == Some(VERSIONING_NAMESPACE))
        }) {
            return Err(kind.error(
                "vc: conditional-inclusion attributes are not evaluated, so wildcard exclusions under them cannot be resolved",
            ));
        }
        if syntax.not_namespace.is_some() && wildcard.attribute("namespace").is_some() {
            return Err(kind.error("namespace and notNamespace cannot both be present"));
        }
        if kind == WildcardKind::Element
            && wildcard.attribute("processContents").unwrap_or("strict") == "strict"
        {
            return Err(kind.error(
                "a strict element wildcard keeps no generic group, so it cannot refuse excluded names",
            ));
        }
        Ok(Some(syntax))
    }

    /// The `notNamespace` constraint, when present.
    pub(super) fn namespace_constraint(
        &self,
        context: &NamespaceContext<'_>,
        kind: WildcardKind,
    ) -> Option<Result<XmlWildcardNamespaceConstraint, XmlFormatError>> {
        let value = self.not_namespace?;
        Some(not_namespace(value, context).map_err(|reason| kind.error(reason)))
    }

    /// Resolves `notQName` against the wildcard's namespace constraint.
    pub(super) fn names(
        &self,
        wildcard: &Node<'_, '_>,
        context: &NamespaceContext<'_>,
        constraint: &XmlWildcardNamespaceConstraint,
        kind: WildcardKind,
        state: &ParseState,
    ) -> Result<Exclusions, XmlFormatError> {
        let mut exclusions = Exclusions::default();
        let mut defined = false;
        for (index, token) in self
            .not_qname
            .unwrap_or_default()
            .split_ascii_whitespace()
            .enumerate()
        {
            if index == MAX_NOT_QNAME_TOKENS {
                return Err(kind.error("notQName exceeds the 1024-token limit"));
            }
            match token {
                "##defined" => defined = true,
                "##definedSibling" if kind == WildcardKind::Element => exclusions.sibling = true,
                "##definedSibling" => {
                    return Err(
                        kind.error("##definedSibling is not allowed on an attribute wildcard")
                    );
                }
                token => {
                    let name =
                        qname(wildcard, token, context).map_err(|reason| kind.error(reason))?;
                    if !constraint.allows(name.namespace().uri()) {
                        return Err(kind.error(
                            "a notQName name is in a namespace the wildcard does not allow",
                        ));
                    }
                    exclusions
                        .insert(name)
                        .map_err(|reason| kind.error(reason))?;
                }
            }
        }
        if defined {
            if state
                .substitutions
                .unresolved_namespaces()
                .iter()
                .any(|unresolved| constraint.allows(unresolved.as_deref()))
            {
                return Err(kind.error(
                    "##defined needs a closed declaration set, but a matching include or import could not be resolved",
                ));
            }
            if state.substitutions.has_redefinition() {
                return Err(kind.error(
                    "##defined cannot resolve declarations changed by xs:redefine or xs:override",
                ));
            }
            let declarations = match kind {
                WildcardKind::Element => state.substitutions.concrete_global_elements()?,
                WildcardKind::Attribute => state.substitutions.concrete_global_attributes()?,
            };
            for declaration in declarations {
                let namespace = declaration
                    .namespace
                    .as_deref()
                    .and_then(XmlNamespace::qualified)
                    .unwrap_or(XmlNamespace::Unqualified);
                if !constraint.allows(namespace.uri()) {
                    continue;
                }
                let name = XmlExpandedName::new(namespace, declaration.local.clone())
                    .ok_or_else(|| kind.error(NO_EXACT_NAME))?;
                exclusions
                    .insert(name)
                    .map_err(|reason| kind.error(reason))?;
            }
        }
        Ok(exclusions)
    }
}

const NO_EXACT_NAME: &str = "an excluded declaration has no exact expanded name";
const TOO_MANY_NAMES: &str = "wildcard exclusions resolve to more than 256 names";

/// An expanded name as a set key: the namespace URI, absent when
/// unqualified, then the local name.
type NameKey = (Option<String>, String);

fn name_key(namespace: Option<&str>, local: &str) -> NameKey {
    (
        namespace
            .filter(|namespace| !namespace.is_empty())
            .map(str::to_string),
        local.to_string(),
    )
}

/// Expanded names resolved for one wildcard so far.
#[derive(Default)]
pub(super) struct Exclusions {
    names: BTreeMap<NameKey, XmlExpandedName>,
    /// `##definedSibling` waits for the governing complex type.
    sibling: bool,
}

impl Exclusions {
    fn insert(&mut self, name: XmlExpandedName) -> Result<(), &'static str> {
        let key = name_key(name.namespace().uri(), name.local());
        if !self.names.contains_key(&key) {
            if self.names.len() == ir::MAX_XML_WILDCARD_EXCLUDED_NAMES {
                return Err(TOO_MANY_NAMES);
            }
            self.names.insert(key, name);
        }
        Ok(())
    }

    /// Whether a declaration with this identity is excluded. A declaration
    /// without namespace identity is unqualified.
    pub(super) fn excludes(&self, namespace: Option<&str>, local: &str) -> bool {
        self.names.contains_key(&name_key(namespace, local))
    }

    /// The resolved names; an empty set records no metadata.
    pub(super) fn names(
        &self,
        kind: WildcardKind,
    ) -> Result<Option<XmlWildcardExcludedNames>, XmlFormatError> {
        if self.names.is_empty() {
            return Ok(None);
        }
        XmlWildcardExcludedNames::new(self.names.values().cloned())
            .map(Some)
            .map_err(|_| kind.error(TOO_MANY_NAMES))
    }

    /// Records the exclusions on a generic `element()` group, marking it
    /// for [`resolve`] when `##definedSibling` is pending.
    pub(super) fn apply(&self, wildcard: &mut SchemaNode) -> Result<(), XmlFormatError> {
        wildcard.xml_wildcard_excluded_names = self.names(WildcardKind::Element)?;
        if self.sibling
            && let Some(nested) = nested_occurrence(wildcard)
        {
            nested.xml_wildcard_process_contents = XmlWildcardProcessContents::Strict;
        }
        Ok(())
    }
}

fn not_namespace(
    value: &str,
    context: &NamespaceContext<'_>,
) -> Result<XmlWildcardNamespaceConstraint, &'static str> {
    // One token past the limit is enough to refuse the value.
    let tokens = value
        .split_ascii_whitespace()
        .take(MAX_NOT_NAMESPACE_MEMBERS + 1)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err("notNamespace must name at least one namespace");
    }
    if tokens.len() > MAX_NOT_NAMESPACE_MEMBERS {
        return Err("notNamespace exceeds the 64-member limit");
    }
    let mut namespaces = Vec::with_capacity(tokens.len());
    for token in tokens {
        let namespace = match token {
            "##local" => XmlNamespace::Unqualified,
            "##targetNamespace" if context.ambiguous => {
                return Err(
                    "##targetNamespace is unknowable in a schema document included from several target namespaces",
                );
            }
            "##targetNamespace" => context
                .target
                .and_then(XmlNamespace::qualified)
                .unwrap_or(XmlNamespace::Unqualified),
            "##any" | "##other" => return Err("notNamespace cannot contain ##any or ##other"),
            token if token.starts_with("##") => {
                return Err("notNamespace contains an unsupported special token");
            }
            token => XmlNamespace::qualified(token)
                .ok_or("notNamespace contains an empty namespace URI")?,
        };
        if !namespaces.contains(&namespace) {
            namespaces.push(namespace);
        }
    }
    XmlWildcardNamespaceList::new(namespaces)
        .map(|namespaces| XmlWildcardNamespaceConstraint::Not { namespaces })
        .ok_or("notNamespace must name at least one namespace")
}

fn qname(
    wildcard: &Node<'_, '_>,
    token: &str,
    context: &NamespaceContext<'_>,
) -> Result<XmlExpandedName, &'static str> {
    const INVALID: &str = "notQName contains a lexically invalid QName";
    let (namespace, local) = match token.split_once(':') {
        Some((prefix, local)) => {
            if !ir::primary_root_ncname_is_valid(prefix) {
                return Err(INVALID);
            }
            let namespace = if prefix == "xml" {
                Some(XML_NAMESPACE)
            } else {
                wildcard
                    .lookup_namespace_uri(Some(prefix))
                    .filter(|namespace| !namespace.is_empty())
            };
            let namespace = namespace.ok_or("notQName uses an undeclared namespace prefix")?;
            (XmlNamespace::qualified(namespace), local)
        }
        None => {
            let default = wildcard
                .lookup_namespace_uri(None)
                .filter(|namespace| !namespace.is_empty());
            let namespace = match default {
                Some(namespace) => Some(namespace),
                None if context.ambiguous => {
                    return Err(
                        "a namespace-less notQName name is unknowable in a schema document included from several target namespaces",
                    );
                }
                None => context.adopted,
            };
            (namespace.and_then(XmlNamespace::qualified), token)
        }
    };
    XmlExpandedName::new(namespace.unwrap_or(XmlNamespace::Unqualified), local).ok_or(INVALID)
}

/// The nested recursive `element()` occurrence of a generic element group.
fn nested_occurrence(wildcard: &mut SchemaNode) -> Option<&mut SchemaNode> {
    if wildcard.name != XML_ELEMENTS_FIELD {
        return None;
    }
    let SchemaKind::Group { children, .. } = &mut wildcard.kind else {
        return None;
    };
    children
        .iter_mut()
        .find(|child| child.name == XML_ELEMENTS_FIELD && child.recursive_ref.is_some())
}

/// Clears the `##definedSibling` mark, returning whether it was set.
fn take_sibling_mark(wildcard: &mut SchemaNode) -> bool {
    nested_occurrence(wildcard).is_some_and(|nested| {
        std::mem::replace(
            &mut nested.xml_wildcard_process_contents,
            XmlWildcardProcessContents::Skip,
        ) != XmlWildcardProcessContents::Skip
    })
}

/// Whether any `##definedSibling` mark remains in a finished schema.
pub(super) fn has_unresolved_mark(schema: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children, dynamic, ..
    } = &schema.kind
    else {
        return false;
    };
    (schema.name == XML_ELEMENTS_FIELD
        && children.iter().any(|child| {
            child.name == XML_ELEMENTS_FIELD
                && child.recursive_ref.is_some()
                && child.xml_wildcard_process_contents != XmlWildcardProcessContents::Skip
        }))
        || children.iter().any(has_unresolved_mark)
        || dynamic.as_deref().is_some_and(has_unresolved_mark)
}

/// Resolves `##definedSibling` once a complex type is complete, then removes
/// every excluded name from the wildcard's projected ports. Runs before
/// [`wildcard_ports::resolve`], while projected ports are still marked.
pub(super) fn resolve(parsed: &mut ParsedComplexType, state: &mut ParseState) {
    for index in 0..parsed.children.len() {
        if !take_sibling_mark(&mut parsed.children[index]) {
            continue;
        }
        let Some(constraint) = parsed.children[index].xml_wildcard_namespace.clone() else {
            continue;
        };
        let mut exclusions = Exclusions::default();
        let explicit = parsed.children[index]
            .xml_wildcard_excluded_names
            .take()
            .into_iter()
            .flat_map(|names| names.iter().cloned().collect::<Vec<_>>());
        let siblings = explicit
            .map(Ok)
            .chain(
                parsed
                    .children
                    .iter()
                    .filter(|child| wildcard_ports::is_declared_element(child))
                    .map(|sibling| sibling_names(sibling, &state.ambiguous_unqualified_elements))
                    .flat_map(|names| match names {
                        Ok(names) => names
                            .into_iter()
                            .filter(|name| constraint.allows(name.namespace().uri()))
                            .map(Ok)
                            .collect::<Vec<_>>(),
                        Err(reason) => vec![Err(reason)],
                    }),
            )
            .try_for_each(|name| exclusions.insert(name?));
        let applied = siblings
            .map_err(|reason| WildcardKind::Element.error(reason))
            .and_then(|()| exclusions.apply(&mut parsed.children[index]));
        if let Err(error) = applied {
            state.reject_wildcard(error);
        }
    }
    remove_excluded_ports(parsed);
}

/// The expanded names a declared element particle contributes: its own name,
/// its name alternatives and its concrete substitution members. A sibling
/// without namespace identity is unqualified, as the finished import
/// qualifies it beside a `lax` wildcard, unless a schema document reached
/// through several namespace contexts declared it: its namespace is then
/// unknowable.
fn sibling_names(
    sibling: &SchemaNode,
    ambiguous: &std::collections::BTreeSet<String>,
) -> Result<Vec<XmlExpandedName>, &'static str> {
    const NO_EXACT_SIBLING: &str = "a sibling element declaration has no exact expanded name";
    if sibling.xml_namespace.is_none() && ambiguous.contains(&sibling.name) {
        return Err(NO_EXACT_SIBLING);
    }
    let own = sibling
        .xml_namespace
        .clone()
        .unwrap_or(XmlNamespace::Unqualified);
    let mut names = std::iter::once(own)
        .chain(sibling.xml_name_alternatives.iter().cloned())
        .map(|namespace| XmlExpandedName::new(namespace, sibling.name.clone()))
        .collect::<Option<Vec<_>>>()
        .ok_or(NO_EXACT_SIBLING)?;
    if sibling.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup {
        for member in sibling.alternatives() {
            let (namespace, local) = match member
                .name
                .strip_prefix('{')
                .and_then(|name| name.split_once('}'))
            {
                Some((namespace, local)) => (XmlNamespace::qualified(namespace), local),
                None => (Some(XmlNamespace::Unqualified), member.name.as_str()),
            };
            names.push(
                namespace
                    .and_then(|namespace| XmlExpandedName::new(namespace, local))
                    .ok_or("a substitution member has no exact expanded name")?,
            );
        }
    }
    Ok(names)
}

/// Drops excluded names from projected ports, which keep only the names the
/// wildcard can match. A port left with no name is removed, with its member
/// of the wildcard's choice.
///
/// A port that shares its local name with a declared sibling is left to
/// [`wildcard_ports::resolve`] instead, with every excluded name of that local
/// name restored, so coverage is judged as it is without exclusions: a
/// declared sibling covering any of those names takes them, and a partly
/// covered `lax` port is dropped in favour of the generic fallback.
fn remove_excluded_ports(parsed: &mut ParsedComplexType) {
    let Some(excluded) = parsed
        .children
        .iter()
        .find(|child| child.name == XML_ELEMENTS_FIELD)
        .and_then(|wildcard| wildcard.xml_wildcard_excluded_names.clone())
    else {
        return;
    };
    let declared = parsed
        .children
        .iter()
        .filter(|child| wildcard_ports::is_declared_element(child))
        .map(|child| child.name.clone())
        .collect::<Vec<_>>();
    let mut removed = Vec::new();
    parsed.children.retain_mut(|port| {
        if !wildcard_ports::is_projected(port) {
            return true;
        }
        let local = |name: &&XmlExpandedName| name.local() == port.name;
        if declared.contains(&port.name) {
            if let Some(own) = &port.xml_namespace {
                for name in excluded.iter().filter(local) {
                    if name.namespace() != own
                        && !port.xml_name_alternatives.contains(name.namespace())
                    {
                        port.xml_name_alternatives.push(name.namespace().clone());
                    }
                }
            }
            return true;
        }
        let is_excluded = |namespace: &XmlNamespace| {
            excluded
                .iter()
                .filter(local)
                .any(|name| name.namespace() == namespace)
        };
        let namespaces = std::iter::once(
            port.xml_namespace
                .clone()
                .unwrap_or(XmlNamespace::Unqualified),
        )
        .chain(port.xml_name_alternatives.iter().cloned())
        .filter(|namespace| !is_excluded(namespace))
        .collect::<Vec<_>>();
        let Some((first, rest)) = namespaces.split_first() else {
            removed.push(port.name.clone());
            return false;
        };
        if port.xml_namespace.is_some() || !rest.is_empty() {
            port.xml_namespace = Some(first.clone());
        }
        port.xml_name_alternatives = rest.to_vec();
        true
    });
    if !removed.is_empty() {
        wildcard_ports::remove_choice_members(
            &mut parsed.repeating_choices,
            &removed,
            &parsed.children,
        );
    }
}
