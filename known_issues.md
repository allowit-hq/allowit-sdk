# Known issues

## Public wallet profile resolution

Selecting a wallet address does not resolve its public execution profile.
The Stellar adapter requires the owner's public profile file through `ALLOWIT_STELLAR_BUNDLE`.
Follow its [configuration reference](cli/src/vendors/stellar/README.md#public-wallet-profile).
The file contains no signing material and must match the selected network, wallet and agent.
Remove this workaround when the adapter can resolve the required public profile behind common wallet selection.

## Native adapter bindings

[Command dispatch](cli/src/vendors/mod.rs) and wallet-profile selection still choose the bundled native implementation directly.
Native key encoding, signing and message domains now live in the vendor library.
Common key custody and owner generation use registered SDK capabilities.
Use `allowit rail ls --json` and `allowit rail show RAIL --json` for the compiled capabilities.
Adding a dependency and registry entry alone does not supply another vendor's command or profile bindings.
Finish those bindings before admitting another implementation. Shared execution and recovery stay in SDK engine.
