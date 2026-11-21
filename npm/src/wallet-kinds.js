// Generated from Core `vendor::kinds::WALLET_KINDS` by the SDK test `npm_wallet_kinds_equal_the_core_description`.
export default Object.freeze([
  {
    "id": "native",
    "title": "Native",
    "explanation": "Your agent's own wallet. Allowit checks each payment against your policy before it co-signs.",
    "recommended": true,
    "pays": "Every x402 seller.",
    "fee": "The lowest: none when the seller's facilitator settles the payment.",
    "enforcement": "offchain",
    "custody": "keypair",
    "supportedParams": [],
    "paramsUrl": null
  },
  {
    "id": "simple",
    "title": "Simple",
    "explanation": "A contract wallet holds the funds. Allowit checks each payment against your policy, as for Native.",
    "recommended": false,
    "pays": "Fewer sellers: only those that accept a settled payment as proof.",
    "fee": "Higher: each payment runs the wallet contract on chain.",
    "enforcement": "offchain",
    "custody": "contract",
    "supportedParams": [],
    "paramsUrl": null
  },
  {
    "id": "standard",
    "title": "Standard",
    "explanation": "A contract wallet enforces the standard rules of your policy on chain.",
    "recommended": false,
    "pays": "Fewer sellers: only those that accept a settled payment as proof.",
    "fee": "Higher: each payment runs the wallet contract on chain.",
    "enforcement": "onchain",
    "custody": "contract",
    "supportedParams": [
      "payment_limit",
      "lifetime_spending_limit",
      "daily_spending_limit",
      "daily_volume_limit",
      "call_limit",
      "destinations",
      "tokens",
      "expires_at",
      "operations"
    ],
    "paramsUrl": "https://github.com/allowit-hq/allowit-core/blob/main/README.md#standard-wallet-parameters"
  },
  {
    "id": "custom",
    "title": "Custom",
    "explanation": "A contract from your policy enforces each step that the chain can check. Allowit checks the rest before it co-signs.",
    "recommended": false,
    "pays": "Fewer sellers: only those that accept a settled payment as proof.",
    "fee": "Higher: each payment runs the wallet contract on chain.",
    "enforcement": "onchain",
    "custody": "contract",
    "supportedParams": [],
    "paramsUrl": null
  }
].map((kind) => Object.freeze({...kind, supportedParams: Object.freeze(kind.supportedParams)})));
