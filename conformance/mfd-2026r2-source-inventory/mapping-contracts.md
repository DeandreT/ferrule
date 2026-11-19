# Mapping contracts: defaults, cardinality and failures

The [contract catalog](mapping-contracts.tsv) adds 112 documented cases for
93 leaves of the [context review](context-leaf-review.csv). Each case supplies
the condition, a synthetic input or design edit, and an expected result. The
expectation is an interpretation of the cited manual, not an execution receipt.

**Example: how many times does a function run?** With input `[2,8]`, an
ordinary scalar parameter drives two invocations of a UDF. A regular UDF with
a sequence parameter can see both values in one invocation. If its body sends
both items individually to a web-service request, that single invocation
can still make multiple requests. Compare `udf-input-cardinality`,
`udf-output-cardinality` and `udf-inner-fanout` (installer manual pp. 793,
794 and 798). Distinguish all three counts before assessing a mapping.

**Example: what does “missing” mean?** An XML nil element exists, an absent
optional element does not, and an unconnected otherwise input need not emit a
value at all. A default rule can itself specify the empty string. See
`xml-nil-present` (p. 160), `conditional-absence` (p. 734) and
`default-empty-string` (p. 774). These are separate test inputs.

**Example: what survives a database error?** Put the transaction around one
parent and its children and a failing child can undo the entire family. Put
transaction handling only on each child and the same failing child can be the
only row undone. `db-rollback-parent` and `db-rollback-child` both cite p. 302.
For EDI, `edi-incomplete-list`, `edi-local-list` and `edi-code-slices` distinguish
a warning on an external code, a local/global list conflict, and failure at a
specific character position. The [configuration guide](../../docs/edi-configuration.md)
describes the separate Ferrule boundary.

## How to review a case

1. Look up `page_index` in [documentation-pages.json](documentation-pages.json).
   It identifies the numbered topic, public-manual source range, and existing
   research-context pointers. Multiple pointers on a page retain their original
   identities. One new case does not adopt them as capabilities.
2. In the [release-bound English Enterprise installer manual](installer-support-matrices.md),
   open the one-based `pdf_page` and find `locator` **within that section**.
   These pages belong to the 1,750-page installer PDF. The public PDF has
   different pagination. Read the surrounding qualifications as well as the
   short locator.
3. Compare `rule`, `synthetic_input` and `expected`. Some cases test an invalid
   design, some predict zero or more values, and others distinguish preview
   from generated execution. A documented error does not specify an exit code
   or exact diagnostic unless the source says so.

Existing evidence answers narrower questions. For example:

- [Singular target feeds](../../docs/conformance/singular-target-feeds.md)
  distinguishes two wires on a single port from cloned target entries.
  `sequence-singular` here concerns **one sequence on one simple output**.
- [Scalar sequence controls](../../docs/conformance/scalar-sequence-native-controls.md)
  distinguish original ordinals from dense positions. `post-filter-position`
  concerns a particular filtered variable and its consumer; the manual example
  does not establish a general saved representation
  ([#203](https://github.com/DeandreT/ferrule/issues/203)).
- [Computed join keys](../../docs/design/join-computed-keys.md)
  identifies the receiving context and shape restrictions of that work
  ([#1236](https://github.com/DeandreT/ferrule/issues/1236)).
  `sql-join-admission` adds distinct requirements for pushing a join down to a
  database; knowing that two scalar keys compare equal is insufficient.
- The [filename model](../../docs/design/primary-document-filenames.md)
  and [document collections](../../docs/design/document-collections.md)
  distinguish primary acquisition from per-record output. `file-parameter`,
  `dynamic-name-counter` and `dynamic-name-key` illustrate that distinction.

## Remaining coverage

Of the original 462 leaves, 30 have manager contracts, 93 now have mapping
contracts, and **339** remain at triage depth (211 operation facets and 128
reference facets). These are page counts: a reviewed page may still contain
unitemized behavior. The page count is not a count of qualified native capabilities.
Further work includes connector- and driver-specific failure behavior, the
remaining EDI dialects and validation rules, document collections, and backend
restrictions on individual operations. All sixteen ledger assessment
dimensions retain their existing independent evidence requirements.

Run `python verify_context_leaf_review.py` for the repository checks. Use
`--pdf PATH` with the pinned installer manual to check bookmark identity, page
extent and all 112 in-section locators. That check establishes that the citation
can be found; interpretation of the surrounding text still needs review.
