# Allowit SDK engine

`allowit-sdk-engine` is the shared offchain engine for the CLI, server engine and SDK callers.
It composes reusable rail communication and execution workflows without terminal I/O.
Callers supply arguments, configuration and the required RPC, signing, storage and authorization capabilities.

[Allowit core](https://github.com/allowit-hq/allowit-core) provides deterministic policy logic and
feature-gated host primitives. Rail cores provide native protocol and settlement semantics.

## Responsibilities

| Component | Responsibility |
| --- | --- |
| SDK engine | Shared preparation, execution, status and recovery workflows for CLI and server callers |
| Allowit core | Shared types, validation, deterministic policy logic and host-only transport and journal primitives |
| Rail core | Native request encoding, addresses, signatures, simulation and settlement validation |
| Server engine | Authentication, trusted policy context, server authority, enforcement and durable application state |
| CLI | Commands, input/output, local key custody and actor-specific configuration |
| Vendor adapter | Provider request/response protocols and typed facts, using common transport and workflow capabilities |

Placement follows responsibility. A reusable rail workflow belongs in SDK engine even when its first caller is the server.
Rail packages can use common core primitives directly. Core has no dependency on SDK engine or a rail package.

## Modules

| Module | Purpose |
| --- | --- |
| `client` | Allowit backend requests with bounded responses and single-send semantics for payment-bearing requests |
| `gateway` | Gateway requests and shared request options |
| `request`, `typed_request` | Request fields, bounded input documents and operation identities |
| `config`, `routing`, `wallet` | Configuration, network selection and calling-actor wallet bindings |
| `output`, `skill` | Result classification, printable values and public operation descriptions |
| `services` | Shared service contracts and selected rail/vendor capabilities, enabled by the `services` feature |
| `error` | The error type from `allowit_sdk::client::error` |

## Communication and authority

Common host transport owns HTTP and JSON-RPC bounds, deadlines and connection behavior.
SDK engine composes those primitives with rail capabilities and caller-supplied state.
Rail semantics determine whether an exact operation can be queried, resumed or resubmitted.
An uncertain mutation retains its original identity and signed bytes.
Provider delivery and chain settlement remain separate results.

The server supplies policy admission, spending limits, trusted context and server-authority bindings.
Its durable journal binds approved requests and settlement evidence to application accounting.
The CLI supplies local signing custody and private journal storage.
SDK engine shares workflow logic between those callers without transferring their keys or authority.

Vendor adapters translate provider protocols. Generic transport, retry budgets and journal mechanics belong in common code.
An adapter exposes an explicit provider recovery contract instead of duplicating the provider's delivery workflow.
The shared core's `Upstream::cached` and `Upstream::resume` contracts preserve that boundary.

## Develop

Run these commands from the repository root:

```sh
cargo test --locked -p allowit-sdk-engine
cargo clippy --locked -p allowit-sdk-engine --all-targets -- -D warnings
cargo clippy --locked -p allowit-sdk-engine --features services --all-targets -- -D warnings
```
