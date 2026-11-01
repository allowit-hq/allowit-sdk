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

Dependencies point one way. This SDK depends on Allowit core (package
`allowit-sdk`, repository allowit-core) and on the rail crates from
allowit-contracts-solana, allowit-contracts-tempo and allowit-contracts-stellar.
The rail crates depend on Allowit core. Never move core or rail code into this
repository, and never add a `[patch]` section. Each git dependency names a
release tag `vX.Y.Z` in the root `[workspace.dependencies]` (the policy template
repeats the core tag). Every crate in the build must name the same core tag:
release the rail repositories on a new core tag before this SDK moves to it.
