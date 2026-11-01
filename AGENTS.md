# allowit-sdk

## Artifact storage

Keep code, required build/test inputs and concise API/usage documentation in this repository. Plans, design reports, reviews, run evidence, demo recordings and screenshots belong in the main project wiki repository under `instance/artifacts/`; capture their original Markdown sources with the wiki source-ingestion tooling. Never commit those artifacts in this subproject or its feature branches. Link to the canonical wiki artifact instead.

## Before pushing

Read the affected workflows in `.github/workflows/`. Run their affected test and
validation commands locally, including their feature flags and selectors. Include
workflow and test-input changes when selecting checks. Resolve failures before
pushing. Preserve existing reviews and deployment restrictions.

All packages share one Cargo workspace, one `Cargo.lock` and one version
(`[workspace.package]`). Packages in this repository depend on each other by
path and are never pinned. Select them with `-p allowit-sdk-engine`,
`-p allowit-cli` or `-p allowit-policy-template`.

Pins point one way. This SDK depends on Allowit core (package `allowit-sdk`,
repository allowit-core) and on the rail crates from allowit-contracts-solana,
allowit-contracts-tempo and allowit-contracts-stellar. The rail crates depend
on Allowit core. Never move core or rail code into this repository, and never
add a `[patch]` section. Each git dependency is pinned by full `rev` in the
root `[workspace.dependencies]` (the policy template repeats the core pin), and
`[workspace.metadata.allowit.track]` says which revision it must be pinned to.
Run `scripts/build.sh` before pushing: it fails on a stale pin or on a second
copy of a git package in `Cargo.lock` or on a `Cargo.lock` that no longer
matches the manifests, then builds the workspace. `scripts/build.sh --update`
rewrites stale pins and `Cargo.lock`; `scripts/build.sh --update <repo>...`
rewrites only the pins of the named repositories.
Update the rail repositories to a new core commit before this SDK, so that
`Cargo.lock` holds one allowit-core revision; the build script fails
otherwise.

`allowit-stellar-wallet` on `stellar-core` still depends on `allowit-sdk` from
this repository at 67f5d69. `cli/src/vendors/stellar/run.rs` converts the
execution request between the two core copies and fails closed, and
`cli/tests/stellar.rs` converts its requests the same way. Remove both
conversions when the wallet depends on allowit-core. Until then `Cargo.lock`
holds two copies of package `allowit-sdk`, and `scripts/build.sh` fails.
