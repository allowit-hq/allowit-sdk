// Generated from Core `vendor::kinds::WALLET_KINDS` by the SDK test `npm_wallet_kinds_equal_the_core_description`.
export default Object.freeze([
  {
    "id": "native",
    "title": "Native",
    "explanation": "Allowit enforces the policy offchain, and a keypair account holds the funds.",
    "enforcement": "offchain",
    "custody": "keypair",
    "supportedParams": [],
    "paramsUrl": null
  },
  {
    "id": "simple",
    "title": "Simple",
    "explanation": "Allowit enforces the policy offchain, as for Native, and a smart-contract wallet holds the funds.",
    "enforcement": "offchain",
    "custody": "contract",
    "supportedParams": [],
    "paramsUrl": null
  },
  {
    "id": "standard",
    "title": "Standard",
    "explanation": "A smart-contract wallet enforces the standard policy onchain for the supported params.",
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
    "explanation": "A custom contract, built from the policy source, enforces the policy onchain.",
    "enforcement": "onchain",
    "custody": "contract",
    "supportedParams": [],
    "paramsUrl": null
  }
].map((kind) => Object.freeze({...kind, supportedParams: Object.freeze(kind.supportedParams)})));
