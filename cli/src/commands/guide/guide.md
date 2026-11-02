# AllowIt agent guide

An AllowIt policy wallet holds funds. Its policy decides what the agent key may
spend. Your agent key signs requests; the policy on chain checks each one.

## 1. Select the rail

```sh
allowit rail ls
allowit rail use stellar:testnet
```

The rails are `stellar:testnet` and `stellar:pubnet`; `stellar:mainnet` aliases `stellar:pubnet`.
Output always uses the CAIP-2 identifier. A command that needs a rail exits 3
until one is selected.

## 2. Create and select your key

```sh
allowit keys generate agent --fund
allowit keys use agent
```

Private keys stay in the config directory. No flag accepts a secret.

## 3. Use the owner's wallet

```sh
allowit wallet use CWALLET…
allowit wallet show
allowit policy show
```

## 4. Spend

```sh
allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-0001
allowit status pay-0001 --wait 60s
```

Set `ALLOWIT_STELLAR_BUNDLE` to the owner's public wallet profile and `ALLOWIT_URL` to the server origin.
The selected key must match the profile's agent. `allowit rail show` lists the supported operations.
`exec` prints a generated request ID on stderr when none was supplied.
After exit 5, retry only with the same `--request-id`, or read the result with `status`.
`history` lists local requests.

Amounts: `CODE:DECIMAL` (`USDC:10`), `CODE:ISSUER:DECIMAL`, `C…:DECIMAL`
(Stellar asset contract) or an `INTEGER` of base units. `XLM` and `native`
name the same asset.

## Generate an owner draft

An owner can generate a portable draft with a separate named local key:

```sh
allowit keys import owner owner.key
allowit --json policy generate --owner-key owner --origin http://localhost:5173 \
  --profile allowit-stellar-policy-wallet-v1 --new-wallet \
  'Pay only my approved recipient, up to 1 XLM per payment'
```

`ALLOWIT_URL` names the backend. The sign-in Origin must be allowed by that backend.
The owner key does not replace the selected agent key. `--new-wallet` requires an advertised template;
`--installation ID` explicitly selects an existing owner installation instead.
Generation returns a draft. It does not deploy, fund or activate a wallet.
Exit 11 means that owner input remains unresolved. After uncertain generation, inspect the owner workspace before creating another draft.

## Exit codes

| Code | Meaning | What to do |
|---|---|---|
| 0 | ok | |
| 2 | usage error | fix the command |
| 3 | configuration, or not available on this rail | follow the hint |
| 4 | rejected by the contract (paused, wrong asset) | ask the owner |
| 5 | uncertain | preserve the original operation; inspect an uncertain generation before creating another |
| 6 | replayed | the earlier result stands |
| 10 | owner signature needed | the owner opens the printed link |
| 11 | waiting for owner input | stop and report the prompt |
| 12 | pending | `allowit status ID --wait 60s` |
| 20 | denied by the policy | do not retry |

## Output

Text is the default. Add `--json` for JSON. Data goes to stdout, including an
owner signing link, which is the `signingUrl` field. Hints and request IDs go
to stderr.
