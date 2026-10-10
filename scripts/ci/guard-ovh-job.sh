#!/usr/bin/env bash
set -euo pipefail

# Installed root-owned outside the runner application. Validate GitHub's event
# before any checkout/action step, independently of contributor workflow edits.
exec /usr/bin/python3 -I - <<'PY'
import json
import os
import sys

repository = "DeandreT/ferrule"
try:
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as source:
        event = json.load(source)
    if (os.environ.get("GITHUB_REPOSITORY") != repository
            or event.get("repository", {}).get("full_name") != repository):
        raise ValueError("unexpected repository")
    kind = os.environ.get("GITHUB_EVENT_NAME")
    if kind == "pull_request":
        head = event.get("pull_request", {}).get("head", {}).get("repo") or {}
        if head.get("full_name") != repository:
            raise ValueError("fork pull requests must use GitHub-hosted runners")
    elif kind == "push":
        if event.get("ref") != "refs/heads/main":
            raise ValueError("only main pushes are admitted")
    elif kind == "workflow_dispatch":
        if not os.environ.get("GITHUB_REF", "").startswith("refs/heads/"):
            raise ValueError("manual builds require a repository branch")
    else:
        raise ValueError("event type is not admitted on OVH")
except (OSError, KeyError, ValueError, AttributeError, TypeError) as error:
    print(f"OVH job refused: {error}", file=sys.stderr)
    sys.exit(1)
print("OVH job admitted for a trusted Ferrule repository event.")
PY
