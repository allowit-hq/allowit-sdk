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
path and are never pinned. Select the SDK with `-p allowit-sdk@0.5.0`. The `allowit-core` dependency alias selects
Core's released `allowit-sdk` package.

Dependencies point one way. This SDK depends on Allowit core (repository
allowit-core, package `allowit-core`) and on each registered rail package. The rail
crates depend on Allowit core. Never move core or rail code into this repository, and
never commit a `[patch]` section. Each git dependency names a release tag `vX.Y.Z` or an
exact reviewed commit in the root `[workspace.dependencies]`. Every crate in the build
must name the same core revision, so the plugin trait objects match.

## Vendor boundary

Only five files may name a vendor, and only on import or dependency lines:
`Cargo.toml`, `crates/allowit-sdk/Cargo.toml`, `crates/allowit-sdk/src/plugins/registered.rs`,
`npm/package.json` and `npm/src/registered.js`. Everything else reaches vendors through
`plugins` (Rust) or `@allowit/sdk` (JavaScript). Maintainers check this boundary in the
allowit-project review before merge.

A policy package registers with one dependency line per manifest and one import line in
`registered.rs`, `use <crate>::PACKAGE as PACKAGE_<n>;`, plus its alias in `PACKAGES`. Its namespace
is the crate name. `plugins::policy_registry` composes rail evidence functions and package
functions, reserves `allowit` and every rail id, and checks the complete list against the
policy-source gate. Hosts call that function and `plugins::invoke_policy_function`.

SDK tests are vendor neutral. They run on every registered rail through
`plugins::testkit::each` (Rust) or `for (const rail of rails())` (JavaScript) and take values
from each rail's fixtures. A rail declares the features it skips, with a reason. Package tests run
on every registered policy package through `plugins::testkit::each_package` with the package's
samples; a package declares the features it skips, with a reason. Vendor
protocol, codec and RPC tests live in the vendor repository.

## Branches

`main` builds from public repositories only. No manifest or lockfile on `main` names a private
repository; the `Public sources` CI job (`tools/public-sources.sh`) reads each Git source without a
credential and fails on one that is not public.

`staging` is `main` plus the registration commits of private policy packages: the dependency lines
in `Cargo.toml` and `crates/allowit-sdk/Cargo.toml`, the import line and its `PACKAGES` alias in
`registered.rs`, and the matching `Cargo.lock` entries. Staging deployments build from it. Never
merge `staging` into `main`, never open a pull request from it and never force-push it. Refresh it
after `main` changes:

```sh
git fetch origin && git switch staging && git merge --ff-only origin/staging
git merge origin/main                      # keep both sides of the registration lines
git checkout origin/main -- Cargo.lock     # only when Cargo.lock conflicts
cargo metadata --format-version 1 >/dev/null   # adds only the lock entries of the registered packages
git add Cargo.lock && git commit           # only when the merge stopped or the lockfile changed
cargo fmt --all --check
cargo test --locked -p allowit-sdk@0.5.0 --features testkit
cargo clippy --locked -p allowit-sdk@0.5.0 --all-targets --features testkit -- -D warnings
git push origin HEAD:refs/heads/staging
```

Every crate in the `staging` build names the Core revision of `main`: move a private package to
that revision in its own repository first, then pin its new commit on `staging`. CI on `staging`
reads private sources through `.github/actions/private-dependency` and runs the SDK tests, which
loop over the registered packages with `plugins::testkit::each_package`.
