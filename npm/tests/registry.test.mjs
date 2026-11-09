// Registry tests run against every registered rail; no assertion names a vendor.
import assert from 'node:assert/strict';
import test from 'node:test';

// A minimal DOM, enough for <allowit-rail> to resolve and create vendor elements.
class FakeElement extends EventTarget {
  constructor(tag = 'allowit-rail') {
    super();
    this.localName = tag;
    this.attributes = new Map();
    this.children = [];
    this.isConnected = false;
    this.ownerDocument = globalThis.document;
  }
  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
    this.attributeChangedCallback?.(name, null, String(value));
  }
  replaceChildren(...nodes) {
    this.children = nodes;
  }
}
const defined = new Map();
globalThis.HTMLElement = FakeElement;
globalThis.document = {createElement: (tag) => new FakeElement(tag)};
globalThis.customElements = {
  get: (tag) => defined.get(tag),
  define: (tag, ctor) => {
    if (defined.has(tag)) throw new Error(`${tag} already defined`);
    defined.set(tag, ctor);
  },
};

const sdk = await import('../src/registry.js');
const KINDS = ['address', 'explorer-link', 'amount', 'wallet-sign-in', 'owner-sign', 'receipt', 'binding-review'];
const FEATURES = ['transfer', 'swap', 'deposit', 'redeem', 'owner-setup', 'deploy', 'fund', 'activate', 'binding-review', 'explorer-links'];

test('at least one rail is registered', () => {
  assert.ok(sdk.rails().length > 0);
  assert.equal(customElements.get('allowit-rail'), sdk.AllowitRail);
});

for (const rail of sdk.rails()) {
  const {network, skips, fixtures} = rail.test;

  test(`${rail.id}: identity and networks`, () => {
    assert.match(rail.id, /^[a-z][a-z0-9]*$/);
    assert.ok(rail.networks.length > 0);
    for (const n of rail.networks) {
      assert.match(n.id, /^[-a-z0-9]{3,8}:[-_a-zA-Z0-9]{1,32}$/, 'CAIP-2 id');
      assert.equal(typeof n.label, 'string');
      assert.equal(typeof n.testnet, 'boolean');
      assert.equal(sdk.railFor(n.id), rail);
      for (const alias of n.aliases) assert.equal(sdk.railFor(alias), rail, alias);
    }
    assert.ok(rail.networks.some((n) => n.id === network && n.testnet), 'test network is a registered testnet');
    assert.ok(sdk.networks().some((n) => n.id === network));
  });

  test(`${rail.id}: element tags carry the rail prefix`, () => {
    for (const [kind, tag] of Object.entries(rail.elements)) {
      assert.ok(KINDS.includes(kind), kind);
      assert.ok(tag.startsWith(`allowit-${rail.id}-`), tag);
    }
  });

  test(`${rail.id}: skips name known features with a reason`, () => {
    for (const [feature, reason] of Object.entries(skips)) {
      assert.ok(FEATURES.includes(feature), feature);
      assert.ok(typeof reason === 'string' && reason.length > 0, feature);
    }
  });

  test(`${rail.id}: address validation`, () => {
    assert.equal(sdk.validateAddress(network, fixtures.address), true);
    assert.equal(sdk.validateAddress(network, fixtures.invalidAddress), false);
    assert.equal(sdk.validateAddress('unknown:none', fixtures.address), false);
  });

  test(`${rail.id}: explorer links`, {skip: skips['explorer-links']}, () => {
    for (const [kind, id] of [['address', fixtures.address], ['tx', fixtures.txHash]]) {
      const url = sdk.explorerUrl(network, kind, id);
      assert.ok(url && new URL(url).protocol === 'https:', `${kind} link`);
      assert.ok(url.includes(id), `${kind} link names the id`);
    }
    assert.equal(sdk.explorerUrl('unknown:none', 'tx', fixtures.txHash), null);
  });

  test(`${rail.id}: assets`, () => {
    const list = sdk.assets(network);
    assert.ok(list.some((a) => a.id === fixtures.asset || a.symbol === fixtures.asset), 'fixture asset is published');
    for (const a of list) {
      assert.equal(a.network, network);
      assert.ok(Number.isInteger(a.decimals) && a.decimals >= 0);
      assert.equal(typeof a.stablecoin, 'boolean');
      assert.ok(a.symbol && a.label);
    }
    assert.deepEqual(sdk.assets('unknown:none'), []);
  });

  for (const [kind, tag] of Object.entries(rail.elements)) {
    test(`${rail.id}: <allowit-rail kind=${kind}> creates ${tag} and forwards data`, () => {
      const host = new sdk.AllowitRail();
      host.setAttribute('kind', kind);
      host.setAttribute('network', network);
      const data = {network};
      host.data = data;
      host.isConnected = true;
      host.connectedCallback();
      assert.equal(host.children.length, 1);
      assert.equal(host.children[0].localName, tag);
      assert.equal(host.children[0].data, data);
      const next = {network, next: true};
      host.data = next;
      assert.equal(host.children[0].data, next);
    });
  }
}

test('<allowit-rail> reports an unknown network', () => {
  const host = new sdk.AllowitRail();
  host.setAttribute('kind', 'address');
  host.setAttribute('network', 'unknown:none');
  let error = null;
  host.addEventListener('allowit-error', (event) => (error = event.detail));
  host.isConnected = true;
  host.connectedCallback();
  assert.equal(host.children.length, 0);
  assert.equal(error?.code, 'unsupported');
});
