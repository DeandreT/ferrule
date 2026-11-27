# Native saved-design reopen checkpoint

**The first real native phase timed out before producing a saved design.**
The 398-byte XML output was retained; there was no native success result,
`Saved1`, reopen, or structural review. The cause of the timeout is unknown.
This checkpoint covers one independently admitted design, not new sequence,
filter/map or capture forms.

[Protocol](https://github.com/DeandreT/ferrule/issues/263) ·
[XML oracle](https://github.com/DeandreT/ferrule/issues/268) ·
[retained native refusal](https://github.com/DeandreT/ferrule/issues/285).

## Evidence boundaries

| Boundary | Recorded result |
| --- | --- |
| Controller and byte-compatibility stubs | 13 groups, 41 distinct simulated subcases. Original policy and compatibility runs matched the same expectations. |
| Complete XML helper | 34 recipes per phase: 6 positive and 62 negative observations across both phases. |
| XML admission/controller | 17 simulated subcases in seven groups: 14 precreation refusals, one two-phase success and two retained first-phase failures. |
| Staging | 33 originals unchanged; four declared redirects in a 56-node staged tree. Only the owned staging copy was writable during redirection, then frozen before the native call. |
| Staging regressions | One fixture raised `PermissionError` instead of its expected `AssertionError` (40/41; admission unrun). After the [fixture correction](https://github.com/DeandreT/ferrule/issues/283), 41+17 fresh observations matched; the 68 helper observations were inherited. |

There are **126 distinct** original helper/controller observations. Rerunning
frozen cases does not increase that count. None is an application save/reopen.

## Freeze the independent expectation

Expected content comes from the owned input, source/target schemas, and the
independently authored logical expectation. It must be fixed before either
native phase. Prior observed output does not supply the oracle.

The checkpoint's complete expected XML content is:

| Field | Exact expectation |
| --- | --- |
| Document | XML 1.0, UTF-8; no logical nodes before or after the root |
| Root | One element named `Envelope`, with no namespace |
| Selected type | The `type` attribute in the XML Schema-instance namespace resolves to the expanded QName with no namespace and local name `RecordPlus` |
| Data attributes | Exactly the unqualified `Token="token-value"` and `Supplement="supplement-value"` attributes |
| Schema hint | Exactly one `noNamespaceSchemaLocation` attribute in the XML Schema-instance namespace, with the exact fresh staged target-schema URI |
| Namespace declarations | Exactly one nonempty prefix binding for `http://www.w3.org/2001/XMLSchema-instance` |
| Root content | No logical text, element children, comments, processing instructions, entities, or tail content |
| Additional content | No other attributes, namespace declarations, elements, or document nodes |

The base type requires `Token`; the derived type requires `Supplement`;
neither has children. Bind the schema hint to the *fresh staged* target URI
with the declared encoding. A stale but equivalent-looking URI fails.
Freeze the complete profiles and hashes separately for first and reopen
phases before execution. Wrong phases, duplicate keys, missing/extra fields
or a stale hash refuse admission.

## Permitted lexical differences

The explicit complete-tree mode permits these spellings while retaining every
original byte:

- Presence or absence of one UTF-8 byte-order mark.
- An optional XML 1.0 declaration, optionally declaring `UTF-8` or `utf-8`.
- Attribute order and single versus double quotes.
- Predefined or numeric character references that decode to the same values.
- A different spelling of the single nonempty XML Schema-instance prefix.
- An empty-element tag or an equivalent explicit closing tag.
- XML whitespace outside the root element.

The selected type still has to resolve to the same expanded QName. Padding,
case-folding, or an unbound prefix does not change an invalid type into a match.
Whitespace inside the empty root is logical content and refuses comparison.
An extra, default, unused, or duplicate namespace binding also refuses comparison.

The helper consumes the complete document and preserves recorded comments,
processing instructions, ordered children, expanded names, attribute values,
namespace bindings, and parser errors. Recovery, external network access, DTD
loading, entity resolution, and large-tree relaxation are disabled. DTD or entity
declarations, standalone declarations, XML 1.1, and other declared encodings are
outside the admitted profile.

Complete-tree mode is enabled only by the explicitly accepted oracle admission
and exact helper/profile hashes. Existing byte mode remains the default and
retains its whole-byte comparisons. The mode must be fixed before execution;
switching comparison modes after a failure is not a qualification result.

## Frozen negative controls

The 34 literal recipes consist of three positives and 31 negatives. Each is
checked independently for both phases.

| Negative family | Recipes | Required observation |
| --- | --- | --- |
| Wrong or qualified root; missing or wrong selected type | 4 | Complete-tree mismatch |
| Wrong data, missing required data, extra ordinary or Schema-instance attribute | 4 | Complete-tree mismatch |
| Stale or missing schema URI; extra, duplicate, or wrong namespace binding | 5 | Complete-tree mismatch |
| Text, internal whitespace, unexpected children, both unexpected child orders, comment, or processing instruction | 8 | Complete-tree mismatch |
| Duplicate attribute, trailing document content, or malformed root | 3 | Complete parser error |
| DTD/entity declaration, unbound or padded type QName, XML 1.1, standalone declaration, or non-UTF-8 declaration | 7 | Complete refusal/error record |

Both unexpected child orders refuse because the expected tree is empty;
this is not positive child-order evidence. Admission controls check receipt,
identity, exact hashes, fresh URI, profile shape and refusal ordering. A
wrong but schema-valid value and malformed XML both retain the first phase
and bar the second. Byte-mode controls also cover occupied destinations,
changed originals and incomplete evidence copies.

## Two-phase protocol

### Before the first phase

Bind one admitted design, sources, tool/environment identities, accepted
controls, deadlines, process births and exclusive owner. Freeze both profiles
and the schema URI before creating fresh execution, output and save paths.
Keep source originals untouched; redirect only declared staging copies.

### First phase and transition

Open the admitted design, generate output, save `Saved1`, and close the
document and host. Retain and read back all closed originals, logs, errors
and identities before comparing. Require the host result, owned closure,
unchanged guards, schema validity and complete first-phase oracle. On any
refusal, stop. Otherwise bind the exact path, body and identity of `Saved1`;
a seed or an equivalent file cannot stand in for it.

### Reopen phase and final review

A second serial host reopens `Saved1`, generates into a fresh directory
and saves `Saved2` separately. Close and retain the second phase before
comparison. Check the first-phase originals through reopen, both phase
oracles, and the entire saved structure, identities, links and referenced
files. Only declared serializer differences are admissible. A different
reviewer checks the originals, custody and finite result.

## Native results and remaining acceptance

The inner deadline expired at 180.001 seconds. The outer owner exited 1;
normal closure was false, although cleanup, final owned closure, lease
release and source/survey guards were verified. The retained XML matched
the complete oracle under the allowed CRLF spelling on **manual** review;
the controller never reached that comparison.

The record contains 46 execution originals (the historical filename says
48), three outer-owner originals and six first-phase copy pairs. The
25-file frozen packet and 33-file preparation evidence remain distinct.
Only the negative and its custody were accepted. Neither that output nor
the simulated controls supply the missing save, reopen or structural review.
