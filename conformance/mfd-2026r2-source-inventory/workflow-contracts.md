# Workflow contracts for the remaining context leaves

The [catalog](workflow-contracts.tsv) gives **358 synthetic cases across the
339 topics** that had only a triage observation after the mapping review.
There are now examples for all 462 leaves in the original cohort: 30 manager
pages, 93 mapping pages and 339 workflow pages. **Zero remain at triage depth.**
Each count is by distinct `page_index`. A topic with three cases still counts
once; a one-case topic may have other operations that need separate cases.

## Reading the catalog

Filter `family` to follow one of 25 related sets of examples. Each row gives a
setup or action (`synthetic_input`), the documented `expected` result, and an
installer PDF page and short `locator`. For example:

- **Flat files and workbooks:** Feed three records to a fixed Excel range
  with Count = 2. Only two are written, with no warning (topic 226).
  A *source* range configured as dynamic instead reads the remaining rows and
  can leave a later range with none. Compare the narrower
  [generated flat workbook profile](../../docs/generated-flat-xlsx.md), which
  expressly excludes update-existing and column selections.
- **PDF extraction:** For pages matching a heading on 2, 4 and 5, grouping
  consecutive matches gives `[2]` and `[4,5]`. Choosing independent groups
  separates the last two (314). Once a region is split, a below-threshold
  strip is discarded by default; it can instead merge into the previous
  non-small fragment (320). Neither rule determines OCR accuracy.
- **HTTP:** Map a response's status and body without a common target parent
  in the two-output example (785): the manual predicts two requests. An HTTP
  404 with no matching response-body definition terminates execution (786).
  Call count and error handling need different tests.
- **Deployment:** The RaptorXML batch wrapper resolves a resource to the path
  from the active configuration (859); a server `.mfx` preserves the
  reference and requires server-side definitions (860). A FlowForce
  resource is a permissioned object (861).

The other families follow design edits and multi-stage XML tutorials; XML
signature creation and verification; connector and driver prerequisites;
EDI, XBRL, JSON and protobuf; extraction editor commands; external functions;
SOAP and GraphQL; certificates, credentials and dynamic node names; debugging;
catalog resolution; IDEs and generated wrappers; the generator template
language; application options, engine profiles and host extensions.

## Source and ledger joins

`page_index` is the key into [documentation-pages.json](documentation-pages.json).
Use that entry for the official topic path, section and existing `joins`.
`pdf_page` in the TSV is a **one-based physical page of the English Enterprise
2026r2 installer PDF**, identified by its SHA-256 in the
[context review](context-leaf-review.md). Do not substitute page numbers from
the separately pinned public PDF. No installer file is required for the
repository-only partition check.

The 339 topics join to 576 distinct [research contexts](../mfd-2026r2.source-contracts.json).
All have a null direct `canonical_id`. Across 275 topics, 512 contexts name
one of 19 broad ledger owners; the other 64 topics each have one context and
no broad owner. Resolve **all** existing joins rather than taking the first
one or creating a new capability from the topic name. A broad owner's
[support assessments](../mfd-2026r2.json) remain scoped to that owner. For
example, a `formats.xlsx` assessment is not an assessment of silently dropping
the third row of a fixed output range. None of the examples supplies new
evidence for import, interpretation, native export, self-roundtrip, GUI,
debugging, generated Rust/C#, or a vendor execution backend.

## Gaps and independent review

- **Source access:** all 339 topic bodies were available in the pinned
  installer manual. The catalog cites a bounded expectation even for settings
  and reference topics. The number of further independently testable
  operations *within* these bodies is unknown. The 64 topics without broad
  owners still need an ownership decision; see `joins` rather than inferring
  an owner from adjacent topics.
- **Limits of the cited evidence:** driver versions in the connection examples
  (131–151) are not version-support intervals. The remote service tutorials
  (794–797) do not establish current endpoint or authorization behavior.
  OCR setup (327–328) gives no accuracy threshold for other scans. Java and
  .NET conversion tables (1113–1114, 1119–1120) do not qualify an installed
  host ABI. Use the existing [58-row matrix](installer-support-matrices.md)
  for documented backend marks and its footnotes, not these examples.
- **Outside this cohort:** the `db-merge-depth` action-name discrepancy
  remains in [mapping-contracts.tsv](mapping-contracts.tsv). Named function
  defaults and failures, generated-wrapper completeness and the release
  binding of the separate online COM catalog retain their
  [existing dispositions](README.md). Page coverage does not close those gaps.
- **Two unresolved FlexText boundaries:** the floating Split once
  description (209, p. 406) defines the second fragment as the text *after*
  the separator, but also says the first container remains empty if the
  separator is first **or last**. For `AB:` those descriptions disagree about
  leading fragment. The line-based split (210, pp. 407–408) likewise describes
  both keeping preceding lines and leaving the first container empty when the
  matching line is last. For two lines, `pre` followed by `x#1`, those rules
  disagree about `pre`. The interior examples avoid both ambiguities.
- **Independent review:** All 358 workflow examples were compared with their
  source sections: 55 in the first pass and 303 in the second. The review
  clarified settings and outcomes for chained previews, SQL result sets,
  FlexText assembly, EDI separators and debugger output, among others.
  All 576 workflow context references resolved in both directions. Across
  789 canonical records, 6,228 of 6,312 vendor-backend assessments remain
  unassessed; the other 84 are marked not applicable. Additional operations
  within these topics and their applicability are tracked in
  [#820](https://github.com/DeandreT/ferrule/issues/820);
  `inventory_complete` remains false.

Run `python verify_context_leaf_review.py` here to check membership, depth,
case fields, duplicate examples, physical extents and research-context joins.
Pass `--pdf PATH` to verify the manual's hash, topic bookmarks and the
in-section citations for the 112 mapping and 358 workflow cases. A locator
match establishes where to read, not whether the interpretation is correct.
