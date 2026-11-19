// The owner workflows with fake rail hooks: the journal, the recovery of a saved signed step, the keypair wallet's setup
// and the owner's approval. Each rail tests its own hooks against its network in its repository.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import test from 'node:test';
import {installAndFund as install, ownerArchiveKey, OwnerFlowError, updateParams as update} from '../src/owner-workflow.js';

const network = 'test:net';
const policyId = 'policy-1', key = 'flow:owner:1', agent = 'AGENT', amount = '5';
const hash = n => String(n).repeat(64).slice(0, 64);
const sha = text => createHash('sha256').update(text).digest('hex');
const step = (action, n) => ({action, payload: `unsigned-${action}-${n}`, transactionHash: hash(n), expiresAt: Math.floor(Date.now() / 1000) + 300});
const savedStep = (action, n) => ({action, payload: `unsigned-${action}-${n}`, hash: hash(n), expiresAt: Math.floor(Date.now() / 1000) - 60});
const refusal = (code, details, status = 409) => Object.assign(new Error(code), {status, code, details});

/**
 * The rail's hooks. History holds every asked transaction as successful in ledger 7 unless `history` answers it; an
 * expired step cannot be checked unless `expired` answers it.
 */
function chainOf({history, expired, keypair, approval} = {}) {
  return {
    transaction: async (net, txHash) => {
      assert.equal(net, network);
      return history ? history(txHash) : {hash: txHash, ledger: 7, successful: true};
    },
    expiredRecord: async (net, txHash, signedPayload) => {
      if (!expired) throw new Error('unchecked');
      return expired(txHash, signedPayload);
    },
    ...(keypair ? {keypairSetup: keypair} : {}),
    ...(approval ? {approval} : {}),
  };
}
const installAndFund = (input, chain = chainOf()) => install(chain, {network, historyPauseMs: 0, ...input});
const updateParams = (input, chain = chainOf()) => update(chain, {network, historyPauseMs: 0, ...input});

/** A service that installs after one `create_wallet` step, which funds the wallet. It prepares a `fund` step when asked. */
function service({submit}) {
  const calls = [], state = {installed: false, prepared: 0};
  const ask = async (path, body) => {
    calls.push([path, body]);
    if (path === 'native/deploy/prepare')
      return state.installed ? {policyId, installed: true, installation: {wallet: 'CWALLET'}} : {policyId, wallet: 'CWALLET', step: step('create_wallet', ++state.prepared)};
    if (path === 'native/owner/prepare') return {policyId, step: step('fund', ++state.prepared)};
    if (path === 'native/owner/submit') {
      const reply = await submit(body, state);
      if (reply.action === 'create_wallet') state.installed = true;
      return reply;
    }
    throw new Error(path);
  };
  return {ask, calls, state};
}
function journalOf(entries = {}) {
  const map = new Map(Object.entries(entries));
  return {map, get: k => map.get(k) ?? null, set: (k, v) => void map.set(k, v), remove: k => void map.delete(k)};
}
const confirmed = body => {
  const [, action, n] = body.payload.split('-');
  return {policyId, action, status: 'confirmed', transactionHash: hash(n), ledger: 7};
};
const signer = signed => async s => (signed.push(s.action), `signed-${s.action}-${s.hash[0]}`);

test('the happy path installs and funds with one signature and never signs a confirmed funding again', async () => {
  const signed = [], journal = journalOf();
  const {ask, calls} = service({submit: confirmed});
  const input = {ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}};
  const result = await installAndFund(input);
  assert.equal(result.wallet, 'CWALLET');
  // The creation moves the amount into the wallet: no funding step follows.
  assert.deepEqual(result.receipts.map(r => r.action), ['create_wallet']);
  assert.deepEqual(signed, ['create_wallet']);
  assert.equal(journal.get(key), null);
  assert.deepEqual(JSON.parse(journal.get(`${key}:funded`)), {policyId, agent, amount});
  assert.deepEqual(calls.find(([path]) => path === 'native/deploy/prepare')[1], {policyId, agent, amount});
  assert.equal(calls.some(([path]) => path === 'native/owner/prepare'), false);
  const again = await installAndFund(input);
  assert.deepEqual([again.wallet, again.receipts.length, signed.length], ['CWALLET', 0, 1]);
});

test('a creation that an earlier version saved funded nothing: its funding still follows', async () => {
  // An earlier version created the wallet in one step and funded it in another. Its saved creation carries no version.
  for (const [version, funds] of [[undefined, true], [2, false]]) {
    const saved = {step: savedStep('create_wallet', 5), signedPayload: 'signed-create_wallet-5', policyId, agent, amount, ...(version ? {version} : {})};
    const signed = [], journal = journalOf({[key]: JSON.stringify(saved)});
    const {ask, calls} = service({submit: body => {
      if (body.payload === saved.signedPayload) throw refusal('owner_transaction_settled', {policyId, action: 'create_wallet', transactionHash: hash(5), ledger: 7});
      return confirmed(body);
    }});
    const result = await installAndFund({ask: async (path, body) => {
      if (path === 'native/deploy/prepare') return (calls.push([path, body]), {policyId, installed: true, installation: {wallet: 'CWALLET'}});
      return ask(path, body);
    }, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}});
    assert.deepEqual(result.receipts.map(r => r.action), funds ? ['create_wallet', 'fund'] : ['create_wallet'], String(version));
    assert.deepEqual(signed, funds ? ['fund'] : [], String(version));
  }
});

test('a wallet installed without funding is funded after its installation', async () => {
  const signed = [], journal = journalOf();
  const {ask, state} = service({submit: confirmed});
  state.installed = true;
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}});
  assert.deepEqual(result.receipts.map(r => r.action), ['fund']);
  assert.deepEqual(signed, ['fund']);
  assert.deepEqual(JSON.parse(journal.get(`${key}:funded`)), {policyId, agent, amount});
});

/** A saved step past its time bound, and the service's expiry reply for it. */
function expiredStep(action) {
  const lost = {step: savedStep(action, 9), signedPayload: `signed-${action}-9`};
  const refuse = () => { throw refusal('owner_transaction_expired', {policyId, action, transactionHash: hash(9)}); };
  return {lost, refuse};
}

test('a lost send that expired and never applied is archived, and the owner signs a new step', async () => {
  const {lost, refuse} = expiredStep('create_wallet');
  const asked = [];
  const chain = chainOf({expired: async (txHash, signedPayload) => (asked.push([txHash, signedPayload]), null)});
  const signed = [], journal = journalOf({[key]: JSON.stringify(lost)});
  const {ask} = service({submit: body => body.payload === lost.signedPayload ? refuse() : confirmed(body)});
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, chain);
  assert.deepEqual(signed, ['create_wallet']);
  assert.deepEqual(result.receipts.map(r => r.transactionHash), [hash(1)]);
  assert.deepEqual(JSON.parse(journal.get(ownerArchiveKey(key, hash(9)))), lost);
  assert.equal(journal.get(key), null);
  // The rail reads the record of exactly the saved signed bytes.
  assert.deepEqual(asked, [[hash(9), lost.signedPayload]]);
});

test('an expired funding that the network shows applied is done and is never funded again', async () => {
  const {lost, refuse} = expiredStep('fund');
  const signed = [], journal = journalOf({[key]: JSON.stringify(lost)});
  const {ask, state, calls} = service({submit: refuse});
  state.installed = true;
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, chainOf({expired: async () => ({ledger: 77})}));
  assert.deepEqual(signed, []);
  assert.deepEqual(result.receipts, [{policyId, action: 'fund', status: 'confirmed', transactionHash: hash(9), ledger: 77}]);
  assert.deepEqual(JSON.parse(journal.get(`${key}:funded`)), {policyId, agent, amount});
  assert.equal(calls.some(([path]) => path === 'native/owner/prepare'), false);
});

test('an expiry reply without the network\'s proof keeps the saved step and signs nothing', async () => {
  const {lost, refuse} = expiredStep('fund');
  // The rail cannot check the record; or the step's own time bound has not passed, so nothing is read at all.
  const live = {...lost, step: {...lost.step, expiresAt: Math.floor(Date.now() / 1000) + 300}};
  let reads = 0;
  for (const [saved, chain] of [[lost, chainOf()], [live, chainOf({expired: async () => (reads++, null)})]]) {
    const signed = [], journal = journalOf({[key]: JSON.stringify(saved)});
    const {ask, state} = service({submit: refuse});
    state.installed = true;
    await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, chain),
      error => error instanceof OwnerFlowError && error.code === 'owner_transaction_expired' && error.retry === true);
    assert.deepEqual(signed, []);
    assert.deepEqual(JSON.parse(journal.get(key)), saved);
    assert.equal(journal.get(ownerArchiveKey(key, hash(9))), null);
    assert.equal(journal.get(`${key}:funded`), null);
  }
  assert.equal(reads, 0);
});

test('one workflow runs per journal key, so a second call never signs', async () => {
  const signed = [], journal = journalOf();
  const {ask} = service({submit: confirmed});
  const input = {ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}};
  const [first, second] = await Promise.allSettled([installAndFund(input), installAndFund(input)]);
  assert.equal(first.status, 'fulfilled');
  assert.ok(second.reason instanceof OwnerFlowError && second.reason.retry && /already running/.test(second.reason.message));
  assert.deepEqual(signed, ['create_wallet']);
  // A params update under the same key waits for nothing and signs nothing either.
  const [setup, change] = await Promise.allSettled([installAndFund({...input, journal: journalOf()}),
    updateParams({ask, sign: signer(signed), journal: journalOf(), key, policyId, params: {daily_spending_limit: {Int: '1'}}, onStep() {}})]);
  assert.equal(setup.status, 'fulfilled');
  assert.match(change.reason.message, /already running/);
});

test('a saved step that already settled is done without a second signature', async () => {
  const saved = {step: savedStep('fund', 5), signedPayload: 'signed-fund-5'};
  const signed = [], journal = journalOf({[key]: JSON.stringify(saved)});
  const {ask, state} = service({submit: () => { throw refusal('owner_transaction_settled', {policyId, action: 'fund', transactionHash: hash(5), ledger: 7}); }});
  state.installed = true;
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}});
  assert.deepEqual(signed, []);
  assert.deepEqual(result.receipts, [{policyId, action: 'fund', status: 'confirmed', transactionHash: hash(5), ledger: 7}]);
  assert.deepEqual(JSON.parse(journal.get(`${key}:funded`)), {policyId, agent, amount});
});

test('pending, unavailable, foreign and untyped replies keep the saved step and sign nothing', async () => {
  const saved = {step: savedStep('fund', 5), signedPayload: 'signed-fund-5'};
  const cases = [
    [refusal('owner_transaction_pending', {policyId, action: 'fund', transactionHash: hash(5)}), 'owner_transaction_pending', true],
    [refusal('network_unavailable', {retry: 'same_request_id'}, 503), 'network_unavailable', true],
    // A network error without the retry promise is not a safe retry signal.
    [refusal('network_unavailable', {}, 503), 'network_unavailable', false],
    // An expiry or settlement that names another transaction settles nothing.
    [refusal('owner_transaction_expired', {policyId, action: 'fund', transactionHash: hash(6)}), 'owner_transaction_expired', false],
    [refusal('owner_transaction_settled', {policyId: 'other', transactionHash: hash(5)}), 'owner_transaction_settled', false],
    [new Error('socket closed'), null, false],
  ];
  for (const [thrown, code, retry] of cases) {
    const signed = [], journal = journalOf({[key]: JSON.stringify(saved)});
    const {ask} = service({submit: () => { throw thrown; }});
    await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}),
      error => error instanceof OwnerFlowError && error.code === code && error.retry === retry);
    assert.deepEqual(signed, []);
    assert.deepEqual(JSON.parse(journal.get(key)), saved);
    assert.equal(journal.get(`${key}:funded`), null);
  }
});

test('signed bytes enter the journal before the send, and a wrong receipt or reply is refused', async () => {
  const signed = [], journal = journalOf();
  const {ask} = service({submit: body => ({...confirmed(body), transactionHash: hash(8)})});
  await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}), /did not confirm/);
  assert.deepEqual(JSON.parse(journal.get(key)), {step: {action: 'create_wallet', payload: 'unsigned-create_wallet-1', hash: hash(1), expiresAt: JSON.parse(journal.get(key)).step.expiresAt}, signedPayload: 'signed-create_wallet-1', policyId, agent, amount, version: 2});
  const other = service({submit: confirmed});
  await assert.rejects(installAndFund({ask: async (path, body) => ({...await other.ask(path, body), policyId: 'other'}), sign: signer([]), journal: journalOf(), key, policyId, agent, amount, onStep() {}}), /different policy/);
  await assert.rejects(installAndFund({ask, sign: signer([]), journal: journalOf({[key]: '{bad'}), key, policyId, agent, amount, onStep() {}}), /unreadable/);
  // An empty signature, or a step that expired before it was signed, is never saved or sent.
  for (const [sign, prepared] of [[async () => '', step('create_wallet', 1)], [signer([]), {...step('create_wallet', 1), expiresAt: 1}]]) {
    const fresh = journalOf(), calls = [];
    const ask = async (path, body) => (calls.push(path), path === 'native/deploy/prepare' ? {policyId, wallet: 'CWALLET', step: prepared} : confirmed(body));
    await assert.rejects(installAndFund({ask, sign, journal: fresh, key, policyId, agent, amount, onStep() {}}), OwnerFlowError);
    assert.deepEqual([fresh.map.size, calls.includes('native/owner/submit')], [0, false]);
  }
});

test('a resume with another amount, agent or policy is refused before anything is sent', async () => {
  const pending = {step: savedStep('fund', 5), signedPayload: 'signed-fund-5', policyId, agent, amount};
  for (const [entries, changed] of [
    [{[key]: JSON.stringify(pending)}, {amount: '10'}],
    [{[key]: JSON.stringify(pending)}, {agent: 'OTHER'}],
    [{[key]: JSON.stringify({...pending, amount: undefined})}, {}],
    [{[`${key}:funded`]: JSON.stringify({policyId, agent, amount})}, {amount: '10'}],
    [{[`${key}:funded`]: '{bad'}, {}],
  ]) {
    const signed = [], journal = journalOf(entries), before = new Map(journal.map);
    const {ask, calls} = service({submit: confirmed});
    await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}, ...changed}),
      error => error instanceof OwnerFlowError && /original wallet setup/.test(error.message));
    assert.deepEqual([signed, calls], [[], []]);
    assert.deepEqual(journal.map, before);
  }
  // A step and a marker saved without inputs predate them; the same inputs resume them.
  const journal = journalOf({[key]: JSON.stringify({step: savedStep('fund', 5), signedPayload: 'signed-fund-5'}), [`${key}:funded`]: 'confirmed'});
  const {ask, state} = service({submit: confirmed});
  state.installed = true;
  const result = await installAndFund({ask, sign: signer([]), journal, key, policyId, agent, amount, onStep() {}});
  assert.deepEqual(result.receipts.map(r => r.action), ['fund']);
});

test('a failed transaction is replaced only when the network holds that exact failure', async () => {
  const pending = {step: savedStep('create_wallet', 5), signedPayload: 'signed-create_wallet-5', policyId, agent, amount};
  const failed = body => {
    if (body.payload === pending.signedPayload) throw refusal('owner_transaction_failed', {policyId, action: 'create_wallet', transactionHash: hash(5), ledger: 9});
    return confirmed(body);
  };
  const chain = record => chainOf({history: txHash => {
    if (txHash !== hash(5)) return {hash: txHash, ledger: 7, successful: true};
    if (record instanceof Error) throw record;
    return record;
  }});
  for (const unproven of [new Error('offline'), null, 'missing', {hash: hash(5), ledger: 9, successful: true}, {hash: hash(5), ledger: 8, successful: false}, {hash: hash(6), ledger: 9, successful: false}]) {
    const signed = [], journal = journalOf({[key]: JSON.stringify(pending)});
    const {ask} = service({submit: failed});
    await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, chain(unproven)),
      error => error instanceof OwnerFlowError && error.code === 'owner_transaction_failed' && error.retry === false);
    assert.deepEqual(signed, []);
    assert.deepEqual(JSON.parse(journal.get(key)), pending);
  }
  const signed = [], journal = journalOf({[key]: JSON.stringify(pending)});
  const {ask} = service({submit: failed});
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, chain({hash: hash(5), ledger: 9, successful: false}));
  assert.deepEqual(signed, ['create_wallet']);
  assert.equal(result.wallet, 'CWALLET');
  assert.deepEqual(JSON.parse(journal.get(ownerArchiveKey(key, hash(5)))), {step: pending.step, signedPayload: pending.signedPayload});
});

test('a confirmation is trusted only when history holds that successful transaction', async () => {
  const history = replies => { let n = 0; return chainOf({history: txHash => {
    const reply = replies[Math.min(n++, replies.length - 1)];
    if (reply instanceof Error) throw reply;
    return reply === undefined ? {hash: txHash, ledger: 7, successful: true} : reply;
  }}); };
  for (const unproven of [[new Error('offline')], ['missing'], [null], [{hash: hash(1), ledger: 7, successful: false}], [{hash: hash(1), ledger: 8, successful: true}], [{hash: hash(2), ledger: 7, successful: true}], [{hash: hash(1), ledger: 0, successful: true}]]) {
    const signed = [], journal = journalOf();
    const {ask} = service({submit: confirmed});
    await assert.rejects(installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, history(unproven)),
      error => error instanceof OwnerFlowError && error.retry === true && /could not be checked/.test(error.message));
    // The signed bytes stay saved and nothing is marked as funded.
    assert.deepEqual(signed, ['create_wallet']);
    assert.equal(JSON.parse(journal.get(key)).signedPayload, 'signed-create_wallet-1');
    assert.equal(journal.get(`${key}:funded`), null);
  }
  // History that trails the network by one read still settles the step.
  const signed = [], journal = journalOf();
  const {ask} = service({submit: confirmed});
  const result = await installAndFund({ask, sign: signer(signed), journal, key, policyId, agent, amount, onStep() {}}, history(['missing', undefined]));
  assert.deepEqual(result.receipts.map(r => [r.action, r.ledger]), [['create_wallet', 7]]);
  // A settled reply that history does not confirm keeps the saved funding and records nothing.
  const pending = {step: savedStep('fund', 5), signedPayload: 'signed-fund-5', policyId, agent, amount};
  const kept = journalOf({[key]: JSON.stringify(pending)});
  const settled = service({submit: () => { throw refusal('owner_transaction_settled', {policyId, action: 'fund', transactionHash: hash(5), ledger: 7}); }});
  settled.state.installed = true;
  await assert.rejects(installAndFund({ask: settled.ask, sign: signer([]), journal: kept, key, policyId, agent, amount, onStep() {}}, history([{hash: hash(5), ledger: 7, successful: false}])), error => error.retry === true);
  assert.deepEqual(JSON.parse(kept.get(key)), pending);
  assert.equal(kept.get(`${key}:funded`), null);
});

const params = {daily_spending_limit: {Int: '50000000'}};
/** A service that prepares one `update_params` step for each asked change. */
function updates({submit, prepare}) {
  const calls = [], state = {prepared: 0};
  const ask = async (path, body) => {
    calls.push([path, body]);
    if (path === 'native/owner/prepare') return prepare ? prepare(body, state) : {policyId, step: step('update_params', ++state.prepared)};
    if (path === 'native/owner/submit') return submit(body, state);
    throw new Error(path);
  };
  return {ask, calls, state};
}

test('a params update signs one step, sends the typed values and is never signed again under its key', async () => {
  const signed = [], journal = journalOf();
  const {ask, calls} = updates({submit: confirmed});
  const input = {ask, sign: signer(signed), journal, key, policyId, params, expectedRevision: 3, onStep() {}};
  const result = await updateParams(input);
  assert.deepEqual([result.done, result.receipts.map(r => r.action)], [true, ['update_params']]);
  assert.deepEqual(signed, ['update_params']);
  assert.deepEqual(calls[0], ['native/owner/prepare', {policyId, action: 'update_params', params, expectedRevision: 3}]);
  assert.equal(journal.get(key), null);
  // The same change under the same key is done: nothing is prepared or signed.
  const again = await updateParams(input);
  assert.deepEqual([again.done, again.receipts.length, signed.length, calls.length], [true, 0, 1, 2]);
  // Another change under that key is refused before anything is sent.
  await assert.rejects(updateParams({...input, params: {daily_spending_limit: {Int: '1'}}}), error => error instanceof OwnerFlowError && !error.retry);
  assert.equal(calls.length, 2);
});

test('a resumed params update settles the saved signed step first and asks for no second signature', async () => {
  const signed = [], journal = journalOf();
  let lost = true;
  const {ask, calls} = updates({submit: body => {
    if (lost) throw refusal('owner_transaction_pending', {});
    return confirmed(body);
  }});
  const input = {ask, sign: signer(signed), journal, key, policyId, params, onStep() {}};
  await assert.rejects(updateParams(input), error => error.code === 'owner_transaction_pending' && error.retry === true);
  const saved = JSON.parse(journal.get(key));
  assert.equal(saved.signedPayload, 'signed-update_params-1');
  lost = false;
  const result = await updateParams(input);
  assert.deepEqual([result.done, result.receipts.length, signed.length], [true, 1, 1]);
  // The resume sent the saved bytes and prepared nothing new.
  assert.deepEqual(calls.filter(([path]) => path === 'native/owner/prepare').length, 1);
  assert.deepEqual(calls.at(-1), ['native/owner/submit', {policyId, payload: 'signed-update_params-1'}]);
  // A saved wallet setup step under this key belongs to other inputs and is refused.
  const other = journalOf({[key]: JSON.stringify({step: savedStep('fund', 4), signedPayload: 'signed-fund-4', policyId, agent, amount})});
  await assert.rejects(updateParams({...input, journal: other}), error => error instanceof OwnerFlowError && !error.retry);
});

test('a refused params update keeps the typed code of the service and signs nothing', async () => {
  for (const code of ['param_out_of_range', 'param_invalid', 'update_unavailable', 'revision_changed']) {
    const signed = [], journal = journalOf();
    const {ask} = updates({submit: confirmed, prepare: () => { throw refusal(code, {param: 'daily_spending_limit'}, 400); }});
    await assert.rejects(updateParams({ask, sign: signer(signed), journal, key, policyId, params, onStep() {}}),
      error => error instanceof OwnerFlowError && error.code === code && error.retry === false);
    assert.deepEqual([signed.length, journal.map.size], [0, 0]);
  }
  // A reply for another policy or another action is refused before the wallet opens.
  for (const reply of [{policyId: 'other', step: step('update_params', 1)}, {policyId, step: step('fund', 1)}, {policyId}]) {
    const signed = [];
    const {ask} = updates({submit: confirmed, prepare: () => reply});
    await assert.rejects(updateParams({ask, sign: signer(signed), journal: journalOf(), key, policyId, params, onStep() {}}), OwnerFlowError);
    assert.equal(signed.length, 0);
  }
  // No change, too many, or an invalid revision never reaches the service.
  const {ask, calls} = updates({submit: confirmed});
  await assert.rejects(updateParams({ask, sign: signer([]), journal: journalOf(), key, policyId, params: {}, onStep() {}}), error => error.code === 'param_invalid');
  const many = Object.fromEntries(Array.from({length: 33}, (_, i) => [`p${i}`, {Int: '1'}]));
  await assert.rejects(updateParams({ask, sign: signer([]), journal: journalOf(), key, policyId, params: many, onStep() {}}), error => error.code === 'param_invalid');
  await assert.rejects(updateParams({ask, sign: signer([]), journal: journalOf(), key, policyId, params, expectedRevision: -1, onStep() {}}), error => error.code === 'revision_changed');
  assert.equal(calls.length, 0);
});

// A keypair wallet: the rail sets up its account with one owner signature, and the owner approves its policy with one
// message signature. The fake rail reads an approval message as JSON and checks a signature as a digest of the signer.
const OWNER = 'OWNER', SERVER = 'SERVER';
const signatureOf = (account, digest) => Array.from(createHash('sha512').update(`${account}:${digest}`).digest());
const source = 'use allowit::prelude::*;\n';
const approvalMessage = (extra = {}) => JSON.stringify({policyId, revision: 2, wallet: 'WALLET-1', source, rules: ['payment_limit'], params: {payment_limit: {Int: '5'}}, ...extra});
const reviewed = () => ({source, rules: ['payment_limit'], params: {payment_limit: {Int: '5'}}});
const installed = wallet => ({wallet, source, rules: ['payment_limit'], params: {payment_limit: {Int: '1'}}});

/** The fake rail of keypair wallets, with the accounts that its setups created and the owner of each. */
function keypairRail({owners} = {}) {
  const world = {created: [], owners: owners ?? new Map(), reads: [], sent: [], setups: new Set()};
  // The network of the fake rail: the setups it holds by hash, and how it answers the next send.
  world.held = new Map();
  world.send = 'ok';
  world.prepared = 0;
  const keypair = {
    prepare: async ({network: net, owner, agent: forAgent, server, amount: funding}) => {
      assert.equal(net, network);
      if (!/^[0-9]+(\.[0-9]{1,7})?$/.test(funding)) throw new Error('Enter an amount with at most seven decimals.');
      const wallet = `WALLET-${++world.prepared}`;
      const payload = `setup|${wallet}|${owner}|${forAgent}|${server}|${funding}`, txHash = sha(payload);
      world.setups.add(txHash);
      return {
        wallet,
        step: {action: 'set_up_wallet', payload, hash: txHash, expiresAt: Math.floor(Date.now() / 1000) + 300},
        // The new account's own signature joins the owner's.
        complete: ownerSigned => {
          if (ownerSigned !== `signed:${payload}`) throw new Error('The wallet signed with another account than the owner.');
          return `${ownerSigned}|countersigned|${world.validUntil ?? Math.floor(Date.now() / 1000) + 300}`;
        },
      };
    },
    read: (signedPayload, net) => {
      const [head, wallet, owner, forAgent, server, , mark, validUntil] = signedPayload.split('|');
      if (head !== 'signed:setup' || mark !== 'countersigned' || net !== network) return null;
      return {hash: sha(signedPayload.slice('signed:'.length, signedPayload.indexOf('|countersigned|'))), owner, wallet, agent: forAgent, server, validUntil: Number(validUntil)};
    },
    send: async (net, signedPayload, txHash) => {
      world.sent.push(signedPayload);
      if (world.send === 'lost') return null;
      const [, wallet, owner] = signedPayload.split('|');
      world.held.set(txHash, {hash: txHash, ledger: 8, successful: world.send !== 'failed'});
      if (world.send === 'failed') return null;
      if (!world.created.includes(wallet)) world.created.push(wallet);
      world.owners.set(wallet, owner);
      return 8;
    },
    configuration: (net, wallet, owner, forAgent, server) => ({wallet, owner, agent: forAgent, server}),
    // This rail sets up wallets of the network's own asset only.
    checkAsset: (signedPayload, net, asset) => {
      world.checked = (world.checked ?? 0) + 1;
      if (!keypair.read(signedPayload, net) || (asset !== undefined && asset !== null))
        throw Object.assign(new Error('The signed wallet setup is for another asset.'), {code: 'asset_mismatch'});
    },
  };
  const canonical = value => JSON.stringify(value, (_, v) => v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort()) : v);
  const approval = {
    read: (message, net) => {
      if (net !== network || typeof message !== 'string') return null;
      try { return JSON.parse(message); } catch { return null; }
    },
    holds: (fields, wallet, policy) => fields.wallet === wallet && fields.source === policy.source && canonical(fields.params) === canonical(policy.params)
      && (policy.rules === undefined || canonical(fields.rules) === canonical(policy.rules)),
    step: message => ({action: 'approve_policy', payload: Buffer.from(message).toString('base64'), hash: sha(message), expiresAt: Math.floor(Date.now() / 1000) + 300}),
    walletOwner: async (net, wallet) => (world.reads.push(wallet), world.owners.get(wallet) ?? null),
    signedBy: (account, digest, signature) => JSON.stringify(signatureOf(account, digest)) === JSON.stringify(signature),
  };
  // History holds what the fake network took; a setup that it never took is missing. Other steps settle in ledger 7.
  const history = txHash => world.held.get(txHash) ?? (world.setups.has(txHash) ? 'missing' : {hash: txHash, ledger: 7, successful: true});
  return {world, chain: chainOf({keypair, approval, history})};
}
/** The owner's wallet: it signs a setup and an approval, and records each step. */
const ownerSigns = (signed, account = OWNER) => async s => (signed.push(s.action),
  s.action === 'approve_policy' ? Buffer.from(signatureOf(account, s.hash)).toString('base64') : `signed:${s.payload}`);

/** A service for one keypair policy: the setup requirement, the registration, the approval and its confirmation. */
function keypairService({registerFails = 0, message} = {}) {
  const calls = [], state = {wallet: null, approved: false, registerFails};
  const ask = async (path, body) => {
    calls.push([path, body]);
    if (path === 'native/deploy/prepare') {
      if (!state.wallet) throw refusal('wallet_setup_required', {walletKind: 'native', policyId, network, owner: OWNER, agent, server: SERVER});
      return {policyId, installed: true, approved: state.approved, walletKind: 'native', wallet: state.wallet, installation: {wallet: state.wallet}};
    }
    if (path === 'native/register') {
      if (state.registerFails-- > 0) throw new Error('The service could not be reached.');
      state.wallet = body.configuration.wallet;
      return {registered: true, wallet: state.wallet, policyId: body.policyId};
    }
    if (path === 'native/approval') return {required: !state.approved, message: message ?? approvalMessage({wallet: state.wallet})};
    if (path === 'micro/allocation/confirm') {state.approved = true; return {active: true};}
    throw new Error(path);
  };
  return {ask, calls, state};
}

test('a keypair policy deploys with two owner signatures: the wallet setup and the approval of the policy', async () => {
  const {world, chain} = keypairRail(), signed = [], journal = journalOf();
  const {ask, calls, state} = keypairService();
  const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
  const result = await installAndFund(input, chain);
  assert.deepEqual(signed, ['set_up_wallet', 'approve_policy']);
  assert.equal(result.wallet, state.wallet);
  assert.deepEqual(result.receipts.map(r => r.action), ['set_up_wallet', 'approve_policy']);
  // One setup created and funded the account: no funding step and no server step.
  assert.deepEqual(world.created, [state.wallet]);
  assert.deepEqual(calls.map(([path]) => path), ['native/deploy/prepare', 'native/register', 'native/deploy/prepare', 'native/approval', 'micro/allocation/confirm']);
  assert.deepEqual(calls[1][1], {configuration: {wallet: state.wallet, owner: OWNER, agent, server: SERVER}, policyId});
  const confirm = calls[4][1];
  assert.deepEqual([confirm.id, confirm.revision, confirm.sourceApproval.length, confirm.sourceApproval[0].signer], [policyId, 2, 1, OWNER]);
  assert.deepEqual(confirm.sourceApproval[0].signature, signatureOf(OWNER, sha(approvalMessage({wallet: state.wallet}))));
  assert.equal(journal.get('flow:wallet-setup'), null);
  assert.deepEqual(JSON.parse(journal.get('flow:funded')), {policyId, agent, amount});
  // The deployed policy signs nothing again.
  const again = await installAndFund(input, chain);
  assert.deepEqual([again.wallet, again.receipts.length, signed.length, world.created.length], [state.wallet, 0, 2, 1]);
});

test('a registration that fails keeps the signed setup: the next run registers the same wallet and signs no second one', async () => {
  const {world, chain} = keypairRail(), signed = [], journal = journalOf();
  const {ask, state} = keypairService({registerFails: 1});
  const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
  await assert.rejects(installAndFund(input, chain), /could not be reached/);
  assert.ok(journal.get('flow:wallet-setup'));
  const result = await installAndFund(input, chain);
  assert.deepEqual([result.wallet, state.wallet, world.created.length], [world.created[0], world.created[0], 1]);
  assert.deepEqual(signed, ['set_up_wallet', 'approve_policy']);
  // Roles that name another agent or policy are refused before anything is signed.
  const {ask: other} = keypairService();
  const wrong = async (path, body) => {
    try {return await other(path, body);} catch (error) {
      if (error.code === 'wallet_setup_required') error.details.agent = 'OTHER';
      throw error;
    }
  };
  await assert.rejects(installAndFund({...input, ask: wrong, journal: journalOf(), key: 'other'}, chain), error => error instanceof OwnerFlowError && /named no roles/.test(error.message));
  assert.equal(signed.length, 2);
  // A setup that the rail cannot prepare, or that the owner's wallet signed wrongly, is refused and saves nothing.
  const refused = new Error('The owner account could not be read from the network.');
  for (const [failing, sign, expected] of [
    [{...chain, keypairSetup: {...chain.keypairSetup, prepare: async () => { throw refused; }}}, ownerSigns([]), /owner account could not be read/],
    [chain, async () => 'signed:another transaction', /another account than the owner/],
  ]) {
    const kept = journalOf();
    await assert.rejects(installAndFund({...input, sign, ask: keypairService().ask, journal: kept, key: 'k'}, failing),
      error => error instanceof OwnerFlowError && error.retry === false && expected.test(error.message));
    assert.equal(kept.map.size, 0);
  }
  // A network whose rail sets up no account refuses the setup.
  await assert.rejects(installAndFund({...input, ask: keypairService().ask, journal: journalOf(), key: 'k'}), /sets up no wallet account/);
});

test('a setup whose result is unknown stays saved and is settled before anything is signed again', async () => {
  const {world, chain} = keypairRail(), signed = [], journal = journalOf();
  const input = {ask: keypairService().ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
  world.send = 'lost';
  await assert.rejects(installAndFund(input, chain), error => error instanceof OwnerFlowError && error.retry === true && /did not confirm the wallet setup/.test(error.message));
  const saved = JSON.parse(journal.get('flow:wallet-setup'));
  assert.deepEqual([saved.owner, saved.agent, saved.server, saved.amount], [OWNER, agent, SERVER, amount]);
  // The network has no record yet and the time bound lasts: the same bytes go again, and nothing is signed.
  world.send = 'ok';
  const result = await installAndFund(input, chain);
  assert.deepEqual([result.wallet, world.sent.length, world.sent[0] === world.sent[1]], ['WALLET-1', 2, true]);
  assert.deepEqual(signed, ['set_up_wallet', 'approve_policy']);
  // Another agent key or amount never replaces a saved signed setup.
  const other = journalOf({'flow:wallet-setup': JSON.stringify(saved)});
  for (const changed of [{agent: 'OTHER-AGENT'}, {amount: '6'}]) {
    const service = keypairService();
    const ask = async (path, body) => {
      try { return await service.ask(path, body); } catch (error) { if (error.details) error.details.agent = body.agent; throw error; }
    };
    await assert.rejects(installAndFund({...input, ask, journal: other, ...changed}, chain), /another agent key or amount/);
  }
  assert.equal(JSON.parse(other.get('flow:wallet-setup')).signedPayload, saved.signedPayload);
  // Unreadable saved bytes are never replaced.
  await assert.rejects(installAndFund({...input, ask: keypairService().ask, journal: journalOf({'flow:wallet-setup': '{"hash":"x"}'})}, chain), /unreadable/);
  assert.equal(signed.length, 2);
});

test('a saved setup is replaced only when the network proves that it never applied', async () => {
  const lost = async (validUntil) => {
    const rail = keypairRail(), journal = journalOf();
    rail.world.send = 'lost';
    rail.world.validUntil = validUntil;
    const input = {ask: keypairService().ask, sign: ownerSigns([]), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
    await assert.rejects(installAndFund(input, rail.chain), /did not confirm the wallet setup/);
    rail.world.send = 'ok';
    return {...rail, journal, input, saved: JSON.parse(journal.get('flow:wallet-setup'))};
  };
  // The network holds the setup as failed: it created nothing, and a new setup with a new account is signed.
  const failed = await lost();
  failed.world.held.set(failed.saved.hash, {hash: failed.saved.hash, ledger: 9, successful: false});
  const replaced = await installAndFund({...failed.input, sign: ownerSigns([])}, failed.chain);
  assert.equal(replaced.wallet, 'WALLET-2');
  // Past its time bound, the rail decides: an applied setup is done, a setup that never applied is replaced, and an
  // unclear answer keeps it saved.
  const past = Math.floor(Date.now() / 1000) - 60;
  for (const [expired, wallet] of [[async () => ({ledger: 11}), 'WALLET-1'], [async () => null, 'WALLET-2'], [async () => { throw new Error('unchecked'); }, null]]) {
    const run = await lost(past);
    if (wallet === 'WALLET-1') run.world.created.push('WALLET-1'), run.world.owners.set('WALLET-1', OWNER);
    const chain = {...run.chain, expiredRecord: expired};
    if (wallet) assert.equal((await installAndFund(run.input, chain)).wallet, wallet);
    else {
      await assert.rejects(installAndFund(run.input, chain), error => error.retry === true && /did not confirm the wallet setup/.test(error.message));
      assert.equal(JSON.parse(run.journal.get('flow:wallet-setup')).hash, run.saved.hash);
    }
  }
  // An unclear record keeps the setup saved.
  const unclear = await lost();
  await assert.rejects(installAndFund(unclear.input, {...unclear.chain, transaction: async () => null}), /did not confirm the wallet setup/);
  await assert.rejects(installAndFund(unclear.input, {...unclear.chain, transaction: async () => ({hash: hash(3), ledger: 5, successful: true})}), /did not confirm the wallet setup/);
  await assert.rejects(installAndFund(unclear.input, {...unclear.chain, transaction: async () => ({hash: unclear.saved.hash, ledger: 5})}), /did not confirm the wallet setup/);
  assert.equal(JSON.parse(unclear.journal.get('flow:wallet-setup')).hash, unclear.saved.hash);
});

test('a registration whose reply is lost funds nothing again: the retained setup records its funding', async () => {
  const {world, chain} = keypairRail(), signed = [], journal = journalOf();
  const {ask: service, calls, state} = keypairService();
  let lose = true;
  const ask = async (path, body) => {
    const reply = await service(path, body);
    if (path === 'native/register' && lose) {lose = false; throw new Error('The reply was lost.');}
    return reply;
  };
  const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
  await assert.rejects(installAndFund(input, chain), /reply was lost/);
  assert.ok(journal.get('flow:wallet-setup'));
  const result = await installAndFund(input, chain);
  assert.equal(result.wallet, state.wallet);
  assert.deepEqual(signed, ['set_up_wallet', 'approve_policy']);
  assert.equal(world.created.length, 1);
  assert.ok(!calls.some(([path, body]) => path === 'native/owner/prepare' && body?.action === 'fund'));
  assert.deepEqual(JSON.parse(journal.get('flow:funded')), {policyId, agent, amount});
  assert.equal(journal.get('flow:wallet-setup'), null);
});

test('a retained setup of other inputs is refused and kept: a changed amount funds nothing twice', async () => {
  const {world, chain} = keypairRail(), signed = [], journal = journalOf();
  const {ask: service, calls} = keypairService();
  let lose = true;
  const ask = async (path, body) => {
    const reply = await service(path, body);
    if (path === 'native/register' && lose) {lose = false; throw new Error('The reply was lost.');}
    return reply;
  };
  const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}};
  await assert.rejects(installAndFund(input, chain), /reply was lost/);
  const retained = journal.get('flow:wallet-setup');
  await assert.rejects(installAndFund({...input, amount: '6'}, chain), error => error instanceof OwnerFlowError && /original wallet setup/.test(error.message));
  assert.equal(journal.get('flow:wallet-setup'), retained);
  assert.deepEqual([signed, world.created.length], [['set_up_wallet'], 1]);
  assert.ok(!calls.some(([path, body]) => path === 'native/owner/prepare' && body?.action === 'fund'));
});

test('an installed keypair wallet that this workflow did not fund is funded by the owner', async () => {
  const {chain} = keypairRail(), signed = [], calls = [];
  const ask = async (path, body) => {
    calls.push([path, body]);
    if (path === 'native/deploy/prepare') return {policyId, installed: true, approved: true, walletKind: 'native', wallet: 'WALLET-9', installation: {wallet: 'WALLET-9'}};
    if (path === 'native/owner/prepare') return {policyId, step: step('fund', 4)};
    if (path === 'native/owner/submit') return confirmed({payload: 'x-fund-4'});
    throw new Error(path);
  };
  const result = await installAndFund({ask, sign: ownerSigns(signed), journal: journalOf(), key: 'existing', policyId, agent, amount, onStep() {}}, chain);
  assert.equal(result.wallet, 'WALLET-9');
  assert.ok(calls.some(([path, body]) => path === 'native/owner/prepare' && body.action === 'fund' && body.amount === amount));
  assert.deepEqual(signed, ['fund']);
  assert.deepEqual(result.receipts.map(r => r.action), ['fund']);
});

test('a deployment signs the approval only of the reviewed policy for the new wallet account, by its owner', async () => {
  for (const [name, message, policy, expected] of [
    ['another source', approvalMessage({wallet: 'WALLET-1', source: 'other'}), reviewed(), /differs from the policy/],
    ['a raised limit', approvalMessage({wallet: 'WALLET-1', params: {payment_limit: {Int: '500'}}}), reviewed(), /differs from the policy/],
    ['another wallet', approvalMessage({wallet: 'WALLET-7'}), reviewed(), /differs from the policy/],
    ['another policy', approvalMessage({wallet: 'WALLET-1', policyId: 'policy-2'}), reviewed(), /differs from the policy/],
    ['an unreadable message', 'not an approval', reviewed(), /differs from the policy/],
    ['other rules', approvalMessage({wallet: 'WALLET-1'}), {...reviewed(), rules: []}, /differs from the policy/],
    ['nothing reviewed', approvalMessage({wallet: 'WALLET-1'}), undefined, /reviewed is missing/],
  ]) {
    const {chain} = keypairRail(), signed = [];
    const {ask} = keypairService({message});
    await assert.rejects(installAndFund({ask, sign: ownerSigns(signed), journal: journalOf(), key: 'flow', policyId, agent, amount, reviewed: policy, onStep() {}}, chain), expected, name);
    assert.deepEqual(signed, ['set_up_wallet'], name);
  }
  // Rules that the host does not state are left to the service.
  const {chain} = keypairRail(), signed = [];
  const {source: text, params: values} = reviewed();
  const result = await installAndFund({ask: keypairService().ask, sign: ownerSigns(signed), journal: journalOf(), key: 'r', policyId, agent, amount, reviewed: {source: text, params: values}, onStep() {}}, chain);
  assert.deepEqual([result.wallet, signed], ['WALLET-1', ['set_up_wallet', 'approve_policy']]);
  // An owner that the network cannot name, or a signature of another account, sends nothing.
  for (const [owners, sign, expected, retry] of [
    [new Map(), ownerSigns([]), /could not be read/, true],
    [undefined, ownerSigns([], 'STRANGER'), /another account than the owner/, false],
    [undefined, async () => '', /did not return a message signature/, false],
    [undefined, async () => 'not base64!', /did not return a message signature/, false],
    // A signature of another length reaches the rail's check, which refuses it.
    [undefined, async () => Buffer.from([1, 2, 3]).toString('base64'), /another account than the owner/, false],
  ]) {
    const {chain: other, world} = keypairRail({owners});
    const {ask, calls} = keypairService();
    // The setup records its own owner unless the test empties the record after it.
    const send = other.keypairSetup.send;
    other.keypairSetup.send = async (...args) => { const ledger = await send(...args); if (owners) world.owners.clear(); return ledger; };
    const signing = async s => s.action === 'set_up_wallet' ? `signed:${s.payload}` : sign(s);
    await assert.rejects(installAndFund({ask, sign: signing, journal: journalOf(), key: 'o', policyId, agent, amount, reviewed: reviewed(), onStep() {}}, other),
      error => error instanceof OwnerFlowError && expected.test(error.message) && error.retry === retry);
    assert.ok(!calls.some(([path]) => path === 'micro/allocation/confirm'));
  }
});

/** A service that prepares a keypair wallet's change as an approval message. */
function approvals(message, answers) {
  const submitted = [], state = {prepares: 0};
  const ask = async (path, body) => {
    if (path === 'native/owner/prepare') {state.prepares++; return {policyId, approval: {message: typeof message === 'function' ? message(state) : message}};}
    if (path === 'native/owner/submit') {submitted.push(body); return answers.shift()();}
    throw new Error(path);
  };
  return {ask, submitted, state};
}
const done = () => ({policyId, action: 'update_params', status: 'confirmed'});
const change = {payment_limit: {Int: '5'}};

/**
 * A fake rail whose wallets hold one asset: the setup takes it, the signed bytes name it, the installation document
 * states it and an approval holds only in it. A wallet without an asset holds the asset of the network itself.
 */
function assetRail() {
  const {world, chain} = keypairRail();
  world.assets = new Map();
  world.preparedAssets = [];
  const base = chain.keypairSetup, own = {symbol: 'NET', issuer: null};
  const keypairSetup = {
    ...base,
    prepare: async input => {
      world.preparedAssets.push(input.asset);
      const prepared = await base.prepare(input);
      world.assets.set(prepared.wallet, input.asset?.issuer ? input.asset : own);
      return prepared;
    },
    // The fake bytes state their funding in their sixth field, and the rail reads it back.
    read: (signedPayload, net) => {
      const read = base.read(signedPayload, net);
      return read ? {...read, asset: world.assets.get(read.wallet) ?? own, amount: signedPayload.split('|')[5]} : null;
    },
    configuration: (net, wallet, owner, forAgent, server, asset) => ({...base.configuration(net, wallet, owner, forAgent, server), asset: asset?.issuer ? asset : own}),
    checkAsset: (signedPayload, net, asset) => {
      const read = base.read(signedPayload, net), held = read ? world.assets.get(read.wallet) ?? own : null, expected = asset?.issuer ? asset : own;
      if (!held || held.symbol !== expected.symbol || held.issuer !== expected.issuer)
        throw Object.assign(new Error('The signed wallet setup is for another asset than the reviewed policy.'), {code: 'asset_mismatch'});
    },
  };
  const sameAsset = (stated, asset) => (asset?.issuer ? asset.symbol === stated?.symbol && asset.issuer === stated?.issuer : stated?.issuer === null);
  const approval = {...chain.approval, holds: (fields, wallet, policy) => chain.approval.holds(fields, wallet, policy) && sameAsset(fields.asset, policy.asset)};
  return {world, chain: {...chain, keypairSetup, approval}};
}
const TKN = {symbol: 'TKN', issuer: 'ISSUER'}, NET = {symbol: 'NET', issuer: null};

test('a keypair wallet holds the asset of the reviewed policy: the setup, the installation document and the approval name it', async () => {
  const {world, chain} = assetRail(), signed = [], journal = journalOf();
  const service = keypairService();
  const ask = async (path, body) => path === 'native/approval' && service.state.wallet && !service.state.approved
    ? (service.calls.push([path, body]), {required: true, message: approvalMessage({wallet: service.state.wallet, asset: TKN})}) : service.ask(path, body);
  const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: {...reviewed(), asset: TKN}, onStep() {}};
  const result = await installAndFund(input, chain);
  assert.deepEqual(signed, ['set_up_wallet', 'approve_policy']);
  // The rail's setup hook got the asset, and the server registers the document of that asset.
  assert.deepEqual(world.preparedAssets, [TKN]);
  const registered = service.calls.find(([path]) => path === 'native/register')[1];
  assert.deepEqual(registered, {configuration: {wallet: result.wallet, owner: OWNER, agent, server: SERVER, asset: TKN}, policyId});
  assert.equal(service.state.approved, true);
});

test('an approval in another asset than the reviewed policy\'s is not signed', async () => {
  for (const [stated, policyAsset] of [[NET, TKN], [TKN, undefined], [TKN, null], [TKN, NET], [{symbol: 'TKN', issuer: 'OTHER'}, TKN]]) {
    const {chain} = assetRail(), signed = [];
    const service = keypairService();
    const ask = async (path, body) => path === 'native/approval' ? {required: true, message: approvalMessage({wallet: service.state.wallet, asset: stated})} : service.ask(path, body);
    const policy = policyAsset === undefined ? reviewed() : {...reviewed(), asset: policyAsset};
    await assert.rejects(installAndFund({ask, sign: ownerSigns(signed), journal: journalOf(), key: 'flow', policyId, agent, amount, reviewed: policy, onStep() {}}, chain),
      /differs from the policy that you reviewed/, JSON.stringify([stated, policyAsset]));
    // The setup is signed for the asset of the reviewed policy; no approval is.
    assert.deepEqual(signed, ['set_up_wallet']);
  }
  // Without an asset in the reviewed policy the wallet holds the network's own asset, and so does the approval.
  const {world, chain} = assetRail(), signed = [];
  const service = keypairService();
  const ask = async (path, body) => path === 'native/approval' ? {required: !service.state.approved, message: approvalMessage({wallet: service.state.wallet, asset: NET})} : service.ask(path, body);
  await installAndFund({ask, sign: ownerSigns(signed), journal: journalOf(), key: 'flow', policyId, agent, amount, reviewed: reviewed(), onStep() {}}, chain);
  assert.deepEqual([signed, world.preparedAssets], [['set_up_wallet', 'approve_policy'], [null]]);
});

test('a saved signed setup of another asset is kept and refused: a resume never registers a wallet of the wrong asset', async () => {
  // The registration fails once, so the signed setup of a TKN wallet stays in the journal.
  const {world, chain} = assetRail(), signed = [], journal = journalOf();
  const service = keypairService({registerFails: 1});
  const input = asset => ({ask: service.ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: asset === undefined ? reviewed() : {...reviewed(), asset}, onStep() {}});
  await assert.rejects(installAndFund(input(TKN), chain));
  const saved = journal.get('flow:wallet-setup');
  assert.ok(saved);
  // A resume that reviews the same policy in another asset, or in none, signs nothing and keeps the saved setup.
  for (const other of [undefined, null, NET, {symbol: 'TKN', issuer: 'OTHER'}, {symbol: 'OTH', issuer: 'ISSUER'}]) {
    await assert.rejects(installAndFund(input(other), chain), error => error instanceof OwnerFlowError && error.code === 'asset_mismatch' && error.retry === false, JSON.stringify(other));
    assert.equal(journal.get('flow:wallet-setup'), saved);
  }
  assert.deepEqual([signed, world.created.length, service.state.wallet], [['set_up_wallet'], 1, null]);
  // The resume with the asset of the setup registers that wallet and signs no second setup.
  const ask = async (path, body) => path === 'native/approval' ? {required: !service.state.approved, message: approvalMessage({wallet: service.state.wallet, asset: TKN})} : service.ask(path, body);
  const result = await installAndFund({...input(TKN), ask}, chain);
  assert.deepEqual([signed, world.created, result.wallet], [['set_up_wallet', 'approve_policy'], [service.state.wallet], service.state.wallet]);
});

test('the signed setup bytes decide the funded amount: a saved amount that differs from them funds nothing and registers nothing', async () => {
  // The registration fails once, so the signed setup that funds 5 stays in the journal.
  const {world, chain} = assetRail(), signed = [], journal = journalOf();
  const service = keypairService({registerFails: 1});
  const input = funding => ({ask: service.ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount: funding, reviewed: {...reviewed(), asset: TKN}, onStep() {}});
  await assert.rejects(installAndFund(input('5'), chain));
  const saved = JSON.parse(journal.get('flow:wallet-setup'));
  assert.equal(saved.amount, '5');
  // The saved amount is rewritten to 10. The bytes still fund 5, so a resume for 10 is refused and keeps the setup.
  const rewritten = JSON.stringify({...saved, amount: '10'});
  journal.set('flow:wallet-setup', rewritten);
  await assert.rejects(installAndFund(input('10'), chain), error => error instanceof OwnerFlowError && /another agent key or amount is saved/.test(error.message) && error.retry === false);
  assert.deepEqual([journal.get('flow:wallet-setup'), signed, world.created.length, service.state.wallet], [rewritten, ['set_up_wallet'], 1, null]);
  // The same amount in another spelling is the same amount.
  journal.set('flow:wallet-setup', JSON.stringify({...saved, amount: '5.0'}));
  const ask = async (path, body) => path === 'native/approval' ? {required: !service.state.approved, message: approvalMessage({wallet: service.state.wallet, asset: TKN})} : service.ask(path, body);
  const result = await installAndFund({...input('5.0'), ask}, chain);
  assert.deepEqual([signed, world.created, result.wallet], [['set_up_wallet', 'approve_policy'], [service.state.wallet], service.state.wallet]);
});

test('a keypair change approves only in the asset of the installed policy that the owner reviewed', async () => {
  const {world, chain} = assetRail(), signed = [];
  world.owners.set('WALLET-1', OWNER);
  const attempt = (stated, asset, n) => updateParams({ask: approvals(approvalMessage({revision: 3, asset: stated}), [done]).ask, sign: ownerSigns(signed), journal: journalOf(),
    key: `change-${n}`, policyId, params: change, installed: asset === undefined ? installed('WALLET-1') : {...installed('WALLET-1'), asset}, onStep() {}}, chain);
  // The approval names another asset than the installed policy's: nothing is signed.
  let n = 0;
  for (const [stated, policyAsset] of [[NET, TKN], [TKN, undefined], [TKN, null], [TKN, NET], [{symbol: 'TKN', issuer: 'OTHER'}, TKN]])
    await assert.rejects(attempt(stated, policyAsset, ++n), OwnerFlowError, JSON.stringify([stated, policyAsset]));
  assert.deepEqual(signed, []);
  const result = await attempt(TKN, TKN, ++n);
  assert.deepEqual([result.done, signed], [true, ['approve_policy']]);
  // An installed policy that states no asset approves in the network's own asset, as before.
  assert.equal((await attempt(NET, undefined, ++n)).done, true);
});

test('a value change of a keypair wallet is one approval signature and no transaction', async () => {
  const {world, chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [], journal = journalOf();
  const message = approvalMessage({revision: 3});
  const {ask, submitted} = approvals(message, [done]);
  const input = {ask, sign: ownerSigns(signed), journal, key: 'change', policyId, params: change, installed: installed('WALLET-1'), onStep() {}};
  const result = await updateParams(input, chain);
  assert.deepEqual([result.done, result.receipts.map(r => r.action), signed], [true, ['update_params'], ['approve_policy']]);
  assert.deepEqual(world.reads, ['WALLET-1']);
  assert.deepEqual([submitted[0].policyId, submitted[0].payload, submitted[0].sourceApproval[0].signer], [policyId, message, OWNER]);
  assert.deepEqual(submitted[0].sourceApproval[0].signature, signatureOf(OWNER, sha(message)));
  // A confirmed change is not prepared or signed again.
  const again = await updateParams(input, chain);
  assert.deepEqual([again.done, again.receipts.length, signed.length, submitted.length], [true, 0, 1, 1]);
});

test('a keypair change signs only the reviewed policy with the requested value', async () => {
  const {chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [];
  const attempt = (message, extra = {}) => updateParams({ask: approvals(message, [done]).ask, sign: ownerSigns(signed), journal: journalOf(), key: 'x', policyId, params: change, installed: installed('WALLET-1'), onStep() {}, ...extra}, chain);
  for (const [name, message, expected] of [
    ['another source', approvalMessage({source: 'other'}), /another policy or other values/],
    ['another value beside the requested one', approvalMessage({params: {daily_spending_limit: {Int: '9'}, payment_limit: {Int: '5'}}}), /another policy or other values/],
    ['other rules', approvalMessage({rules: []}), /another policy or other values/],
    ['another wallet', approvalMessage({wallet: 'WALLET-7'}), /another policy or other values/],
    ['another policy', approvalMessage({policyId: 'policy-2'}), /differs from this change/],
    ['an unreadable message', 'not an approval', /differs from this change/],
  ]) await assert.rejects(attempt(message), expected, name);
  await assert.rejects(attempt(approvalMessage(), {installed: undefined}), /reviewed is missing/);
  assert.equal(signed.length, 0);
  // The reviewed policy with exactly the requested value is signed once.
  const result = await attempt(approvalMessage());
  assert.deepEqual([result.done, signed.length], [true, 1]);
  // A network whose rail signs no approval refuses it before anything is signed.
  await assert.rejects(updateParams({ask: approvals(approvalMessage(), [done]).ask, sign: ownerSigns(signed), journal: journalOf(), key: 'y', policyId, params: change, installed: installed('WALLET-1'), onStep() {}}), /signs no policy approval/);
  assert.equal(signed.length, 1);
});

test('a keypair change keeps its signed approval until a confirmed reply, and sends it again', async () => {
  const {chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [];
  const base = {sign: ownerSigns(signed), policyId, params: change, installed: installed('WALLET-1'), onStep() {}};
  // A reply that is lost, then an unconfirmed one: the signed approval stays saved and nothing is signed again.
  const journal = journalOf();
  const {ask, submitted, state} = approvals(approvalMessage(), [() => {throw new Error('reply lost');}, () => ({status: 'pending'}), done]);
  await assert.rejects(updateParams({...base, journal, key: 'b', ask}, chain), /reply lost/);
  assert.ok(journal.get('b:approval'));
  await assert.rejects(updateParams({...base, journal, key: 'b', ask}, chain), /did not confirm/);
  assert.equal(journal.get('b:updated'), null);
  const result = await updateParams({...base, journal, key: 'b', ask}, chain);
  assert.deepEqual([result.done, state.prepares, signed.length, submitted.length], [true, 1, 1, 3]);
  assert.ok(submitted.every(body => JSON.stringify(body) === JSON.stringify(submitted[0])));
  assert.equal(journal.get('b:approval'), null);
  // A change with other values is refused while a signed approval of this change is saved.
  const held = journalOf();
  await assert.rejects(updateParams({...base, journal: held, key: 'c', ask: approvals(approvalMessage(), [() => {throw new Error('reply lost');}]).ask}, chain), /reply lost/);
  await assert.rejects(updateParams({...base, journal: held, key: 'c', ask, params: {payment_limit: {Int: '6'}}}, chain), /another change/);
  // An unreadable saved approval is never replaced by a new signature.
  await assert.rejects(updateParams({...base, journal: journalOf({'d:approval': '{bad'}), key: 'd', ask}, chain), /unreadable/);
  assert.equal(signed.length, 2);
});

test('a saved approval that the service no longer holds is prepared again and sent with the same signature', async () => {
  const {chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [];
  const lapsed = () => Object.assign(new Error('The approval expired.'), {status: 409, code: 'owner_transaction_expired'});
  const base = {sign: ownerSigns(signed), policyId, params: change, installed: installed('WALLET-1'), onStep() {}};
  const same = approvals(approvalMessage(), [() => {throw new Error('reply lost');}, () => {throw lapsed();}, done]);
  const journal = journalOf();
  await assert.rejects(updateParams({...base, journal, key: 'u', ask: same.ask}, chain), /reply lost/);
  const result = await updateParams({...base, journal, key: 'u', ask: same.ask}, chain);
  assert.deepEqual([result.done, same.state.prepares, signed.length, same.submitted.length], [true, 2, 1, 3]);
  assert.ok(same.submitted.every(body => JSON.stringify(body) === JSON.stringify(same.submitted[0])));
  // A preparation whose message differs needs the owner's signature again; the lapsed one is dropped.
  const changed = approvalMessage({revision: 3});
  const other = approvals(state => state.prepares === 1 ? approvalMessage() : changed, [() => {throw new Error('reply lost');}, () => {throw lapsed();}, done]);
  const fresh = journalOf();
  await assert.rejects(updateParams({...base, journal: fresh, key: 'v', ask: other.ask}, chain), /reply lost/);
  const again = await updateParams({...base, journal: fresh, key: 'v', ask: other.ask}, chain);
  assert.deepEqual([again.done, signed.length, other.submitted.at(-1).payload], [true, 3, changed]);
  // A lapse of the prepared-again approval asks for a resume and keeps the signature.
  const twice = approvals(approvalMessage(), [() => {throw new Error('reply lost');}, () => {throw lapsed();}, () => {throw lapsed();}]);
  const kept = journalOf();
  await assert.rejects(updateParams({...base, journal: kept, key: 'w', ask: twice.ask}, chain), /reply lost/);
  await assert.rejects(updateParams({...base, journal: kept, key: 'w', ask: twice.ask}, chain), error => error.code === 'owner_transaction_expired' && error.retry === true);
  assert.ok(kept.get('w:approval'));
});

test('a keypair change signed by another account is refused before it is saved, and the owner can sign again', async () => {
  const {chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [];
  const journal = journalOf();
  const {ask, submitted} = approvals(approvalMessage(), [done]);
  const input = {journal, key: 'w', policyId, params: change, installed: installed('WALLET-1'), onStep() {}, ask};
  await assert.rejects(updateParams({...input, sign: ownerSigns([], 'STRANGER')}, chain), /another account than the owner. Nothing was saved/);
  assert.deepEqual([journal.map.size, submitted.length], [0, 0]);
  // An owner that the network cannot name signs nothing.
  const {chain: unnamed} = keypairRail();
  await assert.rejects(updateParams({...input, sign: ownerSigns(signed)}, unnamed), error => /could not be read/.test(error.message) && error.retry === true);
  assert.equal(signed.length, 0);
  const result = await updateParams({...input, sign: ownerSigns(signed)}, chain);
  assert.deepEqual([result.done, signed.length, submitted.length], [true, 1, 1]);
});

test('the rail decides the shape of an approval signature: a longer signature of its owner applies and resumes', async () => {
  const {chain} = keypairRail({owners: new Map([['WALLET-1', OWNER]])}), signed = [];
  // A rail whose owner signatures are 96 bytes long.
  const long = (account, digest) => [...signatureOf(account, digest), ...signatureOf(account, digest).slice(0, 32)];
  const rail = {...chain, approval: {...chain.approval, signedBy: (account, digest, signature) => JSON.stringify(long(account, digest)) === JSON.stringify(signature)}};
  const sign = async s => (signed.push(s.action), Buffer.from(long(OWNER, s.hash)).toString('base64'));
  const journal = journalOf();
  const {ask, submitted} = approvals(approvalMessage(), [() => {throw new Error('reply lost');}, done]);
  const input = {ask, sign, journal, key: 'long', policyId, params: change, installed: installed('WALLET-1'), onStep() {}};
  await assert.rejects(updateParams(input, rail), /reply lost/);
  // The saved approval keeps the longer signature and is sent again unchanged.
  assert.equal(JSON.parse(journal.get('long:approval')).signature.length, 96);
  const result = await updateParams(input, rail);
  assert.deepEqual([result.done, signed.length, submitted.length], [true, 1, 2]);
  assert.equal(submitted[1].sourceApproval[0].signature.length, 96);
});

test('the rail decides whether a saved setup is of the reviewed asset; a rail without that check keeps the setup saved', async () => {
  const {world, chain} = assetRail(), signed = [], journal = journalOf();
  const service = keypairService({registerFails: 1});
  const input = {ask: service.ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: {...reviewed(), asset: TKN}, onStep() {}};
  await assert.rejects(installAndFund(input, chain));
  const saved = journal.get('flow:wallet-setup');
  // The rail without the check: nothing is sent, registered or signed, and the setup stays saved.
  const {checkAsset, ...without} = chain.keypairSetup;
  await assert.rejects(installAndFund(input, {...chain, keypairSetup: without}), error => error instanceof OwnerFlowError && error.code === 'asset_mismatch' && error.retry === false);
  assert.deepEqual([journal.get('flow:wallet-setup'), signed, world.created.length, service.state.wallet], [saved, ['set_up_wallet'], 1, null]);
  // A refusal of the rail reaches the host with the rail's own message and code.
  const refusing = {...chain, keypairSetup: {...chain.keypairSetup, checkAsset: () => { throw Object.assign(new Error('The rail refuses this setup.'), {code: 'asset_mismatch'}); }}};
  await assert.rejects(installAndFund(input, refusing), error => error.code === 'asset_mismatch' && error.message === 'The rail refuses this setup.');
  // An asynchronous check of a rail is awaited: its refusal stops the resume as well.
  const later = {...chain, keypairSetup: {...chain.keypairSetup, checkAsset: async () => { throw Object.assign(new Error('Later refusal.'), {code: 'asset_mismatch'}); }}};
  await assert.rejects(installAndFund(input, later), error => error.code === 'asset_mismatch' && error.message === 'Later refusal.');
  assert.deepEqual([journal.get('flow:wallet-setup'), signed, service.state.wallet], [saved, ['set_up_wallet'], null]);
  void checkAsset;
});

test('the registration names the asset of the signed setup: a host that changes its asset object during the run changes nothing', async () => {
  for (const resume of [false, true]) {
    const {chain} = assetRail(), signed = [], journal = journalOf();
    const service = keypairService({registerFails: resume ? 1 : 0});
    const ask = async (path, body) => path === 'native/approval' ? {required: !service.state.approved, message: approvalMessage({wallet: service.state.wallet, asset: TKN})} : service.ask(path, body);
    const asset = {...TKN};
    const input = {ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: {...reviewed(), asset}, onStep() {}};
    if (resume) await assert.rejects(installAndFund(input, chain));
    // The host's object changes while the workflow waits for the network.
    const transaction = chain.transaction;
    const changing = {...chain, transaction: async (...args) => { asset.issuer = 'OTHER'; return transaction(...args); }};
    if (!resume) input.sign = async step => { asset.issuer = 'OTHER'; return ownerSigns(signed)(step); };
    await installAndFund(input, changing);
    const registered = service.calls.filter(([path]) => path === 'native/register').at(-1)[1];
    assert.deepEqual(registered.configuration.asset, TKN, `resume ${resume}`);
    asset.issuer = TKN.issuer;
  }
});

test('the funded marker holds the asset: an install of another asset never reports the earlier funding', async () => {
  const {chain} = assetRail(), signed = [], journal = journalOf();
  const service = keypairService();
  const ask = async (path, body) => path === 'native/approval' ? {required: !service.state.approved, message: approvalMessage({wallet: service.state.wallet, asset: TKN})} : service.ask(path, body);
  const input = asset => ({ask, sign: ownerSigns(signed), journal, key: 'flow', policyId, agent, amount, reviewed: asset === undefined ? reviewed() : {...reviewed(), asset}, onStep() {}});
  const result = await installAndFund(input(TKN), chain);
  assert.equal(journal.get('flow:wallet-setup'), null);
  // The same key and amount with the same asset is done; with another asset or none it is refused.
  assert.deepEqual((await installAndFund(input(TKN), chain)).receipts, []);
  for (const other of [undefined, null, NET, {symbol: 'TKN', issuer: 'OTHER'}])
    await assert.rejects(installAndFund(input(other), chain), error => error instanceof OwnerFlowError && /original wallet setup/.test(error.message), JSON.stringify(other));
  assert.deepEqual([signed, service.state.wallet], [['set_up_wallet', 'approve_policy'], result.wallet]);
});

test('a saved approval of a change is refused before it is sent when the installed policy now names another asset', async () => {
  const {world, chain} = assetRail(), signed = [], journal = journalOf();
  world.owners.set('WALLET-1', OWNER);
  let lose = true;
  const {ask, submitted} = approvals(approvalMessage({revision: 3, asset: TKN}), [done]);
  const losing = async (path, body) => {
    if (path === 'native/owner/submit' && lose) {lose = false; throw new Error('The reply was lost.');}
    return ask(path, body);
  };
  const input = asset => ({ask: losing, sign: ownerSigns(signed), journal, key: 'change', policyId, params: change, installed: {...installed('WALLET-1'), asset}, onStep() {}});
  await assert.rejects(updateParams(input(TKN), chain), /reply was lost/);
  const saved = journal.get('change:approval');
  assert.ok(saved);
  const before = submitted.length;
  for (const other of [null, NET, {symbol: 'TKN', issuer: 'OTHER'}])
    await assert.rejects(updateParams(input(other), chain), error => error instanceof OwnerFlowError && /another change/.test(error.message), JSON.stringify(other));
  assert.deepEqual([journal.get('change:approval'), submitted.length, signed], [saved, before, ['approve_policy']]);
  // The same change in the same asset sends the saved approval and signs nothing new.
  assert.equal((await updateParams(input(TKN), chain)).done, true);
  assert.deepEqual(signed, ['approve_policy']);
});
