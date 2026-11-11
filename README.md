# Allowit SDK

**Go on. On your terms.**

This repository contains `allowit-sdk`, the shared offchain execution library.
Both CLI and server callers use SDK engine. Each actor retains its own keys, authorization and durable application state.
The [Allowit CLI](https://github.com/allowit-hq/allowit-cli) owns terminal commands and local configuration.
Server enforcement, native authority and verified settlement remain authoritative.

[Allowit core](https://github.com/allowit-hq/allowit-core) contains the portable `allowit` policy library, its shared source gate and feature-gated host primitives.
Rail libraries own native requests, encoding, signatures, RPC methods, simulation and settlement rules.
The [workspace manifest](Cargo.toml), [lockfile](Cargo.lock) and [plugin registration](crates/allowit-sdk/src/plugins/registered.rs) define this build.
SDK engine composes those capabilities into preparation, execution, status and recovery workflows.
The server adds trusted policy context, admission, server authority, reservations and accounting.

## Architecture

SDK engine owns reusable execution logic for CLI and server callers.

```mermaid
flowchart TB
    CLI["[allowit-cli]<br/>crate @allowit/cli<br/>DX: policy and agent keypair generation<br/>Policy validation · x402 marketplace"]
    Server["[allowit-engine]<br/>crate @allowit/server<br/>Shared REST API · server enforcement"]
    Engine["[allowit-sdk]<br/>crate allowit-sdk<br/>All other shared logic"]
    Vendor["crate + npm @allowit/vendor/*<br/>Native contracts · signing · chain semantics<br/>SVG graphics · web components"]
    Core["[allowit-core]<br/>crate + npm @allowit/core<br/>Common types · policy rules · primitives<br/>TypeScript interfaces"]

    CLI -->|crate| Engine
    Server -->|crate| Engine
    Engine -->|crate| Vendor
    Engine -->|crate| Core
```

## Wallet kinds, units and the owner workflow

Hosts offer the same four wallet kinds and show the same text. Core describes them; each rail advertises the kinds
it supports. The selection id `standard` names the tier that technical names call Common, and its supported params
are listed in the [Core README](https://github.com/allowit-hq/allowit-core/blob/main/README.md#standard-wallet-parameters).

| Need | Rust (`allowit_sdk`) | npm (`@allowit/sdk`) |
| --- | --- | --- |
| All kinds | `kinds::WALLET_KINDS`, `kinds::WalletKind` | `walletKinds()`, `walletKind(id)` |
| Kinds of a rail | `kinds::wallet_kinds(network)` | `walletKindsFor(network)` |
| Kind that a wallet binds | `kinds::wallet_kind(network, profile)`, `Profile::wallet_kind()`, `ConfigurationInfo::wallet_kind()` | `walletKindOf(network, document)` |
| Unit of an asset | `kinds::native_asset(network)`, `kinds::asset_display(network, asset)` | `nativeAsset(network)`, `assetDisplay(network, asset)` |
| Print an amount | `kinds::format_amount(network, asset, units)`, `AssetDisplay::format(units)` | `formatAmount(network, asset, units)`, `formatUnits(units, decimals)` |
| Settled operation | `execution::Settlement::of(&outcome.document)` | the `settlement` object of the outcome document |
| Review and sign | | `parseReviewLink`, `reviewAgentPrompt`, `reviewParamLines`, `installAndFund(network, input)` |

A host never prints a unit of its own. An asset without a known unit is an error in Rust and `null` in npm.
`npm/src/wallet-kinds.js` is generated from Core by the test `npm_wallet_kinds_equal_the_core_description`;
run it with `ALLOWIT_WRITE_WALLET_KINDS=1` after a Core change.
`progress::set_sink` installs the sink for the progress lines of long provider steps.

## Write a portable policy

Policies are synchronous Rust against the `allowit` package:

```rust
use allowit::prelude::*;

pub fn execute(ctx: &AllowitContext) -> allowit::Result {
    allowit::operations(ctx, &ctx.params[allowit::OPERATIONS])?;
    allowit::daily_spending_limit(ctx, &ctx.params[allowit::DAILY_SPENDING_LIMIT])?;
    allowit::payment_limit(ctx, &ctx.params[allowit::PAYMENT_LIMIT])?;
    Ok(())
}
```

The host and the rail's policy contract use the same source. Owner-approved params supply the exact limits in base units.
Request amounts, native identity, accounting and enrichment come from the executing host and rail.
Policy source cannot provide its own trusted spending state.

The shared host build gate runs Core's Rust source gate, then compiles the source with `rustc` against the exact portable library before installation authorization.
See the [portable policy reference](docs/policy-reference.md) for source restrictions and wallet tiers.

## Embed SDK engine

[`crates/allowit-sdk/`](crates/allowit-sdk/) contains shared workflows without terminal I/O:

- `runtime::Runtime` composes native preparation, required durable retention, submission and recovery.
- `plugins` holds the rail plugin registry. `plugins::for_network(network)` returns the registered `RailPlugin` (trait in `allowit_core::vendor::plugin`) for a CAIP-2 id or alias. Engine, CLI and app code reach vendor behavior only through it.
- `generation`, `owner` and `execution` dispatch owner sign-in, policy generation, owner workflows, execution, status and recovery to the selected plugin. Callers pass their signer capability and a private core `Store`.
- `typed_request` bounds document ingress and derives the versioned operation identity.
- `wallet`, `routing` and `config` supply explicit caller bindings and registered network selection.

## Rail plugins

A vendor implements `allowit_core::vendor::plugin::RailPlugin` in its own repository. It integrates with one entry line per language and one dependency line per manifest:

- Rust: `crates/allowit-sdk/src/plugins/registered.rs`, plus the dependency in `Cargo.toml` and `crates/allowit-sdk/Cargo.toml`.
- JavaScript: `npm/src/registered.js`, plus the dependency in `npm/package.json`.

These five files are the only SDK files that may name a vendor. No other code branches on a vendor. Dispatch is by the CAIP-2 network id that each plugin declares.

The npm package `@allowit/sdk` ([`npm/`](npm/)) is the browser registry. It exposes `rails()`, `railFor(network)`, `networks()`, `validateAddress`, `explorerUrl`, `assets` and one generic element, `<allowit-rail kind network>`. A host sets `element.data` and listens for `allowit-signed`, `allowit-signed-in`, `allowit-verified`, `allowit-cancel` and `allowit-error`. The element renders the registered vendor element for that kind (`address`, `explorer-link`, `amount`, `wallet-sign-in`, `owner-sign`, `receipt` or `binding-review`). [`npm/src/registry.d.ts`](npm/src/registry.d.ts) defines the data and events.

Shared tests run on every registered rail. `plugins::testkit::each(feature, f)` (feature `testkit`) runs `f` with each rail's fixtures and skips a rail that declares the feature unsupported. Vendor-specific tests live in the vendor repository.

Authorization is retained before post-authorization RPC simulation. Signed proof is retained before transmission.
A failed policy-wallet envelope does not prove that its reusable address authorization cannot execute in another envelope.
Unknown outcomes preserve their identity and reservation.

Core HTTP and JSON-RPC primitives supply bounds, deadlines and single-attempt transport.
Rail semantics determine whether exact retained bytes remain valid. SDK engine applies those rules to caller-owned journals.
Provider delivery is separate from native chain settlement.

## Repository map

| Path | Purpose |
| --- | --- |
| [`crates/allowit-sdk/`](crates/allowit-sdk/) | Shared execution library for CLI, server and SDK callers |
| [`npm/`](npm/) | `@allowit/sdk`: the browser rail registry and the `<allowit-rail>` element |
| [`docs/policy-reference.md`](docs/policy-reference.md) | Portable source and execution contract |

The Cargo workspace has one package, `crates/allowit-sdk`, and one lockfile.
Cross-repository dependencies identify immutable release tags or exact source commits. A commit pin does not identify a published release. Core and rail dependencies must resolve the same core revision, so the plugin trait objects match.
SDK exposes the crate as `allowit_sdk`. Core imports use `allowit_core`; `allowit_sdk::portable` reexports it.

## Develop

```sh
cargo build --workspace --all-targets --locked
cargo fmt --all --check
cargo test --locked -p allowit-sdk@0.5.0 --features testkit
cargo clippy --locked -p allowit-sdk@0.5.0 --all-targets --features testkit -- -D warnings
(cd npm && pnpm install && pnpm test)
```

The affected [workflows](.github/workflows/) define the required checks.

## License

SDK-authored code is under the [MIT License](LICENSE). Dependencies keep their own terms, including the portable `allowit` package.
