# Allowit CLI

*The `allowit` command: rails, local keys, policy wallets, agent requests and the policy compiler tools.*

This crate (`allowit-cli`, library `allowit_cli`) builds the `allowit` binary. It acts for the agent and the owner. It selects the rail, signs with keys that stay on the machine, validates receipts and recovers uncertain submissions. Backend requests come from [`allowit-sdk-engine`](../engine/), which has no terminal I/O. Errors, configuration and private storage are the `client` module of [`allowit-sdk`](../core/), which also holds the compiler and evaluator. Rail crates live in their rail repositories and depend on the same core.

```sh
cargo run --locked --bin allowit -- --help
cargo build --locked --release --bin allowit
```

Release builds use the workspace release profile (`lto`, `opt-level = "s"`, `panic = "abort"`, `strip`).

## Commands

```
allowit setup --agent NAME [--force]
allowit whoami

allowit keys generate NAME [--fund]          ed25519 on Stellar and Solana, secp256k1 on Tempo
allowit keys add NAME --public-key ADDR
allowit keys import NAME FILE|-              a private key, or a Solana or Tempo executor bundle
allowit keys ls | show NAME | fund NAME | export NAME | rm NAME

allowit rail ls | show [RAIL] | use RAIL
allowit config show [--reveal] | path

allowit wallet create --kind simple|common|custom --asset ASSET --policy FILE --owner KEY|ADDR --agent KEY [--x402]
allowit wallet ls | show | use ADDR
allowit wallet fund AMOUNT [--from KEY]
allowit wallet withdraw AMOUNT [--to ADDR]
allowit wallet update [--owner KEY|ADDR] [--agent KEY] [--x402 on|off]
allowit wallet pause | resume | close

allowit policy new "PROMPT" [-o FILE]
allowit policy show [--source]
allowit policy check FILE
allowit policy build FILE [-o OUT]
allowit policy test FILE --context CTX.json [--mode oracle|contract]
allowit policy deploy FILE [--x402 on|off]
allowit policy params [show]
allowit policy params set KEY=VALUE... [--request-id ID]
allowit policy functions [NAME]

allowit exec --op OP --to ADDR --amount AMOUNT [--token-out ASSET:MIN] [--args JSON]
             [--request-id ID] [--wait DUR] [--build-only]
allowit eval  (the exec flags; never signs)
allowit status ID [--wait DUR]
allowit history [--limit N]
allowit tx send FILE|-

allowit skills search QUERY | show ID | ls
allowit call SERVICE|URL [--input JSON] [--max-amount AMOUNT]
allowit curl ARGS... | http ARGS...
allowit mcp | claude [-- ARGS] | codex [-- ARGS]

allowit lsp | completion --shell SHELL | guide | version | help [CMD]
```

Every node has `--help`. A node that works on some rail lists examples first; each example runs on the rail it names, or on a rail that `rail show` reports for it. A bare group prints its help and changes nothing. An unknown command exits 2 and lists the commands at that level. `allowit guide` prints the agent guide.

Owner verbs (`wallet create`, `fund`, `withdraw`, `update`, `pause`, `resume`, `close`, `policy deploy` and `policy params set`) share three signing flags: `--sign-with-link` (the default) prints the owner signing link as the `signingUrl` field on stdout, `--sign-with-key NAME` signs with a local key, and `--build-only` prints the unsigned transaction. `rail show` lists the modes that each owner verb accepts on the rail; another mode exits 3. `--wait DUR` waits for the owner; no rail serves it yet, so it exits 3. `wallet fund` and `wallet withdraw` also take `--request-id ID`, which a retry reuses, and `--additional`, which sends a deliberate second operation after an uncertain one. `policy params set` also takes `--request-id ID`; without it, each command is a new change and prints its new ID on stderr.

Earlier command names stay as hidden aliases: `compile`, `check` and `workflow` (`policy check`), `evaluate` (`policy test`), `stellar-native` (`policy build` on Stellar), `registry` (`policy functions`), the `tempo` group, the Solana `policy generate|import|fund|execute|status|revoke|withdraw|close|tune|tune-action` verbs, and the Stellar `wallet configure` and `wallet approve` links. The `tempo` group and the Solana `policy` verbs select a rail only with `--rail` or `ALLOWIT_RAIL`; an unknown rail exits 2. `wget` and `fetch` are aliases of `curl` and `http`. `json` is the hidden editor wire protocol.

## Rails

A rail has one canonical CAIP-2 identifier. Input also accepts the aliases below. Output always prints the identifier.

| Identifier | Aliases |
| --- | --- |
| `stellar:testnet` | `stellar:testnet` |
| `stellar:pubnet` | `stellar:mainnet`, `stellar:pubnet` |
| `solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1` | `solana:devnet` |
| `solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z` | `solana:testnet` |
| `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp` | `solana:mainnet` |
| `eip155:42431` | `tempo:testnet` |
| `eip155:4217` | `tempo:mainnet` |

There is no default rail. A command that needs one exits 3 with the hint `allowit rail use RAIL`. Each rail keeps its own default wallet, agent key and RPC override, so a Stellar C… wallet never becomes active on Solana. Addresses must match the rail: G… accounts and C… contracts on Stellar, base58 on Solana, 0x… on Tempo. A Stellar wallet selection is a C… contract; the classic route of a wallet profile needs no selection.

`allowit rail show RAIL` lists the key curve, the token table, the `exec --op` values, the `policy params set` keys, the signing modes and the verbs that work on that rail. A verb that has no backend on a rail exits 3 with `VERB is not available on RAIL yet`.

| Verb | Stellar testnet | Solana devnet and testnet | Tempo testnet |
| --- | --- | --- | --- |
| `keys fund` | Friendbot | RPC airdrop | not available |
| `keys import` of a bundle | not available | executor bundle | executor bundle |
| `wallet create` | not available | link signing with the first deposit | not available |
| `wallet show`, `policy show` | public wallet profile | native vault, engine harness | executor bundle |
| `wallet fund`, `withdraw`, `pause`, `close` | not available | `--sign-with-key` | not available |
| `wallet update`, `resume`, `tx send` | not available | not available | not available |
| `policy new` | not available | native generation | not available |
| `policy build` | custom policy wallet source | not available | not available |
| `policy params set` | not available | `day_spent_cap`, `transaction_cap` | not available |
| `policy deploy` | not available | not available | not available |
| `exec --op transfer` | engine endpoints, `--build-only` | engine harness | vault payment, `--build-only` |
| `eval` | not available | engine harness | not available |
| `status` | engine endpoints | engine harness | executor bundle |
| `call URL` | not available | not available | MPP |
| `call SERVICE`, `skills`, `curl`, `http`, `mcp`, `claude`, `codex` | not available | not available | not available |

`exec --build-only` prints the unsigned request (Stellar) or the authority-signed wallet plan (Tempo) as JSON and exits 0. The CLI never signs or submits it, and does not record it in the request journal. Stellar builds the request locally and sends nothing. Tempo asks the engine for the plan, and the engine records the request under its ID; a denial, an owner-input request or a settled request keeps its exit code. A signer and submitter outside the CLI use the output; `tx send` is not available on any rail. On Solana, `exec --build-only` exits 3 before a request ID is spent or a credential is read.

A selected wallet (`--wallet`, `ALLOWIT_WALLET` or `wallet use`) must be the wallet that the Stellar profile, the Tempo bundle or the Solana policy context in `ALLOWIT_POLICY_DIR` names; another wallet exits 3. The Solana engine harness names a policy but no vault, so Solana `exec`, `eval`, `status` and `policy show` exit 3 while a wallet is selected. The request journal binds each request ID to its rail, the wallet and policy that the rail resolved, and the request itself; a retry under another wallet or policy exits 2. `status ID` uses the rail and wallet in the journal and reads only under the recorded policy. It refuses a different `--rail` or `--wallet` flag; `ALLOWIT_RAIL`, `ALLOWIT_WALLET` and the `use` defaults yield to the journal.

Mainnet rails serve `exec`, `status`, `wallet show`, `policy show` and `policy build` on Stellar, and `exec`, `eval`, `status` and `policy show` on Solana. Tempo mainnet serves no verb yet. `swap`, `deposit`, `redeem` and `custom` operations need new contract entrypoints on every rail; they exit 3. The paid-service commands keep their request builders in `src/gateway/`, but the engine serves no paid-service route, so they exit 3 on every rail.

## Amounts

| Form | Meaning |
| --- | --- |
| `CODE:DECIMAL` | Display units of a token from the rail's table, for example `USDC:10`. `XLM` and `native` name the same asset. |
| `CODE:ISSUER:DECIMAL` | ISSUER is a Stellar G… issuer, a Solana mint or a Tempo 0x… token. |
| `C…:DECIMAL` | A Stellar Asset Contract. |
| `INTEGER` | Base units of the wallet's installed asset. |
| `CODE:max` | The whole balance, for `wallet withdraw` only. |

`--token-out` takes `ASSET:MIN`, where MIN may be zero. Each rail supplies its decimals: 7 on Stellar, 9 for SOL and 6 for USDC on Solana, 6 on Tempo. An amount with more decimal places than its token exits 2. An amount of a token other than the wallet's own exits 4. `call --max-amount` also takes a plain decimal of the paying asset, for example `0.05`. `curl`, `http`, `claude` and `codex` take the same `--max-amount`; `--max-payment` is its earlier name.

Policy parameter keys are the canonical keys of the contracts params map: `transaction_cap`, `spent_cap`, `day_spent_cap` and `day_total_cap` take amounts, `call_cap` a count, `recipients` addresses, `tokens` assets, `expiry` an RFC 3339 time or a duration such as `30d`, and `operations` operation names. A list value replaces the earlier list.

## Flags, configuration and output

Global flags work in any position: `--rail RAIL` (aliases `-n`, `--network`; env `ALLOWIT_RAIL`), `--wallet ADDR` (env `ALLOWIT_WALLET`), `--output text|json` (alias `--json`), `-y/--yes`, `-q`, `-v`, `--no-color`, `--config-dir DIR` (env `ALLOWIT_CONFIG_DIR`) and `--rpc-url URL`. A flag wins over the environment, and the environment wins over the configuration that `rail use` and `wallet use` write. Local commands such as `policy check`, `policy test`, `policy functions`, `lsp`, `completion`, `guide` and `version` read no rail, wallet or credential.

The configuration directory is `$XDG_CONFIG_HOME/allowit` (default `~/.config/allowit`):

- `config.toml` holds the current rail and, per rail, the default wallet, known wallets, the agent key, an RPC override (`rpc_url`) and the paid-service asset that `setup` chose (USDC; AlphaUSD on Tempo testnet, which has no USDC).
- `keys/NAME.toml` holds public key metadata: rail, address, public key and curve.
- `secrets/NAME` holds a private key in the rail's signer format: a Stellar S… seed, a Solana key-pair JSON array or a Tempo 32-byte hex key. The file is mode 0600 in a 0700 directory. No flag accepts a secret; `keys export` prints one.
- `requests/` is the request journal. `exec` and `eval` record each request before it leaves the process, so `status ID` finds its rail and wallet and `history` lists it. A request ID never names two different requests. When the journal cannot record a result, for example because another process holds its lock, the command keeps the request's exit code and prints a warning. Records live in 256 folders named by the first byte of the request ID's SHA-256; each folder holds at most 1,000 records.
- `stellar-journal/` holds the Stellar operation journal for recovery.

Data goes to stdout, including an owner signing link, which is the `signingUrl` field. Progress, hints, prompts and new request IDs go to stderr; progress and new request IDs leave at once, so an interrupted command keeps its request ID. `-q` drops progress but keeps a generated request ID. Text is the default, also when stdout is a pipe: a result prints one `key value` line per field, with dotted paths such as `config.rail` for nested fields. `--json` selects JSON; with it, policy source prints as one `{"source": …}` object. The output of `--build-only` and the hidden aliases of earlier releases always print JSON. Policy source and JSON documents are syntax-highlighted only when stdout is a terminal, and never with `--json`, `NO_COLOR`, `TERM=dumb`, `--no-color` or a pipe. `NO_DNA` selects compact JSON and no prompts. In JSON mode an error is one object on stderr: `{"error":{"code","exit","message","request_id"}}`. A warning leaves the result and exit code unchanged; it is a `warning:` line on stderr, or `{"warning":{"message"}}` in JSON mode, and `-q` keeps it. A destructive verb asks for confirmation on a terminal and needs `--yes` otherwise.

| Exit | Meaning |
| --- | --- |
| 0 | ok |
| 1 | internal error, including a panic |
| 2 | usage error or invalid input, including an argument that is not UTF-8 |
| 3 | configuration, authentication or not available on this rail |
| 4 | rejected by the contract (paused, not approved, wrong asset) |
| 5 | uncertain or stale: retry with the same request ID |
| 6 | replayed |
| 10 | owner signature needed |
| 11 | waiting for owner input |
| 12 | pending |
| 20 | denied by the policy |
| 130 | interrupted |

## Stellar agent requests

The owner exports the public wallet profile from the browser. Set `ALLOWIT_STELLAR_BUNDLE` to that file until the CLI resolves the wallet from chain state. The profile names the wallet, its owner, agent and server roles, the installed asset and the engine endpoints. It never holds signing material; a profile with a signing seed fails before transport.

```sh
allowit rail use stellar:testnet
allowit keys import agent agent.key
allowit setup --agent agent
allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-0001
allowit status pay-0001 --wait 60s
```

`exec` builds the typed request from its flags: a native transfer for an XLM wallet, or a SEP-41 `transfer` of the installed asset. The wallet library decodes the request before it leaves the process. The agent key from `setup --agent` signs each engine request and the prepared operation. A transfer of another asset exits 4. `--build-only` prints the typed request and sends nothing; a signer outside the CLI submits it, because `tx send` is not available on any rail yet.

The CLI journals each operation before it sends it. After exit 5, run the same `exec` again or read the result with `status`; the CLI never prepares replacement bytes for a request ID. A terminal status requires a verified chain receipt. A rotated agent key can recover an original receipt without signing again. Policy wallet signing revalidates the prepared operation through Stellar RPC before the agent signs: pass `--rpc-url URL` or set `rpc_url` for the rail in `config.toml`. The RPC receives only public observations and unsigned simulation bytes. Classic execution and signed status recovery need no RPC.

The hidden `wallet configure` and `wallet approve` commands keep the Stellar testnet owner links of earlier releases. `configure` prepares the signer-setting operations for a fresh funded classic account. `approve` binds the complete source, revision and public wallet binding of an installed classic policy and moves no funds. Both print a browser URL and exit 10. Their journals belong to the current working directory.

## Solana vaults

The Solana adapter runs the native vault lifecycle of `allowit-native` on devnet and testnet. `policy new` generates and saves a policy and prints its Rust source. The vault files live in `vaults/solana-NETWORK/` in the config directory; `ALLOWIT_POLICY_DIR` names another folder. The hidden verbs of earlier releases keep `.allowit/` in the working directory. `keys import` stores an owner-issued executor bundle as public context; a bundle of another network exits 3 and writes nothing. `wallet create` prints one browser link that binds the exact policy, public roles, network and initial deposit, and exits 10; a retry of the same creation is the same intent. After an unsigned preparation expires, `--request-id ID` with a new ID replaces it and keeps the original journal entry. The native vault takes its first deposit at creation, so Solana `wallet create` needs `--fund AMOUNT` and exits 3 without it. `--asset` and every named amount must resolve to the vault mint, which the imported policy context or `ALLOWIT_MINT` names; another token exits 4. Amounts in base units need no token name. `wallet create` signs with the link; the later owner verbs sign with `--sign-with-key`. After an unfinished `wallet fund` or `wallet withdraw`, the result's `retry` field names the flags of the next command: `--request-id NEW_ID --additional` for a deliberate second operation after an uncertain one, `--request-id NEW_ID` after an expired one, or the same `--request-id` while the audit is pending. After an expired `wallet create` link, it names `--request-id NEW_ID`. `wallet pause` and `wallet close` take no retry flags, so their results have no `retry` field. `policy params set` runs each key as its own operation with the request ID `ID.KEY`, which must fit 100 characters. A retry with the same ID replays the settled changes and runs the others; the command exits 6 only when every change was a replay. After an expired or unexecuted change, the `retry` field names `--request-id NEW_ID`. An operation that expired without executing, and a failed owner operation, exit 4; only a payment that the vault program refuses exits 20. `policy params set` with two keys runs two operations and prints one JSON array with `--json`, or two text blocks separated by a blank line. `exec`, `eval` and `status` use the engine harness through `ALLOWIT_TOKEN`; `status` still reads a request when the policy description is unavailable.

## Tempo vaults

The Tempo adapter runs `allowit-tempo` on Tempo testnet with the bundle in `ALLOWIT_TEMPO_BUNDLE`. The agent key from `setup --agent` signs `exec` and paid `call` payments; without one, `ALLOWIT_TEMPO_EXECUTOR_KEY_FILE` of earlier releases applies. `--rpc-url` or the rail's `rpc_url` replaces the bundle RPC. `exec --wait DUR` is the finality wait, at most 10m, and `status ID --wait DUR` polls until the request settles or fails. The command's request ID replaces an `ALLOWIT_REQUEST_ID` in the environment. `call URL` pays only an `https://` URL; another scheme exits 2. The adapter renders the crate's JSON results through the output module and does not print the crate's progress lines.

## Source layout

- `src/main.rs` is the binary. `src/lib.rs` parses the command line, runs one command and writes its output once.
- `src/commands/<group>/` holds one command group and one file per verb. Each verb file holds its clap arguments, its examples and its `run`. A command reaches a chain only through `vendors::rail`.
- `src/vendors/mod.rs` defines the `Rail` trait. `src/vendors/<rail>/` holds the Stellar, Solana and Tempo adapters. Every verb has a default that exits 3; an adapter overrides only the verbs whose backend works and lists them in its capabilities. `src/vendors/asset.rs` parses amounts and `src/vendors/address.rs` checks address formats.
- `src/ctx.rs` resolves flags, environment and configuration. `src/settings/` holds `config.toml`, the key store and the request journal. `src/term.rs` holds output and highlighting. `src/exit.rs` holds the exit codes.
- `src/gateway/` holds the paid-service request builders that `call`, `curl`, `http`, `skills` and `mcp` share. `Rail::gateway` gates them.
- [`templates/policy/`](templates/policy/) is the copy-paste policy crate, and [`skills/allowit/`](skills/allowit/SKILL.md) is the agent skill.

Help snapshots for every command live in `tests/snapshots/help/`. After an intended help change, run `ALLOWIT_UPDATE_SNAPSHOTS=1 cargo test -p allowit-cli --test help`.

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
