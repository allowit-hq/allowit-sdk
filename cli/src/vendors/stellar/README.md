# Stellar CLI adapter

This adapter binds the common CLI commands to the [Stellar rail library](https://github.com/allowit-hq/allowit-contracts-stellar/tree/main/stellar/core).
The [workspace manifest](../../../../Cargo.toml) selects its release and the [lockfile](../../../../Cargo.lock) fixes the exact source.
[Adapter wiring](../mod.rs) and the [rail table](../../../../engine/src/routing.rs) register it in the executable.
Run `allowit rail ls --json` and `allowit rail show RAIL --json` for the installed build's networks, aliases and capabilities.

The profile resolution workaround is tracked in [known issues](../../../../known_issues.md).

## Public wallet profile

Set `ALLOWIT_STELLAR_BUNDLE` to the owner's public wallet profile file.
`ALLOWIT_URL` names the policy server origin.
The profile names the network, wallet, owner, agent, server, installed asset and engine endpoints.
It contains no signing material. The adapter checks its network and wallet against the CLI selection.
Selecting an address with `wallet use` does not discover or import this profile.
The selected private key must match the profile's agent before transport or signing.

## Keys and asset amounts

The adapter uses Ed25519 account keys. Public account addresses use G strkeys; policy contracts use C strkeys.
Private key import/export uses Stellar S seeds. The common CLI keeps them in its private key store.
`keys fund` uses Friendbot only on supported Testnet networks, as advertised by `rail show`.

Amounts use `CODE:DECIMAL`, `CODE:ISSUER:DECIMAL`, `C…:DECIMAL` (an asset contract) or integer base units.
`XLM` and `native` identify the same asset. Native asset amounts use seven decimal places.

## Agent requests

The profile and installed asset determine the typed native or SEP-41 transfer.
Policy wallets also accept SEP-56 vault deposits and redemptions with `--token-out` minimums.
Use `rail show` to inspect the selected network's operations and available commands.
`--build-only` prints the typed request and sends nothing.

Policy-wallet signing revalidates the prepared operation through `--rpc-url URL` or the rail's `rpc_url` setting.
This RPC receives only public observations and unsigned simulation bytes.
Classic execution and signed recovery require no RPC override.
Shared SDK engine retains the original operation, submits it and recovers verified receipts.
The adapter supplies native rules; it does not own a separate transport retry or delivery coordinator.

## Owner authoring and signing

Use the wallet profile advertised by the owner's server, for example `allowit-stellar-policy-wallet-v1`.
The [CLI owner-generation instructions](../../../README.md#generate-an-owner-policy) describe the common command.
Generation produces a draft; it does not deploy, fund or activate a wallet.

The hidden `wallet configure` and `wallet approve` commands produce browser signing links for Stellar Testnet.
`configure` binds signer-setting operations of a funded classic account.
`approve` binds policy source, revision, typed params, tier, rules, network and wallet subject.
Both retain their original intent in a working-directory journal and exit 10.
