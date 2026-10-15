# GitHub Pages deployment

The public site is [deandret.github.io/ferrule](https://deandret.github.io/ferrule/).
The [site workflow](../.github/workflows/site.yml) builds the WebAssembly demo,
combines it with `site/`, uploads the `github-pages` artifact, and deploys that
artifact through the `github-pages` environment. Repository Settings → Pages →
Build and deployment → Source must be **GitHub Actions** (`build_type: workflow`).
This repository setting persists separately from the workflow file.

## Repair of #247 on 2026-10-15

Main commit `4c1aa3805a012886d19dcc8730dff424f25b6ef5` built successfully in
[site run 38785376772](https://github.com/DeandreT/ferrule/actions/runs/38785376772).
Its deployment creation failed with HTTP 404 and an explicit instruction to
enable Pages. Authenticated repository metadata reported `has_pages: false`, the
account had repository administration permission, the Pages API returned 404,
and the public project URL returned 404. Together these observations confirmed
that the repository had no enabled Pages site.

The smallest correction was one repository configuration request:

```http
POST /repos/DeandreT/ferrule/pages
Content-Type: application/json

{"build_type":"workflow"}
```

No site content, Rust code, workflow permissions, environment rules, custom
domain or build recipe changed. The existing workflow already declares
`pages: write`, `id-token: write`, `needs: build` and the `github-pages`
environment, as required by GitHub's
[custom Pages workflow documentation](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
The [Pages REST API](https://docs.github.com/en/rest/pages/pages#create-a-github-pages-site)
documents the configuration request and `workflow` build type.

Only the failed deployment was retried. The original successful build and
artifact were reused. [Attempt 2](https://github.com/DeandreT/ferrule/actions/runs/38785376772/attempts/2)
succeeded at 22:15 UTC; deploy job `116700168331` submitted artifact
`11998061699` for the same main commit. The Pages deployment API reported
`succeed`. The resulting site uses HTTPS with enforcement enabled.

The artifact API digest matched the downloaded ZIP, its complete regular-file
inventory was checked, and all ten static site files matched the repository
source. Every one of the 13 published files subsequently returned HTTP 200 and
matched the artifact's complete length and SHA256: 19,582,604 uncompressed bytes,
including the generated demo JavaScript and WebAssembly module. The
[evidence manifest](pages-deployment-evidence-2026-10-15.json) records the source,
run, artifact, configuration, complete member inventory and public readback.

Browser navigation verified the landing page, documentation link and demo link.
The demo finished loading and displayed `Mapping completed`, with the default
two-order result: `AllIds` was `A-1, B-2`; the order counts/totals were 2/4 and
1/10. This is a deployment/default-load observation, separate from broader GUI
and mapping qualification.

## Diagnosing a future failure

Inspect the actual failed run and repository configuration before editing a
working build or permissions:

```sh
gh api repos/DeandreT/ferrule --jq '{has_pages, permissions}'
gh api repos/DeandreT/ferrule/pages
gh run list --repo DeandreT/ferrule --workflow site.yml --limit 5
gh run view RUN_ID --repo DeandreT/ferrule --log-failed
gh api repos/DeandreT/ferrule/actions/runs/RUN_ID/artifacts
```

Bind the run's main commit and actual artifact ID/digest before retrying. A Pages
API 404 alone is insufficient to distinguish a missing site from inaccessible
configuration. Check repository access, `has_pages`, workflow permissions and the
deployment log together. Preserve any existing publishing source, domain and
environment rules when diagnosing an already-enabled site.

When the successful build's artifact remains unexpired and the configuration
cause has been repaired, retry its failed jobs:

```sh
gh run rerun RUN_ID --repo DeandreT/ferrule --failed
```

Observe the same run to its new attempt and terminal conclusion. Immediately
after the retry request, the API can show `queued` with the previous attempt
number; the observed #247 retry advanced from attempt 1 to attempt 2 shortly
afterward. Re-read the same run rather than submitting another retry. If the
artifact has expired, dispatch the existing site workflow on the intended main
revision to produce a fresh artifact instead.

Verify the deployment result and the public URL, then fetch the root, docs and
demo assets. A successful build or uploaded artifact alone does not establish
publication. Local #247 originals, full downloaded/published bodies, provider
responses, logs and browser screenshots are retained under
`target/qualification/247-pages-1`; selected non-secret identities are checked in.
