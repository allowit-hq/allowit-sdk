# Allowit SDK engine

`allowit-sdk` is the shared offchain engine for the CLI, server engine and SDK callers.
It composes reusable rail communication and execution workflows without terminal I/O.
Callers supply arguments, configuration and the required RPC, signing, storage and authorization capabilities.

[Allowit core](https://github.com/allowit-hq/allowit-core) provides deterministic policy logic and
feature-gated host primitives. Rail cores provide native protocol and settlement semantics.

## Architecture

SDK engine composes shared logic from core primitives and vendor capabilities.
CLI and server callers use the same library and retain their own keys and authority.
The [repository README](../../README.md#architecture) holds the architecture diagram.

## Responsibilities

| Component | Responsibility |
| --- | --- |
| SDK engine | Shared preparation, execution, status and recovery workflows for CLI and server callers |
| Allowit core | Portable policy types and validation, deterministic rules, host-only transport and journal primitives |
| Rail core | Native request encoding, addresses, signatures, simulation and settlement validation |
| Server engine | Authentication, trusted policy context, server authority, enforcement and durable application state |
| CLI | Developer workflows, policy and agent keypair generation, client validation and x402 marketplace interaction |
| Vendor adapter | Provider request/response protocols and typed facts, using common transport and workflow capabilities |

Placement follows responsibility. Generic orchestration belongs in SDK engine. Each rail's execution, owner workflows, runtime clients, request validation, profiles, formatting, diagnostics and signing belong in its vendor repository, behind the `RailPlugin` trait from `allowit_core::vendor::plugin`.
Rail packages can use common core primitives directly. Core has no dependency on SDK engine or a rail package.

## Modules

| Module | Purpose |
| --- | --- |
| `authoring` | Core's Rust source gate, then a bounded direct `rustc` compilation against the embedded portable library |
| `generation` | Registered owner-authenticated generation with exact selected-rail binding |
| `owner` | Common owner deployment, funding, activation, export and receipt workflows |
| `profiles` | Verified public exports, portable imports, immutable versions and explicit selection |
| `execution`, `input` | Shared request orchestration, amount parsing and selected native format checks |
| `runtime` | Common local execution, caller-owned OperationStore and original-packet recovery |
| `plugins` | Rail plugin registry: `rails`, `for_network`, `by_id`, `networks`; one registration line per vendor in `plugins/registered.rs` |
| `plugins::testkit` | Feature `testkit`: runs shared tests on every registered rail and applies declared skips |
| `request`, `typed_request` | Bounded request identifiers, input documents and operation identities |
| `config`, `routing`, `wallet` | Configuration, registered network selection and caller-owned opaque signers |
| `output` | Shared printable values |
| `transport_failure` | Class of a failed rail call: `network_unavailable` (code 7), unknown outcome (code 5) or other |
| `error` | The error type from `allowit_core::client::error` |

## Communication and authority

Common host transport owns HTTP and JSON-RPC bounds, deadlines and connection behavior.
SDK engine composes those primitives with rail capabilities and caller-supplied state.
Rail semantics determine whether an exact operation can be queried, resumed or resubmitted.
An uncertain mutation retains its original identity and signed bytes.
A call that got no answer is `network_unavailable`: a timeout, a connection failure, or a deadline that expires before the request is sent.
The caller can retry it with the same request ID.
Provider delivery and chain settlement remain separate results.

`Runtime::finalize_authorization` requires the caller's authorization and retention callbacks.
The complete authorization must be durable before the second native simulation starts.
Retention failure prevents that RPC step. The caller retains the returned final packet before exposing it to an agent.

`wallet::local_signer` selects the compiled key provider without exposing native private-key bytes.
Native key formats, cryptography and message domains live in the vendor library.
SDK engine retains secure files, configuration, sessions and workflow sequencing.
`generation::generate` requires a matching signer network before selecting its registered backend capability.

`RailPlugin::generate` takes a distinct caller-owned owner signer. The plugin checks the exact
sign-in message, account, Origin and nonce, uses Core session helpers to check freshness and requires a rotated owner session.
It checks advertised generation profiles, networks and terms before sending one creation request.
It checks returned source, params and native bindings as a draft. This draft grants no publication or spending authority.
An uncertain generation is not repeated automatically.

`RailPlugin::execution` takes a caller-owned signer capability and a private core `Store` opener.
It saves the original intent before preparation and the exact signed proof before transmission.
It preserves packet bytes, hashes and request IDs through status and recovery.
CLI exit codes and terminal rendering stay in the CLI.

The server supplies policy admission, spending limits, trusted context and server-authority bindings.
Its durable journal binds approved requests and settlement evidence to application accounting.
The CLI supplies local signing custody and private journal storage.
SDK engine shares workflow logic between those callers without transferring their keys or authority.

Vendor adapters translate provider protocols. Generic transport, retry budgets and journal mechanics belong in common code.
An adapter exposes an explicit provider recovery contract instead of duplicating the provider's delivery workflow.

## Compiled capabilities and public profiles

The vendor library defines network identities, protocol and presentation aliases, display labels, asset defaults and explorer links. The SDK registers those values for common routing and presentation.
The registered plugins define the rail inventory.

Owner export installs only the exact independently checked activation result.
Explicit public import checks current native authority before saving a profile.
Native imports also check the existing administrator signatures over the exact source and parameters.
Common profile parameters use Core `PolicyValue`. The selected vendor checks native addresses and converts values for its execution model.

The SDK stores public profiles separately from signing keys and private execution journals.
Each profile version retains its complete public binding and server origin.
Historical status consumes a sealed original journal context and a current request signer.
It uses the original signed configuration and packet, without a current profile or authority pin.
That status path cannot prepare or sign a replacement operation.

## Public server authority

`config::ServerAuthority::new(network, origin, public_key)` binds an independently configured public identity to a canonical rail and backend origin.
`profiles::Cache::import` takes that authority and an independent RPC endpoint.
`owner::Context` carries the same authority. The selected vendor checks the exact native role and current administrator quorum.

A public descriptor and an unauthenticated server response cannot supply their own trusted identity.
Fresh Native signing verifies actual source consent and current roles through independent RPC.
The compiler receives an explicit allowlist of public toolchain paths. Provider credentials, signer values and authoring overrides do not enter rustc.

A resolved execution profile and its authority remain owned by one caller across its journal check and dispatch.
The receipt-only recovery capability instead reads the original private record, without importing an obsolete public profile.
It accepts no fresh preparation or operation-signing path. Recovery queries the configured settlement service for the original packet.
A final success requires a verified receipt.

## Provider boundary

The host crate in Core exposes provider contracts behind its optional `vendor` feature.
The SDK retains generic orchestration, storage, routing and consumer integration tests.
It opens the private namespaces `<id>-journal` and `<id>-owner-workflows` for each plugin. A caller-owned store opener preserves
native validation before filesystem changes.
A retained packet serializes as `{"rail": <plugin id>, "packet": <native document>}`. Deserialization restores it through that plugin before exposing retained state.
