# Known issues

## Public wallet profile resolution

Selecting a wallet address does not resolve its public execution profile.
The Stellar adapter requires the owner's public profile file through `ALLOWIT_STELLAR_BUNDLE`.
Follow its [configuration reference](cli/src/vendors/stellar/README.md#public-wallet-profile).
The file contains no signing material and must match the selected network, wallet and agent.
Remove this workaround when the adapter can resolve the required public profile behind common wallet selection.

## Native adapter bindings

[Key codecs](engine/src/wallet.rs), [owner generation](cli/src/commands/policy/generate.rs)
and [command dispatch](cli/src/vendors/mod.rs) still select the bundled native implementation directly.
[Owner authentication](engine/src/stellar/owner_client.rs) also retains native sign-in encoding and signature checks.
Use the registered capabilities from `allowit rail ls --json` and `allowit rail show RAIL --json`.
Adding a dependency and registry entry alone does not supply another vendor's native bindings.
Move these bindings behind vendor capabilities before admitting another implementation.
Keep shared execution and recovery in SDK engine.
