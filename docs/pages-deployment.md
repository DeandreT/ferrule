# GitHub Pages deployment

The public site is [deandret.github.io/ferrule](https://deandret.github.io/ferrule/).
The [site workflow](../.github/workflows/site.yml) builds the WebAssembly demo,
combines it with `site/`, uploads the `github-pages` artifact, and deploys that
artifact through the `github-pages` environment. Repository Settings → Pages →
Build and deployment → Source must be **GitHub Actions** (`build_type: workflow`).
This repository setting persists separately from the workflow file.

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
