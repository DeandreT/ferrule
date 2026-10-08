# Flat Workbook Desktop Setup: 2026-10-08

The normal Linux desktop workflow preserves the worksheet configuration through
schema selection, creation, saving, and reopening for the flat-row fixture below.
The observed application used an ordinary non-test debug build whose GUI
dependency sources were checked against
[commit 1573de551e109bfa04eb8acceb3e97c4d8624324](https://github.com/DeandreT/ferrule/commit/1573de551e109bfa04eb8acceb3e97c4d8624324).
The session used an isolated virtual display and file-dialog session.
This closes the focused desktop setup gate in
[issue #92](https://github.com/DeandreT/ferrule/issues/92).

## Configuration retained

Both schemas are the same closed, non-repeating `Row` group imported from JSON
Schema. All eight fields and their order survive saving and reopening.

| Field | Type | Source column | Target column | Target header |
| --- | --- | ---: | ---: | --- |
| Name | String | 9 (I) | 10 (J) | Person |
| Count | Int | 2 (B) | 2 (B) | Count |
| City | String | 3 (C) | 3 (C) | City |
| Country | String | 4 (D) | 4 (D) | Country |
| Product | String | 5 (E) | 5 (E) | Product |
| Code | String | 6 (F) | 6 (F) | Code |
| Region | String | 7 (G) | 7 (G) | Region |
| Note | String | 8 (H) | 8 (H) | Note |

| Setting | Source | Target |
| --- | --- | --- |
| Format | XLSX flat table | XLSX flat table |
| Sheet | Incoming | Results |
| Header | Skip header row: off | Write header row: on |
| Row | 3, first data row | 5, header row |
| Data path | Not set | Not set |
| Update existing workbook | Off | Off |
| Advanced worksheet layout | None | None |

The row and column numbers are configuration values. This session did not open
an XLSX data file or check cells on a physical worksheet.

## Observed workflow

The six phases used real application controls and local file choosers:

1. Open **File → New** in a 1200 × 800 application window.
2. Choose the source JSON Schema, enable **Configure workbook**, and set the
   source sheet, row, columns, and header choice above.
3. Choose the target JSON Schema, enable **Configure workbook**, and set the
   target sheet, row, columns, header choice, and the first header text above.
4. Reduce the application window to 1200 × 760 and use the New Mapping window's
   outer scrolling to reach **Create**. The run retained twelve actual outer
   wheel events rather than enlarging the window to expose the footer.
5. Create the mapping and save it through the normal file chooser at that size.
   Compare the complete saved project through the public Project decoder and
   serializer against an independently specified expected project.
6. Return to 1200 × 800, reopen the saved project through the normal file chooser,
   and repeat the complete public Project comparison.

Each chooser's actual full-path entry matched its fixed destination before
activation. Reopen identity also used the saved file and complete public
comparison; the clipped on-screen path label alone was insufficient evidence.

Both public comparisons passed. The 3,846-byte project and 310-byte layout
sidecar were byte-for-byte identical before and after reopening. The complete
comparison retained both schemas, every format option, and the empty graph and
scope. The application and file-dialog session closed, with no owned processes
remaining. The complete source inventory and executable identities remained
equal before and after the cohort.

## Qualification boundary

- [x] Choose both flat schemas through ordinary desktop file choosers.
- [x] Configure the source and target independently at ordinary window sizes.
- [x] Reach **Create** with real outer scrolling at 1200 × 760.
- [x] Save and reopen the complete project, worksheet settings, and layout.
- [ ] Execute a connected mapping or qualify Preview/Run in this desktop cohort.
- [ ] Read or write a physical workbook and inspect its cells in this cohort.
- [ ] Extend this desktop check to advanced layouts or other platforms and
  file-dialog backends.

The saved draft has no graph nodes, bindings, or child scopes. Successful setup
and persistence do not establish a runnable mapping. Existing adapter and local
widget tests are separate evidence; this session does not promote their scope.
Earlier failed chooser and capture attempts remain separately retained, and
this successful cohort does not replace their observations. It adds no claim
about general GUI coverage, pixel identity, or diagram rendering.

For ordinary authoring steps, see
[Set up a flat workbook table](../getting-started.md#set-up-a-flat-workbook-table).
For the format's additional layouts, see [Supported formats](../formats.md).
