# Register a policy vendor

A host pins the vendor package in Cargo and registers its compiled bindings.

```rust
use allowit_sdk::vendor_bindings::Registry;

let mut engine = Registry::default();
engine.register(etherfuse::bindings())?;
```

The package supplies its namespace, named functions and a self-test in the binding.
The host uses the same registry to discover operations, admit calls and dispatch them.
One generic integration test calls `engine.self_test()` for all registered packages.

A compiled rail can use `engine.register_rail(stellar::provider::bindings())?`.
The host reserves the rail namespaces for this path.

Functions receive policy arguments and trusted host facts separately. A binding must
validate its inputs and return an observation or a proposal. The host owns
authentication, authorization, signing and settlement. Private packages belong in
private host composition and use the same registration API.
