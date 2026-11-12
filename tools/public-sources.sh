#!/usr/bin/env bash
# The main branch builds from public repositories only. This check reads every tracked manifest
# and lockfile and fails when one names a Git source that an anonymous client cannot read.
# The staging branch registers private packages, so CI does not run this check there.
#
#   tools/public-sources.sh [directory]     default: the repository that holds this script
set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

git ls-files -z -- 'Cargo.toml' '*/Cargo.toml' 'Cargo.lock' '*/Cargo.lock' 'package.json' '*/package.json' \
  'pnpm-lock.yaml' '*/pnpm-lock.yaml' 'package-lock.json' '*/package-lock.json' >"$scratch/files"
[ -s "$scratch/files" ] || { echo "::error::No manifest or lockfile is tracked in $root"; exit 1; }

# A source that needs a key is never public.
if xargs -0 grep -nHE '(git@[A-Za-z0-9.-]+:|ssh://)' <"$scratch/files" >"$scratch/ssh"; then
  cut -d: -f1,2 "$scratch/ssh" | sed 's/^/::error::SSH Git source at /'
  exit 1
fi

# Every Git host form that Cargo, npm and pnpm write: an HTTPS URL, a GitHub tarball URL and the
# github: shorthand. Each becomes one HTTPS repository URL.
{
  xargs -0 grep -ohE 'https?://[A-Za-z0-9.-]+/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+' <"$scratch/files" |
    grep -E '^https?://((codeload\.)?github\.com|gitlab\.com|bitbucket\.org)/' |
    sed -E 's#^http://#https://#; s#^https://codeload\.github\.com/#https://github.com/#; s#\.git$##' || true
  xargs -0 grep -ohE '(github|gitlab|bitbucket):[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+' <"$scratch/files" |
    sed -E 's#^github:#https://github.com/#; s#^gitlab:#https://gitlab.com/#; s#^bitbucket:#https://bitbucket.org/#; s#\.git$##' || true
  # Any other host that a manifest reaches through Git.
  xargs -0 grep -ohE 'git\+https?://[^"#?[:space:]]+' <"$scratch/files" | sed -E 's#^git\+##; s#\.git$##' || true
} | sort -u >"$scratch/repositories"

# Read each repository with no credential: no user or system Git configuration, no helper, no
# prompt and no token of this job.
anonymous() {
  (cd "$scratch" && env -i PATH="$PATH" HOME="$scratch" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/bin/false SSH_ASKPASS=/bin/false \
    git -c credential.helper= ls-remote --exit-code "$1.git" HEAD >/dev/null 2>&1)
}
failed=0
while IFS= read -r repository; do
  ok=0
  for _ in 1 2 3; do
    if anonymous "$repository"; then ok=1; break; fi
    sleep 2
  done
  if [ $ok = 1 ]; then
    echo "public: $repository"
  else
    echo "::error::A manifest or lockfile names a Git source that is not public: $repository"
    failed=1
  fi
done <"$scratch/repositories"
[ $failed = 0 ] || exit 1
echo "Every Git source in the manifests and lockfiles is public ($(wc -l <"$scratch/repositories" | tr -d ' ') repositories)."
