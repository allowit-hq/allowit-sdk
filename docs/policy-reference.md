# Portable policies

A policy is one self-contained synchronous Rust file with this entrypoint:

```rust
use allowit::prelude::*;

pub fn execute(ctx: &AllowitContext) -> allowit::Result {
    allowit::operations(ctx, &ctx.params[allowit::OPERATIONS])?;
    allowit::daily_spending_limit(ctx, &ctx.params[allowit::DAILY_SPENDING_LIMIT])?;
    allowit::payment_limit(ctx, &ctx.params[allowit::PAYMENT_LIMIT])?;
    Ok(())
}
```

The `allowit` package is the portable module of [Allowit core](https://github.com/allowit-hq/allowit-core).
Its `std` feature enables host evaluation. Native adapters bind the portable source to contract backends.
The shared Core source gate admits only the portable vocabulary and one `execute` entrypoint.
Host compilation calls `rustc` directly. It uses the same exact Core revision as the gate and portable library.

## Authoring order

Start the policy's rule sequence with `allowit::operations(ctx, &ctx.params[allowit::OPERATIONS])?`.
Its second argument supplies the owner's allowed operation set.
The owner chooses the operation set. The generator places this rule first, then follows the order of the owner's remaining requirements.
Server generation owns this order. Consumers display it unchanged and preserve explicit owner edits to parameter values.

## Source and state

Policy source can use the admitted `allowit::` and `core` paths, fixed-width integers, local functions and portable control flow.
The gate rejects file and environment access, unsafe code, foreign dependencies, pointer-width values, floating-point values and macros outside its allowlist.
A valid source shape still needs Rust type-checking and native compilation before installation.

`ctx.request` contains authenticated operation facts. `ctx.params` contains owner-approved configuration.
A source reads canonical params by their `allowit` keys. Missing or invalid required values deny execution.
Amounts use exact integer base units; the native asset binding supplies their meaning.

The canonical rules cover payment limits, lifetime and daily spending, daily volume, call count, destinations, tokens, expiry and operations.
The shared library checks state and performs accounting around policy evaluation.
Custom policies use the admitted `get_state` and `set_state` interfaces for bespoke state.
Trusted hosts supply enrichment and application accounting; request JSON cannot replace those bindings.

## Native binding

The selected rail derives the exact native request, effect and fee bounds, authorizers, simulation and settlement rules.
Its Simple, Common and Custom wallet implementations bind source, code and owner configuration.
Common uses the owner-approved leading canonical rule calls. Custom compiles the complete portable source.
Simple requires full trusted host evaluation before the server authorizes execution.
Simple and Common reject bespoke state writes.

`authoring::check` checks source with the portable profile, which a contract compiles. `authoring::check_server` checks source that a server evaluates: it admits the server functions of the build's registry and the text of an owner param, read with `as_str()`, and it names the server functions that the source calls. `authoring::compile` and `authoring::compile_server` type-check the source after the matching profile.

`authoring::onchain_part` derives the on-chain part of one policy source for a Custom wallet. The owner writes and signs one source. The server evaluates all of it before it signs. The contract of a Custom wallet compiles the part that Core derives: a statement stays when the chain can check it by itself, and it leaves when it depends on a server function or on policy state that the server keeps. The part never denies a request that the whole source allows, so a rule in a branch that depends on a server function leaves too; the result lists each such rule with its line, its cause and whether the owner can move it. The result also names the cumulated figures that the kept statements read. A source that names no server function is its own on-chain part.

A policy may keep its own state with `get_state` and `set_state`. The result names the keeper of each key (`Derived::state`). The contract keeps a key when every statement that writes it stays in the on-chain part, and the server reads it from the contract. The server keeps a key when a statement that writes it leaves; then every statement that uses the key leaves. A key that starts with `day_` lives one daily window. A state key is a string literal of 1 to 32 ASCII letters, digits or `_`, and no policy writes a standard figure. Prefer a policy that stores little: no state; then a figure that a canonical rule keeps or that the request gives; then one small value under a short key.

Classic custody runs a canonical-only policy in full on every prepare and sign path.
Any other source, such as one that calls a server function, needs the server's compiled evaluator: the server admits each prepared operation through `authorize_preparation` before it stores and signs it.
Classic custody rejects bespoke state writes.
Compiler-language packets serve only verification and status recovery.
They cannot authorize new preparation, signatures or a replacement payment.

The server binds source and compiled code to the owner's approved record and supplies trusted context, its signing authority, reservations and durable accounting.
SDK engine preserves operation identity and exact signed bytes across execution and recovery.
Each caller retains its keys and storage authority.
