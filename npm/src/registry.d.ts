import type {OwnerChain, OwnerStep as CoreOwnerStep, PolicyValue as ParamValue} from '@allowit/core';
import type {
  WalletKindId,
  WalletKindInfo,
  AssetDisplay,
  Quantity,
  Settlement,
  LimitReport,
  SettledBy,
  PaidService,
  PaymentChallenge,
  PaymentProof,
  PaymentLimit,
  WalletLimits,
  OperationReports,
} from '@allowit/core';

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
/**
 * The wallet kinds, the payment values and the reports of operation, receipt, limits and wallet documents: Core's
 * contract. A document carries settlement, paidService, feeCharged, limits and paymentLimits at its top level.
 */
export type {
  WalletKindId,
  WalletKindInfo,
  AssetDisplay,
  Quantity,
  Settlement,
  LimitReport,
  SettledBy,
  PaidService,
  PaymentChallenge,
  PaymentProof,
  PaymentLimit,
  WalletLimits,
  OperationReports,
};
/**
 * The marketplace catalog: Core's contract. An entry keeps the seller's listing apart from what Allowit holds about the
 * offer: its source, its access, what Allowit observed, the owners' reputation labels and its score. `Ranking` is Core's
 * `CatalogRanking`.
 */
export type {
  IndicativeTerms,
  CatalogEntry,
  CatalogSource,
  CatalogOperation,
  CatalogAccess,
  AccessKind,
  Observation,
  CatalogRank,
  SearchResult,
  CatalogRanking as Ranking,
} from '@allowit/core';
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
/** One typed param value: Core's `PolicyValue`, the JSON of a policy value. An integer is canonical decimal text. */
export type {ParamValue};
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
  | 'revision_changed'
  | 'wallet_setup_required'
  | 'owner_account_unfunded'
  | 'owner_balance_low'
  | 'wallet_wrong_account';
/** The error that `installAndFund` and `updateParams` reject with. */
export interface OwnerFlowFailure extends Error {
  code: OwnerFlowCode | string | null;
  /** True when Resume can send the same saved step, or prepare a new one, without a risk of a second payment. */
  retry: boolean;
}

/**
 * The rail hooks of the owner workflows: Core's contract, which the SDK runs and each rail's `ownerChain` implements.
 */
export type {OwnerChain, TransactionRecord, ApprovalFields, PreparedKeypairSetup, SignedKeypairSetup, ReviewedPolicy} from '@allowit/core';

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
/** One owner step, as Core defines it. */
export type OwnerStep = CoreOwnerStep;

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
export const OWNER_ACCOUNT_UNFUNDED: 'owner_account_unfunded';
export const OWNER_BALANCE_LOW: 'owner_balance_low';
export const WALLET_WRONG_ACCOUNT: 'wallet_wrong_account';
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
