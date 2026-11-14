#!/usr/bin/env bash
# The main branch builds from public repositories only. This check reads every Git source that
# a Cargo manifest declares, and fetches every locked source of the tracked Cargo and pnpm
# lockfiles, with no credential, so a source that is not public fails. It also refuses a
# manifest or lockfile URL that carries a credential or uses SSH. The staging branch registers private packages, so CI does not run this check there.
#
#   tools/public-sources.sh [directory]     default: the repository that holds this script
set -euo pipefail

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
fail() { echo "::error::$1"; exit 1; }

git ls-files -z -- 'Cargo.toml' '*/Cargo.toml' 'Cargo.lock' '*/Cargo.lock' 'package.json' '*/package.json' \
  'pnpm-lock.yaml' '*/pnpm-lock.yaml' 'pnpm-workspace.yaml' '*/pnpm-workspace.yaml' >"$scratch/files"
[ -s "$scratch/files" ] || fail "No manifest or lockfile is tracked in $root"
# Other package managers would need their own fetch below.
if [ -n "$(git ls-files -- 'package-lock.json' '*/package-lock.json' 'yarn.lock' '*/yarn.lock')" ]; then
  fail "This check reads Cargo and pnpm lockfiles only; another lockfile is tracked"
fi

# A URL with a user name, a password or a token, and every SSH source, needs a credential.
# Print the place only: the line can hold a secret.
if xargs -0 grep -nHE "(git@[A-Za-z0-9.-]+:|ssh://|://[^/\"'[:space:]]*@)" <"$scratch/files" >"$scratch/credential"; then
  cut -d: -f1,2 "$scratch/credential" | sed 's/^/::error::A source URL carries a credential or uses SSH at /'
  exit 1
fi

# From here no credential is reachable: no user or system Git configuration, no credential
# helper, no prompt, no SSH agent, no token of this job and a new package cache.
toolchains=${RUSTUP_HOME:-$HOME/.rustup}
for name in $(env | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p' | grep -E '(TOKEN|SECRET|PASSWORD|CREDENTIAL|_KEY$|^SSH_|^GIT_|^GH_|^NPM_CONFIG_|^npm_config_|^CARGO_REGISTRIES_|^CARGO_NET_)' || true); do
  unset "$name"
done
export RUSTUP_HOME=$toolchains HOME=$scratch/home CARGO_HOME=$scratch/cargo XDG_CONFIG_HOME=$scratch/config
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/bin/false SSH_ASKPASS=/bin/false
export GIT_SSH_COMMAND=/bin/false GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0=
export CARGO_NET_GIT_FETCH_WITH_CLI=true COREPACK_ENABLE_DOWNLOAD_PROMPT=0
mkdir -p "$HOME" "$CARGO_HOME" "$XDG_CONFIG_HOME"

# Cargo locks only the sources that a package uses. Read every Git source that a Cargo manifest
# or configuration declares, used or not, with no credential.
git ls-files -z -- 'Cargo.toml' '*/Cargo.toml' '.cargo/config.toml' '*/.cargo/config.toml' >"$scratch/cargo-files"
xargs -0 python3 -c '
import sys, tomllib
def walk(value):
    if isinstance(value, dict):
        if isinstance(value.get("git"), str):
            print(value["git"])
        for item in value.values():
            walk(item)
    elif isinstance(value, list):
        for item in value:
            walk(item)
for path in sys.argv[1:]:
    with open(path, "rb") as file:
        walk(tomllib.load(file))
' <"$scratch/cargo-files" | sort -u >"$scratch/declared" ||
  fail "Could not read the Cargo manifests (python3 3.11 or later is required)"
while IFS= read -r url; do
  ok=0
  for _ in 1 2 3; do
    if (cd "$scratch" && git ls-remote --exit-code "$url" HEAD >/dev/null 2>&1); then ok=1; break; fi
    sleep 2
  done
  [ $ok = 1 ] || fail "A Cargo manifest declares a Git source that is not public: $url"
  echo "Read with no credential: $url"
done <"$scratch/declared"

count=0
while IFS= read -r lock; do
  echo "Fetching the locked sources of $lock with no credential"
  cargo fetch --locked --manifest-path "$(dirname "$lock")/Cargo.toml" >"$scratch/fetch.log" 2>&1 ||
    { tail -n 20 "$scratch/fetch.log"; fail "A source in $lock is not public, or the lockfile is stale"; }
  count=$((count + 1))
done < <(git ls-files -- 'Cargo.lock' '*/Cargo.lock')
while IFS= read -r lock; do
  echo "Fetching the locked sources of $lock with no credential"
  # Install into a copy: the checkout keeps no node_modules from this check.
  copy=$scratch/npm-$count
  mkdir -p "$copy"
  for file in package.json pnpm-lock.yaml pnpm-workspace.yaml; do
    [ ! -f "$(dirname "$lock")/$file" ] || cp "$(dirname "$lock")/$file" "$copy/"
  done
  (cd "$copy" && pnpm install --frozen-lockfile --ignore-scripts --store-dir "$scratch/store") >"$scratch/fetch.log" 2>&1 ||
    { tail -n 20 "$scratch/fetch.log"; fail "A source in $lock is not public, or the lockfile is stale"; }
  count=$((count + 1))
done < <(git ls-files -- 'pnpm-lock.yaml' '*/pnpm-lock.yaml')
[ "$count" -gt 0 ] || fail "No Cargo or pnpm lockfile is tracked in $root"
echo "Every locked source of $count lockfiles was fetched with no credential."
