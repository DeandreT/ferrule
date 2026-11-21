# Ownership of the 64 previously unassigned workflow topics

[Workflow owners](workflow-owners.tsv) records **one decision for each of
the 64 topics** whose original research context has no broad owner. Join its
`page_index` to the [workflow examples](workflow-contracts.tsv) for the
documented operation, hypothetical input, physical installer page and locator.
The frozen research context remains an accurate record of earlier coverage.

- **Match: 30 topics.** The existing native owner covers the operation at
  family level. For example, supplying the constant request argument in the SOAP call (796) belongs to
  `connectors.http-services`. The match does not transfer the owner's evidence
  to an individual example.
- **External: 18 topics.** The action belongs to another system. Installing
  a trusted Windows root (800), editing a stylesheet in StyleVision (829),
  and managing permissions on a FlowForce resource (861) have no native
  capability ID for those actions.
- **Missing from the original ledger: 16 topics.** They now have eight
  [native-profile owners](../mfd-2026r2.json). Several tutorials can exercise
  the same independently testable operation:

| New owner | What distinguishes it | Topics |
| --- | --- | --- |
| `authoring.assisted-connections` | Request wire suggestions using design context. | 805 |
| `authoring.stylesheet-documentation` | Select a report stylesheet and produce a call graph or impact report. A menu command identifier accounts for dispatch only. | 828 |
| `mapping.dynamic-node-names` | Read actual child names as data as the children are visited. | 808, 810 |
| `mapping.dynamic-node-type-selection` | Expose allowed child content through a selected schema type. | 809 |
| `interop.named-credential-resolution` | Resolve imported and local definitions of the same credential name. | 820 |
| `connectors.oauth-credential` | Configure an authorization grant and use its token on a mapping call. | 797, 821 |
| `interop.configured-resource-alias` | Define a logical reference and select its binding, including which mapping output a result resource denotes. | 852–857 |
| `deployment.credential-transfer` | Select populated sensitive fields for transfer; deploy a credential independently from the mapping. Name resolution does not determine what is sent. | 823, 824 |

At `12119a80` the partition was **29/22/13**. Four formerly external topics
changed primary owner: 803 → `deployment.host-api`; 823 and 824 →
`deployment.credential-transfer`; 857 → `interop.configured-resource-alias`.
The [seven source facets](reviewed-workflow-facets.tsv) give the designer,
invocation and external steps separately. Topic 861 remains external for the
stated two-user permissions case; that decision does not classify the whole
section. Topics 822 and 860 cover standalone invocation only.

The 64 topics contain **66 original examples** (820 and 1091 each have two):
31 match, 18 external, 17 missing at baseline. The facets do not increase
that denominator. At `2bd2311d` the ledger had **797 records**: the prior 796 plus the
transfer ID, with 16 unassessed cells and no evidence. Relative to the 789
baseline, all 128 added cells are unassessed, including 64 vendor cells.
Of 6,376 vendor cells, 6,292 are unassessed and 84 remain not applicable.
The [358-case workflow review](workflow-contracts.md#gaps-and-independent-review)
comprised 55 initial cases and 303 further cases; it is distinct from review
of the ownership and wrapper supplements. The [subsequent adapter review](adapter-operations.md)
uses a separate matrix-row denominator.

## Optional inputs in the named function catalog

[Optional-input cases](optional-inputs.tsv) cover **all 55 explicitly optional
slots on 40 of the 319 named function leaves** in [functions.json](functions.json).
Each `topic_pages` pair is `page_index:physical_installer_page`. Multiple
pairs share a case only when the same qualification applies. Parameter names
identify the existing function records without repeating signatures.

Of the 55 slots, 27 have a documented default or omission behavior, 10
describe a context or counter control without a literal default, one
(`replace.flags`) refers to `matches.flags`, and **17 received a
[second source review](optional-review.md)**. For example, an omitted `must-exist`
causes a failed binary-file open to throw, whereas explicit false returns
empty binary. Core date formatting defaults to English; that rule cannot
be transferred to the separately documented XPath 3 formatting functions.

The second review identifies five standard-defined fallbacks, one manual
no-flags case, three jointly illustrated ports, and eight slots without a
documented omission value. The remaining
410 parameter rows comprise 105 mandatory inputs, 304 unmarked inputs and
one unmarked output (`sleep.data`, 625). Thus there are 465 parameter rows
but 464 inputs. The [seven additional leads](optional-leads.tsv) are a subset
of those 304 inputs: six inherited calendar/place descriptions and one
conditional MIME encoding. They are not seven new explicit optional tags.
The same table identifies the output separately. Thirty-five leaves have
no parameter rows; that does not establish arity zero. Diagrams without
parameter rows, extensible arities, empty-sequence behavior and error cases
outside these examples are not resolved by counting ports.

## Generated schema wrappers

[Wrapper cases](wrapper-cases.tsv) bind **all 120 existing canonical member IDs**
to examples or explicit source limits on 12 class-reference pages: document, element type, member
attribute and member element for C++, C# and Java. For example, C++ document
destruction invalidates retained node references; deleting an entire member
collection differs from deleting one indexed occurrence; the C# element
`Value` property is conditional on its content model. See the original
[member contracts](../mfd-2026r2.source-contracts.json) for signatures.

There are 40 identities per language. Use `canonical_id` to look up the exact
`member_identity` (language, generated owner, displayed signature and kind)
in `source_units.API`; `page_index` joins the release topic. The generator
families are C++ (905–908), C# (917–920) and Java (928–931). These are manual
class templates. C++ line-ending overloads on 905 explicitly require the
Xerces3 generator; the other rows do not certify every C++ generator.

There are 122 physical declaration rows. Two pairs of C++ `SaveToFile` rows
on p. 1440 disagree on the role of a Boolean with otherwise identical types
(IDs ending `59977` and `59983`). Each pair is one identity.
The same C++ table prints `std:vector` on the binary loader. The C#
`SaveToString` rows on p. 1455 return `string` but describe writing to a
file. The Java enum setter on p. 1473 calls its parameter `index` but
describes a generated value constant. The cases preserve those limits.

Zero of these 120 identities remain without a case. Other generated classes, schema-specific declarations
and inherited members are outside this bound. The four
[generated-source profiles](schema-wrappers.json) record distinct C++, C#
and Java artifacts; the documentation examples are not receipts for those
artifacts. Exception classes for invalid indices, target-specific lifetimes,
compiled libraries, exact dependencies and native ABI remain unqualified.

Run `python verify_workflow_owners.py` for the ownership partition, original
ledger preservation, empty assessment checks and slot/member joins. With
`--pdf PATH` it verifies 176 citations (164 optional-input and wrapper, five
lead/output and seven mixed-workflow citations)
inside their topic bookmarks and checks the declaration cells against the
existing signatures, including overloads and the two conflicting rows. Run
`verify_context_leaf_review.py --pdf PATH` for the original workflow
citations. Independent review of these additions remains open under
[#820](https://github.com/DeandreT/ferrule/issues/820); `inventory_complete`
remains false.
