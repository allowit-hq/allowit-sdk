# allowit-sdk

## Artifact storage

Keep code, required build/test inputs and concise API/usage documentation in this repository. Keep plans in the canonical `allowit-hq/allowit-project` knowledge repository under `.agents/4-plans/`. Keep design reports, reviews, run evidence, demo recordings and screenshots in its `.agents/artifacts/`. Never commit those artifacts in this subproject or its feature branches. Link to the retained artifact in that project.

## Before pushing

Read the affected workflows in `.github/workflows/`. Run their affected test and
validation commands locally, including their feature flags and selectors. Include
workflow and test-input changes when selecting checks. Resolve failures before
pushing. Preserve existing reviews and deployment restrictions.

All packages share one Cargo workspace, one `Cargo.lock` and one version
(`[workspace.package]`). Packages in this repository depend on each other by
path and are never pinned. Select the shared engine with `-p allowit-sdk-engine`.

Dependencies point one way. This SDK depends on Allowit core (repository
allowit-core, packages `allowit-sdk` and portable `allowit`) and on the Stellar
rail package from allowit-contracts-stellar.
The rail crates depend on Allowit core. Never move core or rail code into this
repository, and never add a `[patch]` section. Each git dependency names a
release tag `vX.Y.Z` in the root `[workspace.dependencies]`. Every crate in the build must name the same core tag:
release the rail repositories on a new core tag before this SDK moves to it.
