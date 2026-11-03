# Counted-field XML contract

These generic MFD, schema and literal whole-output fixtures are independently
authored for Ferrule. No private data, vendor files or reference-product outputs
are used. They inherit the repository's GPL-3.0-only license.

The predicate reads exactly the counted optional String field. Inclusion and
exclusion count present occurrences only, in document, batch and current-record
frames. The mixed case includes an absent field, an empty field and equal values
in separate batches. Every output field and nested item is specified literally.

The complete typed tree and JSON values must match; the serialized XML is parsed
and compared with the complete literal XML tree. Indentation is not part of the
mapping contract. This fixture directory is separate from the retained
`filtered-counts` catalog so its generated-host resource copy stays unchanged.
