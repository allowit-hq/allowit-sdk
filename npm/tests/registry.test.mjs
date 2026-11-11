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

for (const kind of ['constructor', 'toString', '__proto__', 'unknown-kind']) {
  test(`<allowit-rail> reports the unsupported kind ${kind}`, () => {
    const rail = sdk.rails()[0];
    const host = new sdk.AllowitRail();
    host.setAttribute('kind', kind);
    host.setAttribute('network', rail.test.network);
    let error = null;
    host.addEventListener('allowit-error', (event) => (error = event.detail));
    host.isConnected = true;
    host.connectedCallback();
    assert.equal(host.children.length, 0);
    assert.equal(error?.code, 'unsupported');
  });
}

test('the four wallet kinds have stable ids, text and one list of supported params', () => {
  const kinds = sdk.walletKinds();
  assert.deepEqual(kinds.map((kind) => kind.id), ['native', 'simple', 'standard', 'custom']);
  assert.deepEqual(kinds.map((kind) => [kind.enforcement, kind.custody]), [
    ['offchain', 'keypair'], ['offchain', 'contract'], ['onchain', 'contract'], ['onchain', 'contract'],
  ]);
  for (const kind of kinds) {
    assert.ok(kind.title && /\.$/.test(kind.explanation), kind.id);
    assert.equal(sdk.walletKind(kind.id), kind);
    assert.equal(kind.supportedParams.length > 0, kind.id === 'standard');
    assert.equal(kind.paramsUrl !== null, kind.id === 'standard');
    assert.ok(Object.isFrozen(kind) && Object.isFrozen(kind.supportedParams));
  }
  const standard = sdk.walletKind('standard');
  assert.equal(standard.title, 'Standard');
  assert.match(standard.paramsUrl, /^https:\/\/github\.com\/allowit-hq\/allowit-core\/.+#standard-wallet-parameters$/);
  assert.ok(standard.supportedParams.includes('payment_limit'));
  // The old selection value has no alias.
  for (const unknown of ['common', 'Standard', '', null, undefined]) assert.equal(sdk.walletKind(unknown), null);
});

test('amounts print exact decimals with the given unit and never a placeholder', () => {
  assert.equal(sdk.formatUnits('50000000', 7), '5');
  assert.equal(sdk.formatUnits(2500000n, 7), '0.25');
  assert.equal(sdk.formatUnits('-1', 7), '-0.0000001');
  assert.equal(sdk.formatUnits('0', 7), '0');
  assert.equal(sdk.formatUnits('-0', 2), '0');
  assert.equal(sdk.formatUnits('0007', 0), '7');
  assert.equal(sdk.formatUnits('340282366920938463463374607431768211455', 0), '340282366920938463463374607431768211455');
  assert.equal(sdk.formatUnits(Number.MAX_SAFE_INTEGER, 0), '9007199254740991');
  // A number beyond the safe range no longer holds the exact amount.
  for (const bad of ['1.5', 'x', '', '1e3', 1000000000000000100, 2 ** 53, 1.5, NaN, null, undefined, {}, [5]]) assert.throws(() => sdk.formatUnits(bad, 7), TypeError);
  assert.throws(() => sdk.formatAmount('unknown:none', {symbol: 'XYZ', decimals: 0, name: 'x', native: false}, 2 ** 60), TypeError);
  assert.throws(() => sdk.formatUnits('1', -1), TypeError);
  const display = {symbol: 'XYZ', decimals: 2, name: 'Example', native: false};
  assert.equal(sdk.formatAmount('unknown:none', display, '1250'), '12.5 XYZ');
  assert.throws(() => sdk.formatAmount('unknown:none', 'XYZ', '1'), /No unit is known/);
  assert.equal(sdk.nativeAsset('unknown:none'), null);
  assert.equal(sdk.assetDisplay('unknown:none', 'XYZ'), null);
  assert.deepEqual(sdk.walletKindsFor('unknown:none'), []);
  assert.equal(sdk.walletKindOf('unknown:none', {}), null);
  assert.throws(() => sdk.parseReviewLink('#unknown=abc'), /agent policy link is invalid/);
  assert.throws(() => sdk.parseReviewLink(null), /agent policy link is invalid/);
});

test('the owner workflow rejects an unknown network', async () => {
  await assert.rejects(sdk.installAndFund('unknown:none', {}), /No rail is registered/);
});

for (const rail of sdk.rails()) {
  const {network, fixtures} = rail.test;

  test(`${rail.id}: wallet kinds and the bound kind`, () => {
    const offered = sdk.walletKindsFor(network);
    assert.ok(offered.length > 0);
    for (const kind of offered) assert.equal(sdk.walletKind(kind.id), kind);
    assert.deepEqual(offered.map((kind) => kind.id), rail.walletKinds(network).filter((id) => sdk.walletKind(id)));
    assert.equal(sdk.walletKindOf(network, {}), null);
    assert.equal(sdk.walletKindOf(network, null), null);
  });

  test(`${rail.id}: every published asset has a unit`, () => {
    const native = sdk.nativeAsset(network);
    assert.ok(native.native && native.symbol && native.name);
    assert.equal(sdk.formatAmount(network, native, 5n * 10n ** BigInt(native.decimals)), `5 ${native.symbol}`);
    for (const asset of sdk.assets(network)) {
      const display = sdk.assetDisplay(network, asset.id);
      assert.equal(display.symbol, asset.symbol);
      assert.equal(display.decimals, asset.decimals);
      const text = sdk.formatAmount(network, asset.id, '1');
      assert.ok(text.endsWith(` ${asset.symbol}`) && !text.includes('UNIT'), text);
    }
    assert.equal(sdk.assetDisplay(network, fixtures.invalidAddress), null);
    assert.throws(() => sdk.formatAmount(network, fixtures.invalidAddress, '1'), /No unit is known/);
  });

  test(`${rail.id}: review links and the owner workflow dispatch to the rail`, async () => {
    const proposal = {network, revision: 1, title: 'Pay', explanation: '', code: 'fn policy() {}',
      sourceHash: 'ab'.repeat(32), paramsHash: 'cd'.repeat(32), params: {}};
    const encode = (envelope) => rail.review.fragment + Buffer.from(JSON.stringify(envelope)).toString('base64url');
    const link = sdk.parseReviewLink(encode({proposal, workPrompt: 'Do the work.', agent: fixtures.address, amount: '5'}));
    assert.deepEqual(link, {proposal, workPrompt: 'Do the work.', agent: fixtures.address, amount: '5', walletKind: null});
    assert.deepEqual(sdk.parseReviewLink(encode({proposal, workPrompt: 'w'})).agent, null);
    // The link names the chosen kind beside the proposal. The proposal keeps its exact keys.
    for (const kind of sdk.walletKindsFor(network)) {
      const chosen = sdk.parseReviewLink(encode({proposal, workPrompt: 'w', walletKind: kind.id}));
      assert.deepEqual(chosen, {proposal, workPrompt: 'w', agent: null, amount: null, walletKind: kind.id});
      assert.throws(() => sdk.parseReviewLink(encode({proposal: {...proposal, walletKind: kind.id}, workPrompt: 'w'})), /invalid/);
    }
    assert.equal(sdk.parseReviewLink(encode({proposal, workPrompt: 'w', walletKind: null})).walletKind, null);
    for (const bad of [1, {}, ['native']]) {
      assert.throws(() => sdk.parseReviewLink(encode({proposal, workPrompt: 'w', walletKind: bad})), /invalid/);
    }
    assert.throws(() => sdk.parseReviewLink(encode({proposal: {...proposal, network: 'unknown:none'}, workPrompt: 'w'})), /invalid/);
    assert.throws(() => sdk.parseReviewLink(encode({proposal, workPrompt: 'w', extra: 1})), /invalid/);
    assert.match(sdk.reviewAgentPrompt(link, 'https://example.test', 'policy-1', 'wallet-1', fixtures.address), /^Do the work\./);
    assert.deepEqual(sdk.reviewParamLines(network, {}), []);

    // Dispatch only: the rail's workflow gets the host's transport, and a transport failure signs nothing.
    // Each rail tests its own owner protocol in its repository.
    const offline = new Error('offline'), journal = new Map(), signed = [];
    const input = {
      ask: async () => { throw offline; }, sign: async (step) => (signed.push(step), 'signed'),
      journal: {get: (k) => journal.get(k) ?? null, set: (k, v) => void journal.set(k, v), remove: (k) => void journal.delete(k)},
      key: 'k', policyId: 'p', agent: fixtures.address, amount: '5', onStep() {},
    };
    await assert.rejects(sdk.installAndFund(network, input), (error) => error === offline);
    for (const alias of rail.networks.find((n) => n.id === network).aliases) {
      await assert.rejects(sdk.installAndFund(alias, input), (error) => error === offline);
    }
    assert.deepEqual([signed.length, journal.size], [0, 0]);
  });
}

test('server documents give their wallet kind, settlement, charged fee and limits', async () => {
  const sdk = await import('../src/registry.js');
  for (const rail of sdk.rails()) {
    const network = rail.test.network;
    const asset = sdk.nativeAsset(network);
    const quantity = (units) => ({units, asset, text: sdk.formatAmount(network, asset, units)});
    const limits = [{param: 'daily_spending_limit', limit: quantity('500'), spent: quantity('120'), remaining: quantity('380')}];
    const document = {
      walletKind: 'standard',
      settlement: {height: 42, fee: quantity('7'), amount: null},
      feeCharged: quantity('7'),
      limits,
    };
    assert.equal(sdk.walletKindIdOf(document), 'standard');
    assert.equal(sdk.settlementOf(document).fee.text, quantity('7').text);
    assert.deepEqual(sdk.feeChargedOf(document), quantity('7'));
    assert.deepEqual(sdk.limitsOf(document), limits);
    assert.deepEqual(sdk.limitsOf({limits: []}), []);
  }
  // An absent or malformed value is null; it never reads as zero or as an empty report.
  for (const document of [null, 'text', [], {}, {walletKind: 'common', settlement: {height: 1}, feeCharged: '7', limits: [{param: 'p'}]}]) {
    assert.equal(sdk.walletKindIdOf(document), null);
    assert.equal(sdk.settlementOf(document), null);
    assert.equal(sdk.feeChargedOf(document), null);
    assert.equal(sdk.limitsOf(document), null);
  }
  const unit = {symbol: 'XYZ', decimals: 2, name: 'Example', native: false};
  assert.equal(sdk.feeChargedOf({feeCharged: {units: '1.5', asset: unit, text: '1.5 XYZ'}}), null);
  assert.equal(sdk.limitsOf({limits: [{param: '', limit: {}, spent: {}, remaining: {}}]}), null);
});

test('a wallet-kind refusal keeps its code and details', async () => {
  const sdk = await import('../src/registry.js');
  const details = {walletKind: 'custom', network: 'example:testnet', reason: 'No template is configured'};
  assert.deepEqual(
    sdk.walletKindRefusal({code: 'wallet_kind_unsupported', error: 'Refused', details}),
    {code: 'wallet_kind_unsupported', ...details},
  );
  const failure = Object.assign(new Error('Refused'), {code: 'wallet_kind_unknown', details: {walletKind: 'common'}});
  assert.deepEqual(sdk.walletKindRefusal(failure), {code: 'wallet_kind_unknown', walletKind: 'common', network: null, reason: null});
  for (const other of [null, 'text', {}, {code: 'network_unavailable', details}, new Error('x')]) {
    assert.equal(sdk.walletKindRefusal(other), null);
  }
});
