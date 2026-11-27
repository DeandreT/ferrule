# GitHub Pages deployment

The [site workflow](../.github/workflows/site.yml) combines the static files in
`site/` with the built WebAssembly demo, uploads the `github-pages` artifact,
and deploys it to [deandret.github.io/ferrule](https://deandret.github.io/ferrule/).
In **Settings → Pages → Build and deployment**, the source must be **GitHub Actions**
(`build_type: workflow`). This is a repository setting, separate from the workflow.

## Diagnosing a future failure

```sh
gh run list --repo DeandreT/ferrule --workflow site.yml --limit 5
gh run view RUN_ID --repo DeandreT/ferrule --log-failed
gh api repos/DeandreT/ferrule/actions/runs/RUN_ID/artifacts
gh api repos/DeandreT/ferrule --jq '{has_pages, permissions}'
gh api repos/DeandreT/ferrule/pages
```

1. Identify the failed job, source commit and artifact. A successful build or
   upload does not prove deployment.
2. If deployment returns 404, check repository access and the Pages source.
   The workflow needs `pages: write`, `id-token: write`, and the `github-pages`
   environment. A 404 alone does not establish which setting is wrong. Preserve
   any existing custom domain and environment rules.
3. After correcting the cause, an authorized maintainer can retry failed jobs
   on the same run with `gh run rerun RUN_ID --repo DeandreT/ferrule --failed`
   while its artifact is available. If it has expired, rebuild the intended
   revision. Follow the same run through completion before retrying again.
4. Check the public landing page, docs and demo, including the generated JS and
   Wasm and the demo's default mapping result.

See GitHub's [custom workflow requirements](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
