# AllowIt agent guide

An AllowIt policy wallet holds funds. Its policy decides what the agent key may
spend. Your agent key signs requests; the policy on chain checks each one.

## 1. Select the rail

```sh
allowit rail ls
allowit rail use stellar:testnet
```

Rails accept aliases (`stellar:testnet`, `solana:devnet`, `tempo:testnet`).
Output always uses the CAIP-2 identifier. A command that needs a rail exits 3
until one is selected.

## 2. Create and register your key

```sh
allowit keys generate agent --fund
allowit setup --agent agent
allowit whoami
```

Private keys stay in the config directory. No flag accepts a secret.

## 3. Use the owner's wallet

```sh
allowit wallet use CWALLET…
allowit wallet show
allowit policy show
```

## 4. Check, then spend

```sh
allowit eval --op transfer --to GDEST… --amount XLM:5
allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-001
allowit status pay-001 --wait 60s
```

`allowit rail show` lists the verbs that work on the rail; a verb that does
not work there exits 3. `exec` prints a new request ID on stderr when you give
none. After exit 5, retry only with the same `--request-id`, or read the result
with `status`.
`history` lists your requests.

Amounts: `CODE:DECIMAL` (`USDC:10`), `CODE:ISSUER:DECIMAL`, `C…:DECIMAL`
(Stellar asset contract) or an `INTEGER` of base units. `XLM` and `native`
name the same asset.

## Exit codes

| Code | Meaning | What to do |
|---|---|---|
| 0 | ok | |
| 2 | usage error | fix the command |
| 3 | configuration, or not available on this rail | follow the hint |
| 4 | rejected by the contract (paused, wrong asset) | ask the owner |
| 5 | uncertain | retry with the same request ID |
| 6 | replayed | the earlier result stands |
| 10 | owner signature needed | the owner opens the printed link |
| 11 | waiting for owner input | stop and report the prompt |
| 12 | pending | `allowit status ID --wait 60s` |
| 20 | denied by the policy | do not retry |

## Output

Text is the default. Add `--json` for JSON. Data goes to stdout; links, hints
and request IDs go to stderr.
