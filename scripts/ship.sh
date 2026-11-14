#!/usr/bin/env bash
#
# Land the current main commit only after CI has validated that exact commit.
#
# GitHub starts workflows only after a ref exists, so a required status check can
# never authorise the push that creates a commit. This helper pushes the commit
# to a throwaway `verify/<sha>` ref, waits for the CI run for that commit, then
# pushes the same commit to main, where the checks have already passed.
#
# Usage: just ship    (or ./scripts/ship.sh)
#
# Environment overrides:
#   SAYMORE_SHIP_WORKFLOW        workflow file name to watch (default ci.yaml)
#   SAYMORE_SHIP_BRANCH          target branch (default main)
#   SAYMORE_SHIP_TIMEOUT_SECONDS total wait budget (default 5400)
#   SAYMORE_SHIP_POLL_SECONDS    poll interval (default 20)

set -euo pipefail

workflow="${SAYMORE_SHIP_WORKFLOW:-ci.yaml}"
branch="${SAYMORE_SHIP_BRANCH:-main}"
timeout_seconds="${SAYMORE_SHIP_TIMEOUT_SECONDS:-5400}"
poll_seconds="${SAYMORE_SHIP_POLL_SECONDS:-20}"

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

command -v gh >/dev/null || die "gh is required to read workflow results"
command -v git >/dev/null || die "git is required"

[[ "$(git rev-parse --abbrev-ref HEAD)" == "$branch" ]] ||
  die "run this from the $branch branch"
git diff --quiet || die "the working tree has uncommitted changes"
git diff --cached --quiet || die "the index has staged changes"

git fetch --quiet origin "$branch"
sha="$(git rev-parse HEAD)"
short="$(git rev-parse --short HEAD)"
git merge-base --is-ancestor "origin/$branch" "$sha" ||
  die "origin/$branch has commits missing here; pull before shipping"

verify_ref="verify/$sha"
printf 'validating %s on %s\n' "$short" "$verify_ref"
git push --quiet origin "HEAD:refs/heads/$verify_ref"

# CI needs a moment to register the run for a freshly pushed commit.
sleep 5

deadline=$(( $(date +%s) + timeout_seconds ))
while true; do
  read -r status conclusion url <<<"$(
    gh run list --commit "$sha" --workflow "$workflow" --limit 20 \
      --json status,conclusion,url,createdAt \
      --jq 'if length == 0 then "queued pending -" else (sort_by(.createdAt) | last | "\(.status) \(.conclusion // "pending") \(.url)") end'
  )"

  if [[ "$status" == "completed" ]]; then
    [[ "$conclusion" == "success" ]] ||
      die "CI failed for $short: $url (the $verify_ref ref is kept for inspection)"
    break
  fi

  if (( $(date +%s) >= deadline )); then
    die "timed out waiting for CI on $short (${url:-no run registered}); the $verify_ref ref is kept"
  fi
  printf '  %s (%s)\n' "$status" "$short"
  sleep "$poll_seconds"
done

printf 'CI passed for %s; landing on %s\n' "$short" "$branch"
git push --quiet origin "HEAD:refs/heads/$branch"
git push --quiet origin --delete "$verify_ref" ||
  printf 'note: %s was not removed; delete it manually\n' "$verify_ref"
