# Allowit CLI

The `allowit` command selects a Stellar network, keeps local keys, inspects public wallet profiles and sends agent requests.
[`allowit-sdk-engine`](../engine/) supplies reusable workflows for CLI and server callers.
Rail core owns native encoding, signing, simulation and settlement semantics.
[Allowit core](https://github.com/allowit-hq/allowit-core) supplies deterministic policy logic and host transport and journal primitives.
The server supplies trusted policy enforcement and its own signing authority.

```sh
cargo run --locked --bin allowit -- --help
cargo build --locked --release --bin allowit
```

## Commands

```
allowit rail ls | show [RAIL] | use RAIL
allowit keys generate NAME [--fund]
allowit keys add NAME --public-key GADDR
allowit keys import NAME FILE|-
allowit keys use NAME
allowit keys ls | show NAME | fund NAME | export NAME | rm NAME
allowit config show | path
allowit wallet ls | show | use ADDR
allowit policy show [--source]
allowit policy check POLICY.rs
allowit policy test POLICY.rs --context CONTEXT.json
allowit policy functions [NAME]
allowit --rail stellar:testnet policy build POLICY.rs [-o OUT]
allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-0001
allowit status pay-0001 --wait 60s
allowit history
allowit guide
```

`stellar:testnet` and `stellar:pubnet` select separate wallet and key settings.
`stellar:mainnet` is an alias for `stellar:pubnet`.
Private Stellar S seeds remain in `secrets/NAME` with mode 0600 inside a 0700 directory.
`keys use` selects an existing private key for that network. It performs no server registration.
Public keys added with `keys add` cannot sign. `keys export` explicitly prints a private seed.

## Stellar agent requests

The owner supplies the public wallet profile in `ALLOWIT_STELLAR_BUNDLE`.
It names the wallet, owner, agent, server, installed asset and engine endpoints.
It contains no signing material. `ALLOWIT_URL` selects the server origin.

```sh
allowit rail use stellar:testnet
allowit keys import agent agent.key
allowit keys use agent
allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-0001
allowit status pay-0001 --wait 60s
```

The selected agent key must match the profile before transport or signing.
`exec` builds and validates a typed native or SEP-41 transfer for the installed asset.
Policy wallets also accept SEP-56 vault deposits and redemptions with `--token-out` minimums.
`--build-only` prints the typed request and sends nothing.
Amounts use `CODE:DECIMAL`, `CODE:ISSUER:DECIMAL`, `C…:DECIMAL` or integer base units.
`XLM` and `native` identify the same asset.

The CLI journals each operation before sending it. An uncertain result retains its request ID and exact bytes.
Repeat the same `exec` or read `status`; a new request ID represents a new operation.
Terminal status requires a verified receipt. A rotated agent key can recover an original receipt without signing again.
Policy wallet signing revalidates the prepared operation through `--rpc-url URL` or the rail's `rpc_url` setting.
This RPC receives only public observations and unsigned simulation bytes.
Classic execution and signed recovery require no RPC override.

The hidden `wallet configure` and `wallet approve` commands produce browser signing links for Stellar Testnet.
`configure` binds the signer-setting operations of a funded classic account.
`approve` binds the complete policy source, revision and public wallet configuration.
Both retain their original intent in a working-directory journal and exit 10.

## Output and exits

Data goes to stdout. Diagnostics and generated request IDs go to stderr.
`--json` selects JSON; `NO_DNA` selects compact JSON and disables prompts.
`--build-only` always prints JSON. Destructive commands need `--yes` without a terminal.

| Exit | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Internal error |
| 2 | Invalid command or input |
| 3 | Configuration or unavailable capability |
| 4 | Contract rejection |
| 5 | Uncertain; preserve the request ID |
| 6 | Replayed result |
| 10 | Owner signature needed |
| 11 | Owner input needed |
| 12 | Pending |
| 20 | Policy denial |
| 130 | Interrupted |

## Source layout

- `src/commands/` owns command arguments, examples and terminal interaction.
- `src/vendors/stellar/` adapts the retained native wallet library. `asset.rs` and `address.rs` validate Stellar input.
- `src/settings/` owns caller configuration, private keys and local request records.
- `src/ctx.rs` resolves caller settings. `src/term.rs` renders output.
- [`templates/policy/`](templates/policy/) contains the policy crate; [`skills/allowit/`](skills/allowit/SKILL.md) describes authoring.

Help snapshots live in `tests/snapshots/help/`.
After an intended command change, run `ALLOWIT_UPDATE_SNAPSHOTS=1 cargo test -p allowit-cli --test help`.

## Owner operations

`allowit_cli::admin::Admin<V>` delegates each operation to its bound vendor:
`deploy_policy(deployment_version, artifact)`, `set_pause(paused, deployment_version)` and
`recover_funds(token)`. The shared SDK defines separate capability traits without fallback implementations.
Vendor packages own native arguments, trusted state reads, authorization and transaction preparation.
Other configuration edits belong to the complete policy artifact and the same deployment flow.

The Stellar `stellar::admin::OwnerAdmin` prepares unsigned owner-review transactions for the versioned
policy wallet interface. Deployment and pause target exactly the active deployment version plus one. Recovery reads
the full selected token balance and current owner. It has no caller-supplied recipient, amount or nonce.
This profile recovers only the installed asset through the existing owner recovery entrypoint.
It does not implement arbitrary-token recovery or direct owner token authorization.

Preparation checks the network, pinned executable, current owner, policy binding, native authorization
scope and fee ceiling. It returns transaction XDR and required authorizers for native-wallet review and
signing. It does not submit transactions or produce a browser signing URL. This adapter requires a native
account as the current owner. Contract owners require a matching native authorization adapter.

`deployment_version` is the ordered owner-configuration counter. Deployment and pause use its next
value. `contract_version` identifies the policy contract code release. `engine_version` identifies the
engine code release. The first explicit contract and engine release values are each 1. Release numbers
are metadata; the exact executable hash identifies code.

The Stellar adapter supports contract release 1 and requires matching release metadata in the current
stored state and executing policy binding. New preparation uses `deployment_version`. Existing signed
`revision` packets from canonical SDK main retain a private decoder for status and recovery only.
They cannot authorize a new preparation or signature. Exact signed transaction bytes remain unchanged.

`PolicyDeployment.x402_enabled` is explicit owner opt-in. The contract derives the Ed25519 key
from the configured native G-agent address. The vendor validates that address and encodes the Boolean.
Contract agents cannot enable this native-key delegation.
This delegates the raw key; native account signer or threshold changes do not revoke it.
The owner disables x402 to revoke this delegated route.

`stellar::rpc::engine_version` is a separate read-only metadata query. It returns `None` only when host
diagnostics identify the missing getter on the selected engine. Network errors, malformed metadata
and other contract failures reject. Owner operations do not query the engine.

## Policy configuration and spent values

`stellar::admin::Grant` is a named map of native `ScVal` values. `NativeValues` preserves arbitrary
supported native values, including keys that no selected check reads. Its JSON form stores each value
as canonical base64 XDR. An installed asset is a separate immutable custody binding. Grant keys do
not activate checks automatically. `PolicyDeployment` has no pause field; deployment preserves the
current pause state, and `set_pause` changes it.

A native policy receives read-only Grant configuration and a working State containing Grant and spent.
It returns the exact final spent map to commit:

```rust
use allowit::v1::prelude::*;
async fn _execute(request: &Context, grant: &Grant, state: &State) -> Result<Spent, PolicyError> {
    let daily = allowit::daily_cap(request, state)?;
    let after_daily = allowit::with_spent(state, daily)?;
    let total = self::total_cap(request, &after_daily)?;
    let n = allowit::state_get(&total, "custom_count", 0u64)?;
    let final_spent = allowit::state_set(total, "custom_count", n + 1)?;
    Ok(final_spent)
}
```

`with_spent` replaces only spent and preserves Grant. `state_get` reads an expected scalar type;
`state_set` and `state_remove` return a new spent map. A property-only policy starts with
`allowit::spent(state)`. Custody commits only the map returned by the policy after successful effects;
returning an earlier map selects that earlier value. Denial, errors and failed token effects roll back
all changes. Policy maps cannot overwrite owner, asset, deployment version, execution nonce or replay
records. Rule accounting keys remain policy data, so owner code can intentionally change them.

The compiler recommends SimplePolicyEngine, CommonPolicyEngine or CustomPolicy from the whole source. Common uses an exact ordered subset of canonical checks. Custom uses local `allowit::` or `self::` mixins and preserves bespoke control flow. Shared-engine calls, cost selection and hybrid placement are rejected. The owner reviews the selected implementation, rule list, source identity and exact module hash.

`stellar::rpc::prepare` reads the current native Grant and full spent map, then simulates the exact
installed policy before new signing. It rejects archived, malformed and restoration-required
observations. A missing spent entry needs a native getter's successful None proof. Arbitrary request
JSON supplies neither these observations nor engine decisions. Classic/native wallets retain their
existing trusted accounting and local guard evaluation; a selected shared-engine call cannot run on
that route.

The server signs its native authorization after initial preflight. A second enforce-mode simulation checks the signed scope and resources before the agent receives the packet. Successful policy wallet preflight creates an opaque in-memory binding to the exact prepared operation.
`sign` rechecks that binding. Loading an unsigned intent from JSON requires fresh preflight under the
same logical operation ID. Existing signed bytes retain verification, status lookup and retry without
new signing approval. Preflight is an observation; execution applies the current native policy and
engine behavior. Common and Custom use native spent values. Simple requires durable service accounting and exact source evaluation before the server signs.

For new policy wallet signing, the command-line client reads `--rpc-url` or the rail's `rpc_url` setting.
It uses the Stellar vendor's `rpc::revalidate_prepared` against that RPC before signing the existing
gateway intent. The URL comes from local configuration, not the gateway bundle. The RPC receives
only public native observations and unsigned simulation bytes; it receives no token, seed or gateway
authorization header. HTTPS is required except for loopback HTTP. Missing configuration, stale state
or insufficient existing resources stops signing. Revalidation preserves the exact unsigned
transaction, hash and logical request ID. Classic execution and signed status/recovery do not require
this setting. If resource bounds cannot cover the current simulation, the client retains the original
intent and reports the failure; it does not replace the transaction or logical request ID.
