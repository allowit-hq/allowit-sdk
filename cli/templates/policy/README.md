# Portable policy template

Replace [`src/policy.rs`](src/policy.rs) with your policy. [`src/lib.rs`](src/lib.rs) includes that file unchanged.
The crate uses the portable `allowit` package's host `std` feature. Soroban contracts compile the same authored source with its Soroban backend.

```sh
cargo test --locked -p allowit-policy-template
allowit policy check cli/templates/policy/src/policy.rs
allowit policy build cli/templates/policy/src/policy.rs -o wallet.rs
```

The template reads the owner-approved daily and per-payment limits. It denies when a required limit is absent or invalid.
Amounts use native base units. The executing host and rail supply authenticated request facts and accounting.

```rust
use allowit::prelude::*;

pub fn execute(ctx: &AllowitContext) -> allowit::Result {
    allowit::daily_spending_limit(ctx, &ctx.params[allowit::DAILY_SPENDING_LIMIT])?;
    allowit::payment_limit(ctx, &ctx.params[allowit::PAYMENT_LIMIT])?;
    Ok(())
}
```

Cargo type-checks this file. The shared source gate validates its portable structure.
An installation binds the exact source and owner configuration. A policy wallet also binds its installed contract code through owner-approved deployment.
