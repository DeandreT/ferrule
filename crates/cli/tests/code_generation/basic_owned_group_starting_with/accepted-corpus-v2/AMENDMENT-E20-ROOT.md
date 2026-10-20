# E20-root fixture amendment

E20-root's root-iteration (`Source[]`) failure predicate originally compared the
owned root field `Limit` (node 7). Owned failure eligibility covers only rules
over one nonempty static Primary collection segment, so every generation
profile, including StartingWith, refuses that read with a `FailureRule`
capability diagnostic. The case expects the rule to raise `global.reject` in all
three engines, which requires an ordinary root rule.

The predicate now reads `Limit` through an ordinary `SourceField` (node 54). The
shared E20/C03 arm also adds nodes 53 and 54 to its other four profiles, which
do not reference them. The compared value, expected results, catalogues,
profile/case/call counts and every other fixture are unchanged. Among the sealed
files, only `source/conformance/basic-owned-group-starting-with/projects.rs` and
this note differ; `PACKET.sha256`, `PACKET.sha256.sha256` and the `ACCEPTED`
constant in `historical.rs` carry the new seal.
