/**
 * The owner workflows of every rail: install the reviewed policy wallet and fund it, and change installed params on an
 * active wallet. The host supplies its authenticated transport, the signing of one step and a journal. This module holds
 * the rules for a saved signed step, for the journal and for the typed replies of the service. The rail of the network
 * supplies what only it can read or check, through its `ownerChain` hooks: the network's record of a transaction, the
 * record of an expired signed step, the setup of a keypair wallet and the approval of a keypair wallet's policy.
 */

/** Typed codes of a refused `native/owner/submit`. The reply body is `{error, code, details}`. */
export const OWNER_TRANSACTION_EXPIRED = 'owner_transaction_expired';
export const OWNER_TRANSACTION_PENDING = 'owner_transaction_pending';
export const OWNER_TRANSACTION_SETTLED = 'owner_transaction_settled';
/** The transaction is in a ledger and failed. `details.ledger` names that ledger. */
export const OWNER_TRANSACTION_FAILED = 'owner_transaction_failed';
/** HTTP 503 with `details.retry = "same_request_id"`: the service sent nothing. */
export const NETWORK_UNAVAILABLE = 'network_unavailable';
/** Typed codes of a refused params update. `details.param` names the param of a refused value. */
export const PARAM_INVALID = 'param_invalid';
export const PARAM_OUT_OF_RANGE = 'param_out_of_range';
/** The policy is not active, or its wallet kind takes no owner-signed update. */
export const UPDATE_UNAVAILABLE = 'update_unavailable';
/** The wallet is at another configuration revision than the update expects. */
export const REVISION_CHANGED = 'revision_changed';
/** The action name of the owner step of a params update. */
export const UPDATE_PARAMS = 'update_params';
/** The service's refusal of a deployment whose keypair wallet needs its setup first. */
export const WALLET_SETUP_REQUIRED = 'wallet_setup_required';
/** Receipt labels of a keypair wallet's setup and of the owner's approval of its policy. */
export const SET_UP_WALLET = 'set_up_wallet';
export const APPROVE_POLICY = 'approve_policy';

/** An owner workflow failure with the typed server code, when the service sent one. */
export class OwnerFlowError extends Error {
  constructor(message, code, retry, cause) {
    super(message, {cause});
    this.name = 'OwnerFlowError';
    /** The typed server code, or null. */
    this.code = code;
    /** True when the same saved step can be sent again without a new signature. */
    this.retry = retry;
  }
}

const HASH = /^[0-9a-f]{64}$/;
const object = v => v !== null && typeof v === 'object' && !Array.isArray(v);

/** The rail-neutral step of a reply. The server reports its hash as `transactionHash`. */
function stepOf(step) {
  if (!step || typeof step.action !== 'string' || typeof step.payload !== 'string' || typeof step.expiresAt !== 'number') return null;
  const hash = typeof step.hash === 'string' ? step.hash : step.transactionHash;
  return typeof hash === 'string' && HASH.test(hash)
    ? {action: step.action, payload: step.payload, hash, expiresAt: step.expiresAt, ...(step.explorer ? {explorer: step.explorer} : {})} : null;
}
const installedWallet = reply => reply.installed ? reply.installation?.wallet ?? reply.wallet : undefined;

/** The key that keeps the signed bytes of an expired step after the journal drops it. */
export function ownerArchiveKey(key, hash) {
  return `${key}:expired:${hash}`;
}

const unverified = () => new OwnerFlowError('The network record of the signed step could not be checked. It stays saved; resume to check it again.', null, true);
const unchecked = () => new OwnerFlowError('The expired signed step could not be checked on the network. It stays saved; resume to check it again.', OWNER_TRANSACTION_EXPIRED, true);

/**
 * The ledger in which the network holds this transaction as successful. The service's word alone never clears a signed
 * step or records a funding. History can trail the network by a few seconds, so a missing record is read again.
 */
async function settledLedger(chain, network, step, ledger, pause) {
  for (let attempt = 0; attempt < 5; attempt++) {
    if (attempt) await new Promise(resolve => setTimeout(resolve, pause));
    const record = await chain.transaction(network, step.hash).catch(() => null);
    if (record === 'missing') continue;
    if (!object(record) || record.hash !== step.hash || record.successful !== true || !Number.isSafeInteger(record.ledger)
      || record.ledger <= 0 || ledger !== undefined && record.ledger !== ledger) break;
    return record.ledger;
  }
  throw unverified();
}

/** True when the network holds this transaction as failed in the ledger that the service named. */
async function failedOnChain(chain, network, step, ledger) {
  if (!Number.isSafeInteger(ledger)) return false;
  const record = await chain.transaction(network, step.hash).catch(() => null);
  return object(record) && record.hash === step.hash && record.ledger === ledger && record.successful === false;
}

/** The most bytes of a message signature that the workflow keeps. The rail decides the exact shape. */
const MAX_SIGNATURE_BYTES = 1024;
/** True for a list of bytes that can hold a message signature; the rail's `signedBy` checks the signature itself. */
const signatureShaped = bytes => Array.isArray(bytes) && bytes.length > 0 && bytes.length <= MAX_SIGNATURE_BYTES
  && bytes.every(b => Number.isInteger(b) && b >= 0 && b <= 255);
/** The bytes of a wallet's message signature, which the wallet returns as base64 text. */
function signatureBytes(signature) {
  let bytes = [];
  try {
    bytes = typeof signature === 'string' ? Array.from(atob(signature), c => c.charCodeAt(0)) : [];
  } catch { /* refused below */ }
  if (!signatureShaped(bytes)) throw new OwnerFlowError('Your wallet did not return a message signature.', null, false);
  return bytes;
}

/** Workflows that run in this page, by journal key. */
const running = new Set();
const busy = () => new OwnerFlowError('This wallet setup is already running in this or another tab.', null, true);

/** The inputs that a saved step and a completion marker belong to: the wallet setup or one params update. */
const FIELDS = ['policyId', 'agent', 'amount', 'update'];
const changed = identity => new OwnerFlowError('update' in identity
  ? 'The saved signed step belongs to another change. It stays saved; resume that change first.'
  : 'Use the agent key and amount from the original wallet setup. The saved signed step stays saved.', null, false);
/** True when a saved value names these inputs. A value saved without inputs predates them and names none. */
function sameInputs(value, identity) {
  const named = FIELDS.filter(field => value[field] !== undefined);
  return named.length === 0 || named.length === Object.keys(identity).length && named.every(field => value[field] === identity[field]);
}

/**
 * Reads a retained signed setup of a keypair wallet when the service reports it installed. True when the setup created
 * `wallet` for this agent with this funding amount: the server registers a wallet only after the network confirmed its
 * setup, so that setup funded it. False when nothing is retained. Any other retained setup stays saved and is refused:
 * it names other inputs, and funding again under them could move the owner's funds twice.
 */
function retainedSetup(chain, raw, network, wallet, identity) {
  if (raw === null) return false;
  let value;
  try {
    value = JSON.parse(raw);
  } catch { /* refused below */ }
  const setup = object(value) && typeof value.signedPayload === 'string' ? chain.keypairSetup?.read(value.signedPayload, network) : null;
  if (!object(value) || !setup || setup.wallet !== wallet || value.agent !== identity.agent || setup.agent !== identity.agent
    || value.amount !== identity.amount)
    throw changed(identity);
  return true;
}

/**
 * Sets up the account of a keypair wallet: one transaction that the owner's wallet signs. The rail builds it, and its
 * `complete` adds the new account's own signature. The signed bytes are saved under `key` before they are sent, and they
 * stay saved after the network confirms them: the caller removes them once the server has registered the wallet, so a
 * run that stops in between names the same account again.
 *
 * A saved setup is settled first. It is done when the network holds it as successful. It is sent again while its own
 * time bound lasts. It is dropped only when the network holds it as failed, or proves past its time bound that it never
 * applied; then a new setup is prepared, so one run never creates two wallets. Any unclear answer keeps it saved.
 */
async function setUpKeypairWallet(setup, chain, {network, owner, agent, server, amount, sign, onStep, journal, key}) {
  const pending = cause => new OwnerFlowError('The network did not confirm the wallet setup. The signed setup stays saved: resume to check it. Nothing is signed twice.', null, true, cause);
  const unreadable = () => new OwnerFlowError('The saved signed wallet setup is unreadable. It stays saved; nothing was signed.', null, false);
  const done = (wallet, hash, ledger) => ({wallet, transactionHash: hash, ledger, configuration: setup.configuration(network, wallet, owner, agent, server)});
  const raw = journal.get(key);
  if (raw !== null) {
    let saved;
    try {
      saved = JSON.parse(raw);
    } catch { /* refused below */ }
    if (!object(saved) || typeof saved.hash !== 'string' || !HASH.test(saved.hash) || typeof saved.signedPayload !== 'string')
      throw unreadable();
    if (saved.owner !== owner || saved.agent !== agent || saved.server !== server || saved.amount !== amount)
      throw new OwnerFlowError('A signed wallet setup with another agent key or amount is saved. It stays saved; use the agent key and amount of that setup.', null, false);
    onStep('Checking the saved wallet setup…');
    // The saved bytes decide: their hash, the account that they create and their own time bound.
    const signed = setup.read(saved.signedPayload, network);
    if (!object(signed) || signed.hash !== saved.hash || signed.owner !== owner || signed.agent !== agent || signed.server !== server
      || !Number.isSafeInteger(signed.validUntil))
      throw unreadable();
    const record = await chain.transaction(network, saved.hash).catch(() => null);
    if (object(record)) {
      // Only the record of this exact transaction settles it: successful, or failed in a ledger.
      if (record.hash !== saved.hash || !Number.isSafeInteger(record.ledger) || record.ledger <= 0) throw pending();
      if (record.successful === true) return done(signed.wallet, saved.hash, record.ledger);
      if (record.successful !== false) throw pending();
      // The network holds the setup as failed: it used its sequence, created nothing and can never apply.
    } else if (record !== 'missing') throw pending();
    else if (signed.validUntil >= Math.floor(Date.now() / 1000)) {
      // The same signed bytes go again while their time bound lasts.
      const ledger = await setup.send(network, saved.signedPayload, saved.hash).catch(() => null);
      if (!Number.isSafeInteger(ledger)) throw pending();
      return done(signed.wallet, saved.hash, ledger);
    } else {
      // Past its time bound a missing record proves nothing: history can lack an applied setup.
      const expired = await chain.expiredRecord(network, saved.hash, saved.signedPayload).catch(cause => {throw pending(cause);});
      if (expired) return done(signed.wallet, saved.hash, expired.ledger);
    }
    journal.remove(key);
  }
  onStep('Preparing the wallet…');
  const failed = cause => new OwnerFlowError(cause instanceof Error && cause.message ? cause.message : 'The wallet account could not be set up.', null, false, cause);
  const prepared = await setup.prepare({network, owner, agent, server, amount}).catch(cause => {throw failed(cause);});
  const step = stepOf(prepared?.step);
  if (!object(prepared) || !step || typeof prepared.wallet !== 'string' || typeof prepared.complete !== 'function') throw failed();
  onStep('Sign the wallet setup in your wallet…');
  const ownerSigned = await sign(step);
  let signedPayload;
  try {
    signedPayload = prepared.complete(ownerSigned);
  } catch (cause) {
    throw failed(cause);
  }
  // The signed bytes are saved before they leave: an unknown result is settled from them, never signed again.
  journal.set(key, JSON.stringify({hash: step.hash, signedPayload, owner, agent, server, amount}));
  onStep('Waiting for the network to confirm…');
  const ledger = await setup.send(network, signedPayload, step.hash).catch(() => null);
  if (!Number.isSafeInteger(ledger)) throw pending();
  return done(prepared.wallet, step.hash, ledger);
}

/** A signed approval of a keypair wallet's new values, saved before it is sent. */
function savedApproval(journal, key, identity) {
  const raw = journal.get(key);
  if (raw === null) return null;
  let value;
  try {
    value = JSON.parse(raw);
  } catch { /* refused below */ }
  if (!object(value) || typeof value.message !== 'string' || typeof value.signer !== 'string' || !signatureShaped(value.signature))
    throw new OwnerFlowError('The saved signed approval is unreadable. It stays saved; contact support before signing again.', null, false);
  if (!sameInputs(value, identity)) throw changed(identity);
  return value;
}

/**
 * The journal version of a signed step that this workflow saves. A wallet creation saved under it also funds the wallet;
 * a creation saved without it, by an earlier version, created the wallet alone, and the funding still follows.
 */
const STEP_VERSION = 2;
function saved(journal, key, identity) {
  const raw = journal.get(key);
  if (raw === null) return null;
  let value;
  try {
    value = JSON.parse(raw);
  } catch { /* fall through */ }
  const step = object(value) ? stepOf(value.step) : null;
  if (!object(value) || !step || typeof value.signedPayload !== 'string' || !value.signedPayload)
    throw new OwnerFlowError('The saved signed step is unreadable. It stays saved; contact support before signing again.', null, false);
  if (!sameInputs(value, identity)) throw changed(identity);
  return {step, signedPayload: value.signedPayload, current: value.version === STEP_VERSION};
}

/** True when the funding or the update of these inputs is confirmed. A marker of other inputs is refused. */
function funded(journal, fundedKey, identity) {
  const raw = journal.get(fundedKey);
  if (raw === null) return false;
  if (raw === 'confirmed') return true;
  let value;
  try {
    value = JSON.parse(raw);
  } catch { /* fall through */ }
  if (!object(value) || !sameInputs(value, identity)) throw changed(identity);
  return true;
}

/** Run `work` alone for this journal key: in this page, and where the browser has Web Locks in every tab. */
async function exclusive(key, work) {
  const name = `allowit.owner-flow:${key}`;
  if (running.has(name)) throw busy();
  running.add(name);
  try {
    const locks = globalThis.navigator?.locks;
    if (!locks) return await work();
    return await locks.request(name, {ifAvailable: true}, async lock => {
      if (!lock) throw busy();
      return work();
    });
  } finally {
    running.delete(name);
  }
}

/**
 * Install the reviewed policy wallet, then fund it. The service prepares each owner step and the host signs it. Signed
 * bytes enter the journal before they are sent, so a reload sends the same signature and never signs a step twice. A
 * saved step is settled first:
 * - confirmed, or `owner_transaction_settled` for the same hash: the step is done once the network holds that successful
 *   transaction. Until then it stays saved and the error has `retry` set;
 * - `owner_transaction_expired`: the network decides. When the transaction applied before its time bound, the step is
 *   done. When the network proves that it never applied, its signed bytes move to the archive key and the owner signs a
 *   new step. When the network cannot be read, the step stays saved and the error has `retry` set;
 * - `owner_transaction_failed`: when the network holds that failed transaction, its signed bytes move to the archive key
 *   and the owner signs a new step;
 * - `owner_transaction_pending` or `network_unavailable`: the step stays saved and the error has `retry` set.
 * Any other failure keeps the saved step, so no second signature exists while the first can still apply. A saved step
 * and the funded marker name the policy, agent key and amount. A resume with other inputs is refused. One workflow runs
 * per journal key: a second call in this page or, where the browser has Web Locks, in another tab is refused before it
 * reads the journal. `chain` is the rail's `ownerChain`.
 */
export function installAndFund(chain, input) {
  return exclusive(input.key, () => run(chain, input));
}

/** The parts of one workflow that send, settle and sign its owner steps. */
function stepper(chain, {network, historyPauseMs, ask, sign, journal, key, policyId, onStep, identity, confirmed}) {
  const receipts = [];
  const done = (step, receipt, current) => {
    confirmed(step, current);
    journal.remove(key);
    receipts.push(receipt);
  };
  /** Send a saved step. False means that it expired and the journal dropped it. */
  const submit = async (step, signedPayload, current) => {
    let receipt;
    try {
      receipt = await ask('native/owner/submit', {policyId, payload: signedPayload});
    } catch (error) {
      const {code, details} = object(error) ? error : {};
      const about = object(details) ? details : {};
      // A typed refusal settles the saved step only when it names that exact transaction.
      const same = about.transactionHash === step.hash && (about.policyId === undefined || about.policyId === policyId);
      if (code === OWNER_TRANSACTION_SETTLED && same) {
        const ledger = await settledLedger(chain, network, step, Number.isSafeInteger(about.ledger) ? about.ledger : undefined, historyPauseMs);
        done(step, {policyId, action: step.action, status: 'confirmed', transactionHash: step.hash, ledger}, current);
        return true;
      }
      if (code === OWNER_TRANSACTION_EXPIRED && same) {
        if (step.expiresAt >= Math.floor(Date.now() / 1000)) throw unchecked();
        const record = await chain.expiredRecord(network, step.hash, signedPayload).catch(() => {throw unchecked();});
        if (record) {
          // The transaction applied before its time bound, so the step is done and is never signed again.
          done(step, {policyId, action: step.action, status: 'confirmed', transactionHash: step.hash, ledger: record.ledger}, current);
          return true;
        }
        journal.set(ownerArchiveKey(key, step.hash), JSON.stringify({step, signedPayload}));
        journal.remove(key);
        return false;
      }
      // A failed transaction used its sequence and can never apply. The network must hold that exact failure.
      if (code === OWNER_TRANSACTION_FAILED && same && await failedOnChain(chain, network, step, about.ledger)) {
        journal.set(ownerArchiveKey(key, step.hash), JSON.stringify({step, signedPayload}));
        journal.remove(key);
        return false;
      }
      if (code === OWNER_TRANSACTION_PENDING)
        throw new OwnerFlowError('The network has not settled the signed step yet. Resume in a moment; do not sign again.', code, true, error);
      if (code === NETWORK_UNAVAILABLE && about.retry === 'same_request_id')
        throw new OwnerFlowError('The network is unavailable. Nothing was sent; resume to send the same signed step.', code, true, error);
      throw new OwnerFlowError(error instanceof Error && error.message ? error.message : 'The signed owner step was not confirmed.',
        typeof code === 'string' ? code : null, false, error);
    }
    if (!object(receipt) || receipt.policyId !== policyId || receipt.action !== step.action || receipt.status !== 'confirmed'
      || receipt.transactionHash !== step.hash)
      throw new OwnerFlowError('The network did not confirm the signed owner step.', null, false);
    const ledger = await settledLedger(chain, network, step, receipt.ledger, historyPauseMs);
    done(step, {...receipt, ledger}, current);
    return true;
  };
  /** Settle the saved signed step, if the journal holds one, before anything else is prepared. */
  const settleSaved = async () => {
    const pending = saved(journal, key, identity);
    if (pending) {
      onStep('Checking the saved signed step…');
      if (!await submit(pending.step, pending.signedPayload, pending.current)) onStep('The saved step can never apply. Preparing a new one…');
    }
  };
  const signAndSubmit = async step => {
    if (step.expiresAt <= Math.floor(Date.now() / 1000)) throw new OwnerFlowError('The prepared owner step already expired. Resume to prepare a new one.', null, true);
    onStep(`Sign ${step.action.replace(/_/g, ' ')} in your wallet…`);
    const signedPayload = await sign(step);
    if (typeof signedPayload !== 'string' || !signedPayload) throw new OwnerFlowError('Your wallet did not return the signed step.', null, false);
    journal.set(key, JSON.stringify({step, signedPayload, ...identity, version: STEP_VERSION}));
    onStep('Waiting for the network to confirm…');
    if (!await submit(step, signedPayload, true)) throw new OwnerFlowError('The network did not take the signed step. Resume to prepare a new one.', null, true);
  };
  return {receipts, settleSaved, signAndSubmit};
}

/**
 * The owner's signature of a keypair wallet's approval message: one message signature, and no transaction. Only the
 * signature of the wallet's owner, as the network names it, is kept: another account's signature would be refused and
 * sent again forever.
 */
async function ownerApproval(chain, {network, sign, onStep}, message, wallet, {then, prompt, kept}) {
  const signer = await chain.approval.walletOwner(network, wallet).catch(() => null);
  if (typeof signer !== 'string' || !signer)
    throw new OwnerFlowError(`The owner of the wallet account could not be read from the network. ${then}`, null, true);
  onStep(prompt);
  const step = chain.approval.step(message);
  const signature = signatureBytes(await sign(step));
  if (!chain.approval.signedBy(signer, step.hash, signature))
    throw new OwnerFlowError(`Your wallet signed with another account than the owner. Nothing was ${kept}; sign with the owner account.`, null, false);
  return {signer, signature};
}
const noApproval = () => new OwnerFlowError('This network signs no policy approval. Nothing was signed.', null, false);

async function run(chain, {network, ask, sign, journal, key, policyId, agent, amount, reviewed, onStep, historyPauseMs = 1_500}) {
  const fundedKey = `${key}:funded`;
  const identity = {policyId, agent, amount};
  const {receipts, settleSaved, signAndSubmit} = stepper(chain, {network, historyPauseMs, ask, sign, journal, key, policyId, onStep, identity,
    // A confirmed funding is recorded before its signed bytes leave the journal. A contract wallet's creation funds it
    // in the same transaction, when this version saved it: an earlier version's creation funded nothing.
    confirmed: (step, current) => {
      if (step.action === 'fund' || step.action === 'create_wallet' && current) journal.set(fundedKey, JSON.stringify(identity));
    }});
  // A saved step or a funded marker of other inputs is refused before anything is sent.
  saved(journal, key, identity);
  funded(journal, fundedKey, identity);
  await settleSaved();
  let wallet;
  // True once this run set up the wallet account of a keypair policy: the deployment then names the wallet.
  let keypairSetUp = false;
  const walletSetupKey = `${key}:wallet-setup`;
  /** Creates and funds the agent's wallet account with one owner signature, and registers it for this policy. */
  const setUpWallet = async details => {
    const roles = object(details) ? details : {};
    if (typeof roles.owner !== 'string' || typeof roles.server !== 'string' || roles.agent !== agent || roles.policyId !== policyId)
      throw new OwnerFlowError('The service named no roles for the wallet account of this policy.', null, false);
    if (!chain.keypairSetup) throw new OwnerFlowError('This network sets up no wallet account.', null, false);
    // The signed setup stays in the journal until the server has registered the wallet, so a run that stops in
    // between finds the same account again.
    const setup = await setUpKeypairWallet(chain.keypairSetup, chain,
      {network, owner: roles.owner, agent, server: roles.server, amount, sign, onStep, journal, key: walletSetupKey});
    receipts.push({policyId, action: SET_UP_WALLET, status: 'confirmed', transactionHash: setup.transactionHash, ledger: setup.ledger});
    onStep('Registering the wallet…');
    const registered = await ask('native/register', {configuration: setup.configuration, policyId});
    if (!object(registered) || registered.registered !== true || registered.wallet !== setup.wallet || registered.policyId !== policyId)
      throw new OwnerFlowError('The service did not register the wallet account for this policy. Resume to register it; nothing is signed again.', null, true);
    // The setup funded the wallet: no funding step follows.
    journal.set(fundedKey, JSON.stringify(identity));
  };
  /** The owner's approval of the policy of a keypair wallet: one message signature, and no transaction. */
  const approvePolicy = async account => {
    onStep('Preparing the approval…');
    const approval = await ask('native/approval', {policyId});
    if (!object(approval) || approval.required !== true) return;
    if (!chain.approval) throw noApproval();
    const fields = chain.approval.read(approval.message, network);
    if (!reviewed) throw new OwnerFlowError('The policy that you reviewed is missing. Nothing was signed.', null, false);
    if (!fields || fields.policyId !== policyId || fields.wallet !== account || !chain.approval.holds(fields, account, reviewed))
      throw new OwnerFlowError('The approval differs from the policy that you reviewed or from its wallet account. Nothing was signed.', null, false);
    const {signer, signature} = await ownerApproval(chain, {network, sign, onStep}, approval.message, account,
      {then: 'Resume to read it again.', prompt: 'Sign the approval of the policy in your wallet…', kept: 'sent'});
    await ask('micro/allocation/confirm', {id: policyId, revision: fields.revision, sourceApproval: [{signer, signature}]});
    receipts.push({policyId, action: APPROVE_POLICY, status: 'confirmed', transactionHash: ''});
  };

  // A deployment takes at most the wallet creation, which funds it, and a lifetime renewal. The server uploads the code
  // itself.
  for (let i = 0; i < 4 && !wallet; i++) {
    onStep('Preparing the wallet…');
    let reply;
    try {
      reply = await ask('native/deploy/prepare', {policyId, agent, amount});
    } catch (error) {
      const {code, details} = object(error) ? error : {};
      // A keypair policy needs the wallet account of its agent. This workflow sets the account up from the owner's own
      // account, the server registers it, and the deployment then answers with the wallet.
      if (code === WALLET_SETUP_REQUIRED && !keypairSetUp) {
        await setUpWallet(details);
        keypairSetUp = true;
        i--;
        continue;
      }
      throw error;
    }
    if (!object(reply) || reply.policyId !== policyId) throw new OwnerFlowError('The deployment reply names a different policy.', null, false);
    wallet = installedWallet(reply);
    if (wallet && reply.walletKind === 'native') {
      // A setup of this workflow created and funded the wallet account. A run that stopped after the registration and
      // before its reply kept the signed setup: when it names this wallet, agent and amount, the setup funded it, and
      // that funding is recorded before the setup leaves the journal. What remains is the owner's approval of the
      // policy. A wallet account that this workflow did not fund is funded below.
      if (retainedSetup(chain, journal.get(walletSetupKey), network, wallet, identity)) journal.set(fundedKey, JSON.stringify(identity));
      journal.remove(walletSetupKey);
      if (reply.approved !== true) await approvePolicy(wallet);
      break;
    }
    if (wallet) break;
    const step = stepOf(reply.step);
    if (!step) throw new OwnerFlowError('The service did not prepare an owner step for this policy.', null, false);
    await signAndSubmit(step);
  }
  if (!wallet) throw new OwnerFlowError('The wallet installation could not be confirmed.', null, true);
  // A confirmed funding is never prepared or signed again, including after a reload or a failed export.
  if (!funded(journal, fundedKey, identity)) {
    onStep('Preparing the funding…');
    const funding = await ask('native/owner/prepare', {policyId, action: 'fund', amount});
    const step = stepOf(funding?.step);
    if (!object(funding) || funding.policyId !== policyId || !step || step.action !== 'fund') throw new OwnerFlowError('The funding reply differs from this policy.', null, false);
    await signAndSubmit(step);
  }
  return {wallet, receipts};
}

/** Canonical text of a JSON value: object keys in code-unit order at every level. */
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (object(value)) return `{${Object.keys(value).sort().map(k => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  return JSON.stringify(value) ?? 'null';
}

/**
 * Change installed params on an active wallet with one owner signature. The service checks each value, prepares the
 * owner step and, when the network confirms it, saves the new params of the installation and of the policy in one write.
 * The wallet keeps its recorded usage and its request nonce. The workflow resumes as `installAndFund` does: a saved
 * signed step is settled first, an expired step that never applied moves to the archive key, and the owner then signs a
 * new step. A confirmed change is never prepared or signed again under its journal key; `done` is then true and
 * `receipts` is empty. A refusal of the service keeps its typed code in `OwnerFlowError.code`: `param_invalid`,
 * `param_out_of_range`, `update_unavailable` or `revision_changed`, and the codes of a refused submission. `chain` is the
 * rail's `ownerChain`.
 */
export function updateParams(chain, input) {
  return exclusive(input.key, () => runUpdate(chain, input));
}

async function runUpdate(chain, {network, ask, sign, journal, key, policyId, params, expectedRevision, installed, onStep, historyPauseMs = 1_500}) {
  if (!object(params) || Object.keys(params).length === 0 || Object.keys(params).length > 32)
    throw new OwnerFlowError('Change one to 32 params.', PARAM_INVALID, false);
  if (expectedRevision !== undefined && !(Number.isSafeInteger(expectedRevision) && expectedRevision >= 0))
    throw new OwnerFlowError('The expected revision is invalid.', REVISION_CHANGED, false);
  const updatedKey = `${key}:updated`;
  const identity = {policyId, update: canonical({params, ...(expectedRevision === undefined ? {} : {expectedRevision})})};
  const {receipts, settleSaved, signAndSubmit} = stepper(chain, {network, historyPauseMs, ask, sign, journal, key, policyId, onStep, identity,
    // A confirmed update is recorded before its signed bytes leave the journal.
    confirmed: step => {if (step.action === UPDATE_PARAMS) journal.set(updatedKey, JSON.stringify(identity));}});
  // A saved step or a marker of another change is refused before anything is sent.
  saved(journal, key, identity);
  if (funded(journal, updatedKey, identity)) return {receipts, done: true};
  await settleSaved();
  if (funded(journal, updatedKey, identity)) return {receipts, done: true};
  const approvalKey = `${key}:approval`;
  const prepare = async () => {
    onStep('Preparing the change…');
    try {
      return await ask('native/owner/prepare', {policyId, action: UPDATE_PARAMS, params, ...(expectedRevision === undefined ? {} : {expectedRevision})});
    } catch (error) {
      // The service's reason and typed code reach the owner unchanged.
      const code = object(error) && typeof error.code === 'string' ? error.code : null;
      throw new OwnerFlowError(error instanceof Error && error.message ? error.message : 'The change could not be prepared.', code, code === NETWORK_UNAVAILABLE, error);
    }
  };
  /** The approval message of a keypair wallet's change, or null for a reply that prepares a transaction. */
  const approvalOf = prepared =>
    object(prepared) && prepared.policyId === policyId && object(prepared.approval) && !prepared.step ? prepared.approval.message : null;
  /**
   * Sends a signed approval of a keypair wallet's values. Only a confirmed reply for this policy and change ends the
   * change. Null means that the service no longer holds the preparation of this approval (it expired, or another
   * instance prepared it): the approval never applied, because the service answers a settled approval as confirmed. Any
   * other answer keeps the signed approval saved, and the next call sends the same bytes again.
   */
  const submitApproval = async ({message, signer, signature}) => {
    onStep('Saving the change…');
    let reply;
    try {
      reply = await ask('native/owner/submit', {policyId, payload: message, sourceApproval: [{signer, signature}]});
    } catch (error) {
      const code = object(error) && typeof error.code === 'string' ? error.code : null;
      if (code === OWNER_TRANSACTION_EXPIRED || (object(error) && error.status === 404)) return null;
      throw new OwnerFlowError(error instanceof Error && error.message ? error.message : 'The change could not be saved.', code, true, error);
    }
    if (!object(reply) || reply.policyId !== policyId || reply.action !== UPDATE_PARAMS || reply.status !== 'confirmed')
      throw new OwnerFlowError('The service did not confirm the change. The signed approval stays saved: resume to send it again.', null, true);
    journal.set(updatedKey, JSON.stringify(identity));
    journal.remove(approvalKey);
    receipts.push({policyId, action: UPDATE_PARAMS, status: 'confirmed', transactionHash: ''});
    return {receipts, done: true};
  };
  const lapsed = () => new OwnerFlowError('The approval lapsed before the service took it. Resume to prepare the same change again.', OWNER_TRANSACTION_EXPIRED, true);
  let prepared;
  // A signed approval that a lost reply left saved is sent again before anything new is signed. When the service no
  // longer holds its preparation, the same change is prepared again: the same values give the same message, and the
  // saved signature applies. A message that differs needs the owner's signature again.
  const pendingApproval = savedApproval(journal, approvalKey, identity);
  if (pendingApproval) {
    const settled = await submitApproval(pendingApproval);
    if (settled) return settled;
    prepared = await prepare();
    if (approvalOf(prepared) === pendingApproval.message) return (await submitApproval(pendingApproval)) ?? Promise.reject(lapsed());
    journal.remove(approvalKey);
  } else prepared = await prepare();
  // A keypair wallet holds no values on chain: its change is a new approval of the policy, with no transaction.
  const message = approvalOf(prepared);
  if (message !== null) {
    if (!chain.approval) throw noApproval();
    const fields = chain.approval.read(message, network);
    if (!fields || fields.policyId !== policyId) throw new OwnerFlowError('The approval differs from this change. Nothing was signed.', null, false);
    // The owner signs only the reviewed policy with this change: the same wallet, source and rules, each changed param
    // at exactly its requested value, and every other value as it is installed.
    if (!installed || !object(installed.params))
      throw new OwnerFlowError('The installed policy that you reviewed is missing. Nothing was signed.', null, false);
    if (!chain.approval.holds(fields, installed.wallet, {source: installed.source, rules: installed.rules, params: {...installed.params, ...params}, tier: installed.tier}))
      throw new OwnerFlowError('The approval names another policy or other values than this change. Nothing was signed.', null, false);
    const {signer, signature} = await ownerApproval(chain, {network, sign, onStep}, message, fields.wallet,
      {then: 'Nothing was signed; try again.', prompt: 'Sign the approval of the new values in your wallet…', kept: 'saved'});
    // The signed approval is saved before it leaves: a lost reply is settled from it, never signed again.
    const approval = {...identity, message, signer, signature};
    journal.set(approvalKey, JSON.stringify(approval));
    return (await submitApproval(approval)) ?? Promise.reject(lapsed());
  }
  const step = stepOf(prepared?.step);
  if (!object(prepared) || prepared.policyId !== policyId || !step || step.action !== UPDATE_PARAMS)
    throw new OwnerFlowError('The reply differs from this change.', null, false);
  await signAndSubmit(step);
  return {receipts, done: true};
}
