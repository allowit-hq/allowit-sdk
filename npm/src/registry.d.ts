export type ElementKind =
  | 'address'
  | 'explorer-link'
  | 'amount'
  | 'wallet-sign-in'
  | 'owner-sign'
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
  | 'explorer-links';
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
  /** One sentence that every host shows unchanged. */
  explanation: string;
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
/** The reports that a status or receipt document may carry at its top level. */
export interface OperationReports {
  settlement?: Settlement;
  /** The fee that the spending limits count, in the installed asset. */
  feeCharged?: Quantity;
  limits?: LimitReport[];
  explorer?: string;
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
  onStep: (text: string) => void;
}
/** Typed codes of the owner workflow. `OwnerFlowFailure.code` holds one, another server code or null. */
export type OwnerFlowCode =
  | 'owner_transaction_expired'
  | 'owner_transaction_pending'
  | 'owner_transaction_settled'
  | 'network_unavailable';
/** The error that `installAndFund` rejects with. */
export interface OwnerFlowFailure extends Error {
  code: OwnerFlowCode | string | null;
  /** True when Resume can send the same saved step, or prepare a new one, without a risk of a second payment. */
  retry: boolean;
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
  readonly ownerWorkflow: {
    /** `network` is the canonical network id that the SDK resolved. */
    installAndFund(input: InstallAndFundInput & {network: string}): Promise<{wallet: string; receipts: OwnerReceipt[]}>;
  };
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
  receipt: {
    network: string;
    receipt: {hash: string; status: string; fee: {native: string; charged: string}; debits: unknown[]};
  };
  'binding-review': {network: string; installation: unknown};
}

/** Events that vendor elements emit; all bubble and are composed. */
export interface RailEvents {
  'allowit-signed-in': CustomEvent<{address: string; signature: string}>;
  'allowit-signed': CustomEvent<{signedPayload: string; hash: string}>;
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
export function limitsOf(document: unknown): LimitReport[] | null;
export function walletKindRefusal(error: unknown): WalletKindRefusal | null;
export function nativeAsset(network: string): AssetDisplay | null;
export function assetDisplay(network: string, asset: string): AssetDisplay | null;
export function formatUnits(units: string | bigint | number, decimals: number): string;
export function formatAmount(network: string, asset: string | AssetDisplay, units: string | bigint | number): string;
export function parseReviewLink(fragment: string): ReviewLink;
export function reviewAgentPrompt(link: ReviewLink, origin: string, policyId: string, wallet: string, agent: string): string;
export function reviewParamLines(network: string, params: ReviewProposal['params']): ReviewParamLine[];
export function installAndFund(network: string, input: InstallAndFundInput): Promise<{wallet: string; receipts: OwnerReceipt[]}>;

export class AllowitRail extends HTMLElement {
  data: ElementData[ElementKind] | undefined;
}
export function defineElements(): void;

declare global {
  interface HTMLElementTagNameMap {
    'allowit-rail': AllowitRail;
  }
}
