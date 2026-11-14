// Vendor-neutral rail registry and the generic <allowit-rail> element.
// Hosts pass data in and listen for allowit-* events; the registered plugin renders the rest.
import * as registered from './registered.js';
import WALLET_KINDS from './wallet-kinds.js';

const RAILS = Object.freeze(Object.values(registered));

/** Every registered rail plugin, in registration order. */
export function rails() {
  return RAILS;
}

/** Every network of every registered rail. */
export function networks() {
  return RAILS.flatMap((rail) => rail.networks);
}

function lookup(network) {
  for (const rail of RAILS) {
    const found = rail.networks.find((n) => n.id === network || n.aliases.includes(network));
    if (found) return {rail, network: found};
  }
  return null;
}

/** The plugin that serves a CAIP-2 network id or alias, or null. */
export function railFor(network) {
  return lookup(network)?.rail ?? null;
}

/** Whether `text` is a valid address on the network. Unknown networks reject every address. */
export function validateAddress(network, text) {
  const hit = lookup(network);
  return hit ? hit.rail.validateAddress(hit.network.id, text) : false;
}

/** The explorer URL for an address, transaction or contract, or null when the rail has none. */
export function explorerUrl(network, kind, id) {
  const hit = lookup(network);
  return hit ? hit.rail.explorerUrl(hit.network.id, kind, id) : null;
}

/** Asset and stablecoin metadata that the rail publishes for the network. */
export function assets(network) {
  const hit = lookup(network);
  return hit ? hit.rail.assets(hit.network.id) : [];
}

/** The four wallet kinds, in selection order, with the text that every host shows. */
export function walletKinds() {
  return WALLET_KINDS;
}

/** The description of one kind by its selection id, or null. Ids are exact; there are no aliases. */
export function walletKind(id) {
  return WALLET_KINDS.find((kind) => kind.id === id) ?? null;
}

/** The kinds that the rail of `network` supports, in selection order. */
export function walletKindsFor(network) {
  const hit = lookup(network);
  if (!hit) return [];
  const offered = hit.rail.walletKinds(hit.network.id);
  return WALLET_KINDS.filter((kind) => offered.includes(kind.id));
}

/**
 * The kind that a policy, draft, wallet profile or installation binds, or null when it binds none.
 * The document's rail reads its own binding, so hosts never read a rail-specific field.
 */
export function walletKindOf(network, document) {
  const hit = lookup(network);
  return walletKind(hit ? hit.rail.walletKind(document) : null);
}

const WALLET_KIND_IDS = new Set(WALLET_KINDS.map((kind) => kind.id));
const record = (value) => (value !== null && typeof value === 'object' && !Array.isArray(value) ? value : null);

/**
 * The wallet kind id at the top level of a server draft, policy or wallet document (`walletKind`), or null
 * when the document names none or names an unknown id. Ids are exact.
 */
export function walletKindIdOf(document) {
  const id = record(document)?.walletKind;
  return typeof id === 'string' && WALLET_KIND_IDS.has(id) ? id : null;
}

/** A quantity `{units, asset, text}` of a server document, or null when the value is not one. */
function quantity(value) {
  const item = record(value);
  const asset = record(item?.asset);
  return item && asset && typeof item.units === 'string' && /^-?[0-9]+$/.test(item.units)
    && typeof item.text === 'string' && typeof asset.symbol === 'string'
    && Number.isInteger(asset.decimals) && asset.decimals >= 0
    ? item
    : null;
}

/** The settlement `{height, fee, amount}` of a status or receipt document, or null. */
export function settlementOf(document) {
  const item = record(record(document)?.settlement);
  if (!item || !Number.isSafeInteger(item.height) || !quantity(item.fee)) return null;
  return item.amount === null || item.amount === undefined || quantity(item.amount) ? item : null;
}

/**
 * The paid-service context `{resource, challengeId?, payTo, amount, asset}` that a status or receipt
 * document records, or null. The resource is an `http` or `https` URL; every field is bounded text.
 */
export function paidServiceOf(document) {
  const item = record(record(document)?.paidService);
  const text = (value, max) => typeof value === 'string' && value.length > 0 && value.length <= max
    && /^[\x21-\x7e]+$/.test(value);
  if (!item || !text(item.resource, 2048) || !/^https?:\/\//.test(item.resource)) return null;
  if (!text(item.payTo, 128) || !text(item.asset, 128) || !/^[1-9][0-9]{0,38}$/.test(String(item.amount))) return null;
  if (typeof item.amount !== 'string') return null;
  if (item.challengeId !== undefined && !/^[A-Za-z0-9._:-]{1,128}$/.test(String(item.challengeId))) return null;
  if (Object.keys(item).some((key) => !['resource', 'challengeId', 'payTo', 'amount', 'asset'].includes(key))) return null;
  return item;
}

/** The fee that the spending limits count for an operation (`feeCharged`), or null. */
export function feeChargedOf(document) {
  return quantity(record(document)?.feeCharged);
}

/**
 * The spending limits of a status, receipt or wallet document: `{param, limit, spent, remaining}` per limit.
 * Null when the document reports none or one entry is malformed; an empty list is a report of no limits.
 */
export function limitsOf(document) {
  const list = record(document)?.limits;
  if (!Array.isArray(list)) return null;
  const valid = list.every((entry) => {
    const item = record(entry);
    return item && typeof item.param === 'string' && item.param
      && quantity(item.limit) && quantity(item.spent) && quantity(item.remaining);
  });
  return valid ? list : null;
}

/**
 * The per-payment limits of a limits, receipt or wallet document: `{param, limit}` per limit on one operation.
 * Such a limit keeps no counter. Null when the document reports none or one entry is malformed; an empty list is
 * a report of no such limit.
 */
export function paymentLimitsOf(document) {
  const list = record(document)?.paymentLimits;
  if (!Array.isArray(list)) return null;
  const valid = list.every((entry) => {
    const item = record(entry);
    return item && typeof item.param === 'string' && item.param && quantity(item.limit);
  });
  return valid ? list : null;
}

/**
 * The typed wallet-kind refusal of a rejected generate or adopt request, or null. `error` is the rejection of
 * the host transport, or the parsed error body: `{code, error, details: {walletKind, network, reason}}`.
 */
export function walletKindRefusal(error) {
  const item = record(error);
  const code = item?.code;
  if (code !== 'wallet_kind_unsupported' && code !== 'wallet_kind_unknown') return null;
  const details = record(item.details) ?? {};
  const text = (value) => (typeof value === 'string' && value ? value : null);
  return {
    code,
    walletKind: text(details.walletKind),
    network: text(details.network),
    reason: text(details.reason),
  };
}

/** Display metadata of the network's native asset, or null for an unknown network. */
export function nativeAsset(network) {
  const hit = lookup(network);
  return hit ? hit.rail.nativeAsset(hit.network.id) : null;
}

/** Display metadata of an asset reference, or null when the rail knows no unit for it. */
export function assetDisplay(network, asset) {
  const hit = lookup(network);
  return hit ? hit.rail.assetDisplay(hit.network.id, asset) : null;
}

/**
 * Base units as an exact decimal without trailing zeros. `units` is integer text, a bigint or a safe
 * integer number. A number beyond the safe range has already lost its exact value, so it throws.
 */
export function formatUnits(units, decimals) {
  const exact = typeof units === 'string' || typeof units === 'bigint' || Number.isSafeInteger(units);
  const text = exact ? String(units) : '';
  if (!/^-?\d+$/.test(text) || !Number.isInteger(decimals) || decimals < 0 || decimals > 255) {
    throw new TypeError('units must be integer text, a bigint or a safe integer, and decimals a count from 0 to 255');
  }
  const negative = text.startsWith('-');
  const digits = (negative ? text.slice(1) : text).replace(/^0+(?=\d)/, '').padStart(decimals + 1, '0');
  const whole = digits.slice(0, digits.length - decimals);
  const fraction = digits.slice(digits.length - decimals).replace(/0+$/, '');
  return `${negative && /[1-9]/.test(digits) ? '-' : ''}${whole}${fraction ? `.${fraction}` : ''}`;
}

/**
 * Base units of an asset as an amount with the rail's unit, such as `5 XYZ`. `asset` is an asset
 * reference, or the display metadata itself. An asset without a known unit throws; nothing prints
 * a placeholder unit.
 */
export function formatAmount(network, asset, units) {
  const display = typeof asset === 'string' ? assetDisplay(network, asset) : asset;
  if (!display) throw new Error(`No unit is known for ${String(asset)} on ${network}`);
  return `${formatUnits(units, display.decimals)} ${display.symbol}`;
}

/**
 * Decode and check a Review and sign link. The rail of the proposal checks it; an unknown rail or a
 * malformed link throws. The result has `network` so the host can dispatch the workflow, and
 * `walletKind`: the id that the link names beside the proposal, or null. A kind inside the proposal throws.
 */
export function parseReviewLink(fragment) {
  for (const rail of RAILS) {
    if (typeof fragment !== 'string' || !fragment.startsWith(rail.review.fragment)) continue;
    try {
      const link = rail.review.parseLink(fragment);
      if (lookup(link.proposal.network)?.rail === rail) return link;
    } catch {
      // Another rail may own this link.
    }
  }
  throw new Error('The agent policy link is invalid.');
}

function hitOf(network) {
  const hit = lookup(network);
  if (!hit) throw new Error(`No rail is registered for ${network}`);
  return hit;
}
const railOf = (network) => hitOf(network).rail;

/** The prompt that the owner gives the agent after signing. */
export function reviewAgentPrompt(link, origin, policyId, wallet, agent) {
  return railOf(link.proposal.network).review.agentPrompt(link, origin, policyId, wallet, agent);
}

/** Owner-readable lines for the params of a proposal. */
export function reviewParamLines(network, params) {
  return railOf(network).review.paramLines(params);
}

/**
 * Install the reviewed policy wallet, then fund it. The host supplies its transport, the signing of
 * one owner step and a journal; the rail settles a saved signed step before it signs another one.
 * An unknown network rejects.
 */
export async function installAndFund(network, input) {
  const hit = hitOf(network);
  return hit.rail.ownerWorkflow.installAndFund({...input, network: hit.network.id});
}

const Base = globalThis.HTMLElement ?? class {};

/**
 * <allowit-rail kind="owner-sign" network="…">. Set `.data`; the matching vendor element receives it.
 * Vendor events bubble through this element unchanged. An unknown network or kind emits allowit-error.
 */
export class AllowitRail extends Base {
  #data = undefined;
  #child = null;

  static get observedAttributes() {
    return ['kind', 'network'];
  }

  get data() {
    return this.#data;
  }

  set data(value) {
    this.#data = value;
    if (this.#child) this.#child.data = value;
  }

  connectedCallback() {
    this.#render();
  }

  attributeChangedCallback() {
    if (this.isConnected) this.#render();
  }

  #render() {
    const kind = this.getAttribute('kind');
    const network = this.getAttribute('network');
    const rail = network ? railFor(network) : null;
    const tag = rail && Object.hasOwn(rail.elements, kind) ? rail.elements[kind] : null;
    if (!tag) {
      this.#child = null;
      this.replaceChildren();
      this.dispatchEvent(
        new CustomEvent('allowit-error', {
          bubbles: true,
          composed: true,
          detail: {code: 'unsupported', message: `No ${kind} element for ${network}`},
        }),
      );
      return;
    }
    if (this.#child?.localName === tag) {
      this.#child.data = this.#data;
      return;
    }
    rail.define();
    const child = this.ownerDocument.createElement(tag);
    child.data = this.#data;
    this.#child = child;
    this.replaceChildren(child);
  }
}

/** Define <allowit-rail> once. Importing this module in a browser does it. */
export function defineElements() {
  const registry = globalThis.customElements;
  if (registry && !registry.get('allowit-rail')) registry.define('allowit-rail', AllowitRail);
}

defineElements();
