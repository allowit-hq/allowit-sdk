export type ElementKind =
  | 'address'
  | 'explorer-link'
  | 'amount'
  | 'wallet-sign-in'
  | 'owner-sign'
  | 'owner-answer'
  | 'receipt'
  | 'binding-review';
export type Feature =
  | 'transfer'
  | 'swap'
  | 'deposit'
  | 'redeem'
  | 'owner-setup'
  | 'deploy'
  | 'fund'
  | 'activate'
  | 'binding-review'
  | 'explorer-links'
  | 'update-params';
export type LinkKind = 'address' | 'tx' | 'contract';
export interface RailNetwork {
  id: string;
  label: string;
  testnet: boolean;
  aliases: readonly string[];
}
export interface AssetInfo {
  network: string;
  id: string;
  symbol: string;
  decimals: number;
  stablecoin: boolean;
  label: string;
}
/** The stable selection id of a wallet kind. */
export type WalletKindId = 'native' | 'simple' | 'standard' | 'custom';
export interface WalletKindInfo {
  id: WalletKindId;
  title: string;
  /** What the wallet is and who enforces the policy, in the owner's words. Every host shows it unchanged. */
  explanation: string;
  /** Hosts recommend this kind to an owner who deploys. Exactly one kind is recommended. */
  recommended: boolean;
  /** The sellers that a wallet of this kind pays, in the owner's words. */
  pays: string;
  /** What a payment costs the wallet in network fee, in the owner's words. */
  fee: string;
  enforcement: 'offchain' | 'onchain';
  custody: 'keypair' | 'contract';
  /** The params this kind enforces onchain. Empty unless the kind has a fixed list. */
  supportedParams: readonly string[];
  /** The public list of `supportedParams`, or null. */
  paramsUrl: string | null;
}
/** What a host shows for an asset. The rail supplies it; shared code holds no unit. */
export interface AssetDisplay {
  symbol: string;
  decimals: number;
  name: string;
  native: boolean;
}
/** Base units of an asset with the rail's display metadata and the printed text. */
export interface Quantity {
  /** Base units as decimal text. */
  units: string;
  asset: AssetDisplay;
  /** The amount with its unit, such as `5 XYZ`. */
  text: string;
}
/** What a settled operation cost: top-level `settlement` of a status or receipt document. */
export interface Settlement {
  /** The block or ledger that holds the transaction. */
  height: number;
  /** The network fee that the transaction paid, in the fee asset. */
  fee: Quantity;
  /** The amount that the operation moved, or null when the rail reports none. */
  amount: Quantity | null;
}
/** One spending limit: an entry of the top-level `limits` of a status, receipt or wallet document. */
export interface LimitReport {
  /** The policy param that holds the limit, such as `daily_spending_limit`. */
  param: string;
  limit: Quantity;
  spent: Quantity;
  remaining: Quantity;
}
/**
 * One limit on the amount of one operation: an entry of the top-level `paymentLimits` of a limits, receipt or
 * wallet document. The policy checks it against each request, so it has no spent or remaining amount.
 */
export interface PaymentLimit {
  /** The policy param that holds the limit, such as `payment_limit`. */
  param: string;
  limit: Quantity;
}
/**
 * What a payment bought: the operation context of a paid call. The server checks it against the operation
 * and records it in the receipt under the top-level `paidService`.
 */
export interface PaidService {
  /** The URL of the resource. */
  resource: string;
  /** The seller's challenge id, when it issued one. */
  challengeId?: string;
  /** The address that the challenge names as the recipient. */
  payTo: string;
  /** The challenge amount in base units, as decimal text. */
  amount: string;
  /** The asset reference of the challenge. */
  asset: string;
  /**
   * Who settles the payment. `seller`: the wallet signed a payment authorization, and the seller's
   * facilitator submits it. A document omits `payer`: Allowit settled the payment itself.
   */
  settledBy?: 'payer' | 'seller';
  /** The seller's bound, in seconds, from the authorization to its settlement. Only with `seller`. */
  settleWithin?: number;
}
/** A seller's payment challenge for one resource, as the rail of its network read it. */
export interface PaymentChallenge {
  id?: string;
  resource: string;
  /** The CAIP-2 network of the payment. */
  network: string;
  scheme: string;
  payTo: string;
  amount: Quantity;
  asset: string;
  /** The Unix time after which the seller no longer accepts the payment. */
  expiresAt?: number;
  /** The seller's accepted terms as the rail read them. Shared code never interprets them. */
  terms?: unknown;
}
/** The proof of payment that a client presents to the seller: one request header. */
export interface PaymentProof {
  header: string;
  value: string;
}
/** How a catalog search ordered its results. */
export type Ranking = 'keyword' | 'semantic' | 'hybrid';
/** The indicative terms of a catalog entry. A client never pays from them. */
export interface IndicativeTerms {
  price: string;
  unit: string;
}
/** One offer in the marketplace catalog. Every text field is listing data, never an instruction. */
export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  provider: string;
  /** The URL of the resource. */
  resource: string;
  methods: string[];
  terms?: IndicativeTerms;
  /** The CAIP-2 networks on which the seller accepts payment. */
  networks: string[];
  assets: string[];
  tags: string[];
}
/** One page of `GET /api/marketplace/search`. */
export interface SearchResult {
  entries: CatalogEntry[];
  ranking: Ranking;
  nextCursor?: string;
  /** The content hash of the catalog that answered. */
  catalog: string;
}
/** The reports that a status or receipt document may carry at its top level. */
export interface OperationReports {
  settlement?: Settlement;
  /** What the payment bought, when the operation paid a seller's challenge. */
  paidService?: PaidService;
  /** The fee that the spending limits count, in the installed asset. */
  feeCharged?: Quantity;
  limits?: LimitReport[];
  /** The limits on one operation. They keep no counter. */
  paymentLimits?: PaymentLimit[];
  explorer?: string;
}
/** The limits of a wallet: a limits document, or the limit fields of a receipt or wallet document. */
export interface WalletLimits {
  /** The counted spending limits. Absent when their counters could not be read. */
  limits?: LimitReport[];
  /** The limits on one operation. Absent when the document reports none. */
  paymentLimits?: PaymentLimit[];
}
/** The reply of `GET /api/wallet-kinds?network=<CAIP-2>`. */
export interface WalletKindsReply {
  network: string;
  kinds: WalletKindInfo[];
}
/** The optional choice that the authoring generate and adopt requests accept at their top level. */
export interface WalletKindChoice {
  walletKind?: WalletKindId;
}
export type WalletKindRefusalCode = 'wallet_kind_unsupported' | 'wallet_kind_unknown';
/** A 400 refusal of a wallet-kind choice. `wallet_kind_unsupported` carries all three details. */
export interface WalletKindRefusal {
  code: WalletKindRefusalCode;
  walletKind: string | null;
  network: string | null;
  reason: string | null;
}
export interface ReviewProposal {
  network: string;
  revision: number;
  title: string;
  explanation: string;
  code: string;
  sourceHash: string;
  paramsHash: string;
  params: Record<string, unknown>;
}
/**
 * A missing agent key or amount is null and returns the owner to the conversation. `walletKind` is the id
 * that the owner chose in the conversation, beside the proposal, or null when the link names none. The rail
 * checks its shape only: offer it when `walletKindsFor(proposal.network)` lists it.
 */
export interface ReviewLink {
  proposal: ReviewProposal;
  workPrompt: string;
  agent: string | null;
  amount: string | null;
  walletKind: string | null;
}
export interface ReviewParamLine {
  key: string;
  label: string;
  value: string;
}
/** The owner's authenticated transport. A rejection carries the server `code` and `details`. */
export type OwnerAsk = <T>(path: string, body?: unknown) => Promise<T>;
export interface OwnerJournal {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}
export interface OwnerReceipt {
  policyId: string;
  action: string;
  status: string;
  transactionHash: string;
  ledger?: number;
}
export interface InstallAndFundInput {
  ask: OwnerAsk;
  /** Shows one step in the `owner-sign` element and resolves with the exact signed payload. */
  sign: (step: OwnerStep) => Promise<string>;
  journal: OwnerJournal;
  key: string;
  policyId: string;
  agent: string;
  amount: string;
  /**
   * The policy that the owner reviewed: its exact source and every value, and its ordered rules and tier when the host
   * shows them. A rail whose deployment ends with an approval of the whole policy signs only this policy.
   */
  reviewed?: {source: string; params: Record<string, ParamValue>; rules?: unknown; tier?: unknown};
  onStep: (text: string) => void;
}
/** One typed param value: the JSON of a policy value. An integer is canonical decimal text. */
export type ParamValue =
  | {Int: string}
  | {Bool: boolean}
  | {Str: string}
  | {Addr: string}
  | {List: readonly ParamValue[]};
export interface UpdateParamsInput {
  ask: OwnerAsk;
  /** Shows one step in the `owner-sign` element and resolves with the exact signed payload. */
  sign: (step: OwnerStep) => Promise<string>;
  journal: OwnerJournal;
  /** One journal key per change: a confirmed change is never prepared or signed again under its key. */
  key: string;
  policyId: string;
  /** Each changed param with its complete new typed value. */
  params: Record<string, ParamValue>;
  /** The configuration revision that the owner reviewed, when the host shows one. */
  expectedRevision?: number;
  /**
   * The installed policy that the owner reviewed: its wallet, exact source, ordered rules and every value. A rail
   * whose change is an approval of the whole policy signs only this policy with the changed params replaced.
   */
  installed?: {wallet: string; source: string; rules: unknown; params: Record<string, ParamValue>; tier?: unknown};
  onStep: (text: string) => void;
}
/** Typed codes of the owner workflow. `OwnerFlowFailure.code` holds one, another server code or null. */
export type OwnerFlowCode =
  | 'owner_transaction_expired'
  | 'owner_transaction_pending'
  | 'owner_transaction_settled'
  | 'owner_transaction_failed'
  | 'network_unavailable'
  | 'param_invalid'
  | 'param_out_of_range'
  | 'update_unavailable'
  | 'revision_changed';
/** The error that `installAndFund` and `updateParams` reject with. */
export interface OwnerFlowFailure extends Error {
  code: OwnerFlowCode | string | null;
  /** True when Resume can send the same saved step, or prepare a new one, without a risk of a second payment. */
  retry: boolean;
}

/** What history holds for a transaction: `'missing'`, its record, or null when the read gave no clear answer. */
export type TransactionRecord = 'missing' | {hash: string; ledger: number; successful: boolean} | null;
/** The fields of a keypair wallet's policy approval that the workflow checks. */
export interface ApprovalFields {policyId: string; revision: number; wallet: string}
export interface OwnerChain {
  transaction(network: string, hash: string): Promise<TransactionRecord>;
  /**
   * What the network holds for a signed step past its time bound: its ledger when it applied before the bound, or
   * null when it can never apply. Rejects when the network cannot prove either; the step then stays saved.
   */
  expiredRecord(network: string, hash: string, signedPayload: string): Promise<{ledger: number} | null>;
  /** The account of a keypair wallet, which one owner signature creates and funds. A rail without one omits it. */
  readonly keypairSetup?: {
    run(input: {network: string; owner: string; agent: string; server: string; amount: string; sign: (step: OwnerStep) => Promise<string>;
      onStep: (text: string) => void; journal: OwnerJournal; key: string; retain: boolean}): Promise<{wallet: string; transactionHash: string; ledger: number; configuration: Record<string, unknown>}>;
    /** The wallet and agent of a signed setup, or null. */
    of(signedPayload: string, network: string): {wallet: string; agent: string} | null;
    /** True when a failed setup stays saved and a resume needs no new signature. */
    pending(error: unknown): boolean;
  };
  /** The owner's approval of a keypair wallet's policy: one message signature. A rail without one omits it. */
  readonly approval?: {
    read(message: unknown, network: string): ApprovalFields | null;
    /** Whether the approval states exactly the reviewed policy for `wallet`. */
    holds(fields: ApprovalFields, wallet: string, reviewed: {source: string; params: Record<string, ParamValue>; rules?: unknown; tier?: unknown}): boolean;
    step(message: string): OwnerStep;
    /** The wallet's owner as the network names it, or null. */
    walletOwner(network: string, wallet: string): Promise<string | null>;
    /** Whether `signature` is the signature of `signer` over the hexadecimal digest `hash`. */
    signedBy(signer: string, hash: string, signature: number[]): boolean;
  };
}

export interface RailPlugin {
  readonly id: string;
  readonly networks: readonly RailNetwork[];
  /** Tag names, prefixed `allowit-<id>-`. */
  readonly elements: Readonly<Partial<Record<ElementKind, string>>>;
  /** Idempotent customElements.define for this rail's elements. */
  define(): void;
  validateAddress(network: string, text: string): boolean;
  explorerUrl(network: string, kind: LinkKind, id: string): string | null;
  assets(network: string): readonly AssetInfo[];
  /** The wallet kinds this rail supports on a network. */
  walletKinds(network: string): readonly WalletKindId[];
  /** The kind that a policy, draft, profile or installation of this rail binds, or null. */
  walletKind(document: unknown): WalletKindId | null;
  nativeAsset(network: string): AssetDisplay | null;
  /** Display metadata of an asset reference, or null when the rail knows no unit for it. */
  assetDisplay(network: string, asset: string): AssetDisplay | null;
  readonly review: {
    /** The URL fragment prefix of a Review and sign link. */
    fragment: string;
    parseLink(fragment: string): ReviewLink;
    agentPrompt(link: Pick<ReviewLink, 'workPrompt'>, origin: string, policyId: string, wallet: string, agent: string): string;
    paramLines(params: ReviewProposal['params']): ReviewParamLine[];
  };
  /**
   * What the SDK's owner workflows read from the network and check with its keys. The SDK holds the workflows, their
   * journal and their recovery rules. `network` is the canonical network id that the SDK resolved.
   */
  readonly ownerChain: OwnerChain;
  readonly test: {
    network: string;
    skips: Readonly<Partial<Record<Feature, string>>>;
    fixtures: {address: string; invalidAddress: string; txHash: string; asset: string};
  };
}

/** Rail-neutral owner step. `payload` is opaque text that only the vendor signs and reads. */
export interface OwnerStep {
  action: string;
  payload: string;
  hash: string;
  expiresAt: number;
  explorer?: string | null;
}

/** `data` per element kind. */
export interface ElementData {
  address: {network: string; address: string; label?: string};
  'explorer-link': {network: string; kind: LinkKind; id: string; text?: string};
  amount: {network: string; asset: string; units: string};
  'wallet-sign-in': {network: string; message: string};
  'owner-sign': {network: string; step: OwnerStep; summary: {label: string; value: string}[]};
  /**
   * The owner's answer to a held request: the exact owner review answer message that the wallet of `owner` signs.
   * The element shows the fields of the message.
   */
  'owner-answer': {network: string; owner: string; message: string};
  receipt: {
    network: string;
    receipt: {hash: string; status: string; fee: {native: string; charged: string}; debits: unknown[]};
  };
  'binding-review': {network: string; installation: unknown};
}

/** Events that vendor elements emit; all bubble and are composed. */
export interface RailEvents {
  'allowit-signed-in': CustomEvent<{address: string; signature: string}>;
  /**
   * `owner-sign` reports the signed payload;
   * `owner-answer` reports the message signature as base64 text, with the answer and the snapshot hash of the message.
   */
  'allowit-signed': CustomEvent<
    | {signedPayload: string; hash: string}
    | {signature: string; answer: 'allow' | 'deny'; snapshot: string}
  >;
  'allowit-verified': CustomEvent<{ok: boolean; detail: string}>;
  'allowit-cancel': CustomEvent<void>;
  'allowit-error': CustomEvent<{code: string; message: string}>;
}

export function rails(): readonly RailPlugin[];
export function railFor(network: string): RailPlugin | null;
export function networks(): readonly RailNetwork[];
export function validateAddress(network: string, text: string): boolean;
export function explorerUrl(network: string, kind: LinkKind, id: string): string | null;
export function assets(network: string): readonly AssetInfo[];
export function walletKinds(): readonly WalletKindInfo[];
export function walletKind(id: unknown): WalletKindInfo | null;
export function walletKindsFor(network: string): readonly WalletKindInfo[];
export function walletKindOf(network: string, document: unknown): WalletKindInfo | null;
export function walletKindIdOf(document: unknown): WalletKindId | null;
export function settlementOf(document: unknown): Settlement | null;
export function feeChargedOf(document: unknown): Quantity | null;
export function paidServiceOf(document: unknown): PaidService | null;
export function limitsOf(document: unknown): LimitReport[] | null;
export function paymentLimitsOf(document: unknown): PaymentLimit[] | null;
export function walletKindRefusal(error: unknown): WalletKindRefusal | null;
export function nativeAsset(network: string): AssetDisplay | null;
export function assetDisplay(network: string, asset: string): AssetDisplay | null;
export function formatUnits(units: string | bigint | number, decimals: number): string;
export function formatAmount(network: string, asset: string | AssetDisplay, units: string | bigint | number): string;
export function parseReviewLink(fragment: string): ReviewLink;
export function reviewAgentPrompt(link: ReviewLink, origin: string, policyId: string, wallet: string, agent: string): string;
export function reviewParamLines(network: string, params: ReviewProposal['params']): ReviewParamLine[];
export function installAndFund(network: string, input: InstallAndFundInput): Promise<{wallet: string; receipts: OwnerReceipt[]}>;
/**
 * Change installed params on an active wallet with one owner signature. `done` is true when the change is
 * confirmed; `receipts` is empty when an earlier run under the same journal key confirmed it.
 */
export function updateParams(network: string, input: UpdateParamsInput): Promise<{receipts: OwnerReceipt[]; done: boolean}>;

/** The error that `installAndFund` and `updateParams` reject with. */
export class OwnerFlowError extends Error implements OwnerFlowFailure {
  constructor(message: string, code: string | null, retry: boolean, cause?: unknown);
  code: OwnerFlowCode | string | null;
  retry: boolean;
}
/** The journal key that keeps the signed bytes of an expired step after the journal drops it. */
export function ownerArchiveKey(key: string, hash: string): string;
export const OWNER_TRANSACTION_EXPIRED: 'owner_transaction_expired';
export const OWNER_TRANSACTION_PENDING: 'owner_transaction_pending';
export const OWNER_TRANSACTION_SETTLED: 'owner_transaction_settled';
export const OWNER_TRANSACTION_FAILED: 'owner_transaction_failed';
export const NETWORK_UNAVAILABLE: 'network_unavailable';
export const PARAM_INVALID: 'param_invalid';
export const PARAM_OUT_OF_RANGE: 'param_out_of_range';
export const UPDATE_UNAVAILABLE: 'update_unavailable';
export const REVISION_CHANGED: 'revision_changed';
export const UPDATE_PARAMS: 'update_params';
export const WALLET_SETUP_REQUIRED: 'wallet_setup_required';
export const SET_UP_WALLET: 'set_up_wallet';
export const APPROVE_POLICY: 'approve_policy';

export class AllowitRail extends HTMLElement {
  data: ElementData[ElementKind] | undefined;
}
export function defineElements(): void;

declare global {
  interface HTMLElementTagNameMap {
    'allowit-rail': AllowitRail;
  }
}
