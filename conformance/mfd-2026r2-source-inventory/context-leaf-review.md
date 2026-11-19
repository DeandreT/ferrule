# What the 462 context-only leaves contain

The [page-by-page review](context-leaf-review.csv) joins every
`compared_context_or_control_only` entry in [the frozen page
inventory](documentation-pages.json) to its numbered section in the English
manual shipped with Enterprise 2026r2. All 462 are leaves. The index is the
existing zero-based page identity, **not** a PDF page number. The two page
columns are one-based physical pages in the [pinned installer
manual](installer-support-matrices.md), SHA-256
`9cf5bfa17ec38140941566dd15b19a9c32185339b04fb7383674f50452f43a09`.
The corresponding online topic, public-manual line range and previous
registry joins remain available at `documentation-pages.json#/pages/<index>`.

| Body classification | Pages | Criterion |
| --- | ---: | --- |
| Operation | 290 | The selected observation identifies an action or workflow described in the body. |
| Reference | 172 | The selected observation identifies a rule, setting, constraint, type or environmental condition. The page may also contain instructions. |
| Navigation only | 0 | No leaf in this cohort was found to consist solely of pointers to other topics. |

For example, the PDF editor's **Help** topic (343) specifies which local help
document opens. The **XQuery 1.0** topic (1091) starts with a list of links but
goes on to define schema-awareness and encoding behavior. Conversely, a
procedure for installing an external database driver is meaningful source
material, but is not proof that Ferrule implements a connector.

The CSV gives one identifying observation per leaf, not a complete contract
for each operation on that leaf. Of the 462, **30** have the bounded
manager contracts below; **93** have [mapping contracts](mapping-contracts.md)
and **339** are still at triage depth. The original
462-page classification remains the baseline for this comparison. The
families' parent pages (`97`, `264`, for example) already have separate body
reviews and are not counted again.

## Schema and taxonomy package managers

Each row pairs a source rule with an independently authored example. The
numbers in the first column identify the CSV row by `page_index` and link it
to the existing research ID. The sections and physical page numbers are for
the installer manual. Examples are expected outcomes from the documentation,
not executions. Names such as `harbor-1.0` are hypothetical catalog entries.

### XML Schema Manager

| Index | Section · PDF pages | Contract | Example |
| ---: | --- | --- | --- |
| 93 | 4.3.9.1 · 177–179 | The GUI starts with installed schemas selected. Selection edits are pending until Apply. A downloaded selection file can open the GUI with proposed installs. | Opening a selection file for a new schema lets the operator inspect the pending change before installing it. |
| 94 | 4.3.9.2 · 179–181 | A partially selected version group differs from an all-selected group. A highlighted change can be an install **or** a removal. | An unchecked, highlighted version that was installed is pending removal, not an available version waiting for installation. |
| 95 | 4.3.9.3 · 181–182 | Patch Selection stages available patches together; Apply commits them. Installing a schema also installs referenced schemas. | If `harbor-1.0` references `ports-1.0`, installing `harbor-1.0` brings in `ports-1.0` as well. |
| 96 | 4.3.9.4 · 182–183 | GUI Reset Selection restores the installed-state display. CLI reset removes installed schemas and the cache. | To discard an accidental pending deselection without uninstalling anything, use Reset Selection. |
| 98 | 4.3.9.5.1 · 183–184 | Help after the command describes that command; help directly after the executable requests general help. | `info -h` requests information about `info`; `-h info` selects general help. |
| 99 | 4.3.9.5.2 · 184 | `info` accepts one or more full or partial names and reports metadata, references and installed status. Use `list` for package identifiers. | Inspect the publisher and dependencies of schemas matching `harbor` before choosing a version to install. |
| 100 | 4.3.9.5.3 · 184–185 | `initialize` (`init`) creates the cache. The first schema-aware product installation normally does this automatically. | Following a bare reset, initialize the environment before refreshing its catalog. |
| 101 | 4.3.9.5.4 · 185 | Install takes versioned identifiers, unique abbreviations, or a `.altova_xmlschemas` selection file; an abbreviation resolves to the latest version. | Given versions 1.0 and 2.0 of a uniquely named schema, use `harbor-1.0` to pin the older one rather than `harbor`. |
| 102 | 4.3.9.5.5 · 185–186 | `list` (`ls`) lists available schemas by default. Name fragments, installed-only and upgradeable-only filters narrow the result. The synopsis shows one optional name; the body permits several. | A two-fragment search for `harbor` and `lock` lists matches for either fragment. `list -u` instead asks for patch candidates. |
| 103 | 4.3.9.5.6 · 186–187 | Reset removes installed schemas and the cache. Initialization and catalog refresh are separate options, both off by default. | `reset -i -u` requests removal, reinitialization and refresh; a bare reset does not request the latter two steps. |
| 104 | 4.3.9.5.7 · 187 | Uninstall removes referenced schemas by default. `--keep-references` retains them. The text allows a unique abbreviation but also describes removing all names containing it; ambiguous input is unresolved. | To remove only `harbor-1.0`, name that version explicitly and request `--keep-references`. |
| 105 | 4.3.9.5.8 · 188 | `update` retrieves the list of available schemas from online storage and refreshes the local cache. | After initialization, `update` refreshes which revisions can be found; it is not the operation for patching installed schemas. |
| 106 | 4.3.9.5.9 · 188 | `upgrade` applies available patches across installed schemas. It removes a deprecated schema if there is no newer version. | With a patch for `harbor` and no successor for a deprecated `lock`, an upgrade can patch one and remove the other. |

### Taxonomy Package Manager

| Index | Section · PDF pages | Contract | Example |
| ---: | --- | --- | --- |
| 258 | 4.8.5.1 · 518–520 | The pre-2026r2 taxonomy store and the package store are separate. With an old nonempty store and no new store, the migration wizard can populate the new store or leave it empty; the old store stays unchanged. | Declining the offered migration does not erase taxonomies used by an older product. |
| 259 | 4.8.5.2 · 520–523 | Desktop users can open the GUI or CLI after installation. A validation needing missing packages can prompt for installation. | A missing package during validation can open a simplified install dialog rather than requiring a manual search from the main window. |
| 260 | 4.8.5.3 · 523–524 | Installed state, pending selection, partial group selection and patch availability have distinct indicators. | Selecting an uninstalled version can make it checked and yellow; that alone does not mean it is installed. |
| 261 | 4.8.5.4 · 525–526 | Apply installs staged patches together. Deselecting a package marked for patching queues removal instead. Installation brings in the referenced packages. | If the intent is to skip one patch, do not assume that clearing the package's installed checkbox means “leave this version alone.” |
| 262 | 4.8.5.5 · 526–527 | GUI Reset Selection cancels pending choices. GUI uninstallation lists referenced packages and requests confirmation. CLI reset removes the installed packages and cache. | After a tentative bulk deselection, Reset Selection restores the displayed installed set; Apply would pursue removal. |
| 263 | 4.8.5.6 · 527–530 | Global and per-user cache locations are separate. Network settings distinguish time between transfers (40 s default) from connection establishment including handshakes (300 s default). | Switching to a per-user folder does not make globally cached packages available there; the same packages must be installed again if needed. |
| 265 | 4.8.5.7.1 · 530–531 | Help accepts an optional command name; placement of the help flag determines general versus command-specific help. | `help upgrade` asks about patching packages, whereas `-h upgrade` requests general help. |
| 266 | 4.8.5.7.2 · 531 | `info` accepts full or partial names and reports package metadata, dependencies and installed status. | Query two partial names to inspect their publishers before deciding which identifiers to install. |
| 267 | 4.8.5.7.3 · 531–532 | `initialize` (`init`) creates the package-information cache. The first XBRL-enabled product install normally initializes it. | An environment deliberately reset without reinitialization can have its cache recreated with `init`. |
| 268 | 4.8.5.7.4 · 532–533 | Install accepts multiple space-separated package identifiers; a unique abbreviation selects the latest version. | Use the full `harbor-1.0` plus `locks-2.0` to request two fixed versions in one installation. |
| 269 | 4.8.5.7.5 · 533 | `list` (`ls`) lists available packages by default and supports installed-only, upgradeable-only and name filters. The body permits multiple name strings despite the singular synopsis. | Before patching, `ls -u` asks for upgradeable packages, not the entire installed set. |
| 270 | 4.8.5.7.6 · 533–534 | `migrate-xbrl` (`xbrl2tp`) selects old taxonomies by substring of the **entry-point URL**, or by all-installed selection. Test mode previews without installation; a missing counterpart warns in test mode and errors in an actual migration. | A test selection for `harbor.example` can match two different old entry points on that host. Review both candidates before requesting any installation. |
| 271 | 4.8.5.7.7 · 535 | Bare reset removes installed packages and the cache. The init and update options request the subsequent setup steps. | Resetting with `-i` recreates the initial state; refreshing the available list is a separate request. |
| 272 | 4.8.5.7.8 · 535–536 | Search matches a full or partial entry-point URL **in the package store**; global is the default, with a user-folder option. The heading says `search-ep-pkg`, the synopsis says `search-ep-package`, and the synopsis and examples use `ep-pkg`. | A lookup for `/harbor/forms/` in the user package store need not return a package installed only in the global store. The example uses the documented `ep-pkg` alias; the two long spellings are not assumed equivalent. |
| 273 | 4.8.5.7.9 · 536–537 | Uninstall normally removes referenced packages too. `--keep-references` retains them. The page describes a full identifier or unique abbreviated name, but does not specify how an ambiguous abbreviation fails. | Removing `harbor-1.0` with `--keep-references` retains its referenced `ports-1.0`. |
| 274 | 4.8.5.7.10 · 537 | Update retrieves the available package list and refreshes the local cache. | A freshly initialized cache can discover a new package without installing that package. |
| 275 | 4.8.5.7.11 · 537–538 | Upgrade applies available patches to installed packages. A deprecated package with no replacement is removed. | A discovery-only check uses `list -u`; `upgrade` can change or remove installed packages. |

These examples specify command selection and documented state changes only.
Actual catalog contents, exit codes, rollback after partial failure and
dependency retention when multiple owners share a package require additional
evidence. Product setup facilities are not MFD graph operations. No ledger
support status or vendor-backend applicability follows from these rows.
