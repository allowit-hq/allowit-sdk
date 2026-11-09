// Vendor-neutral rail registry and the generic <allowit-rail> element.
// Hosts pass data in and listen for allowit-* events; the registered plugin renders the rest.
import * as registered from './registered.js';

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
