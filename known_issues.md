# Known issues

## Public wallet profile resolution

Selecting a wallet address does not resolve its public execution profile.
The Stellar adapter requires the owner's public profile file through `ALLOWIT_STELLAR_BUNDLE`;
follow its [configuration reference](cli/src/vendors/stellar/README.md#public-wallet-profile).
The file contains no signing material and must match the selected network, wallet and agent.
Remove this workaround when the adapter can resolve the required public profile behind common wallet selection.

## Native key and owner-client bindings

[Key codecs](engine/src/wallet.rs) and [owner generation](cli/src/commands/policy/generate.rs)
still call the bundled native library directly.
Use the registered capabilities from `allowit rail ls --json` and `allowit rail show RAIL --json`.
Adding a dependency and registry entry alone does not supply another vendor's key codec or owner client.
Move these native bindings behind the adapter boundary before admitting another implementation.
