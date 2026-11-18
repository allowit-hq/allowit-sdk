// A host's view of the SDK types. The owner workflows' rail hooks are Core's contract.
import type {OwnerChain, OwnerStep, TransactionRecord, RailPlugin, InstallAndFundInput} from '../src/registry.js';
import {installAndFund, rails, OwnerFlowError} from '../src/registry.js';

const step: OwnerStep = {action: 'fund', payload: 'p', hash: 'ab'.repeat(32), expiresAt: 1};
const record: TransactionRecord = {hash: step.hash, ledger: 7, successful: true};
const chain: OwnerChain = {transaction: async () => record, expiredRecord: async () => null};
const plugin: RailPlugin | undefined = rails()[0];
const hooks: OwnerChain | undefined = plugin?.ownerChain;
const input: Omit<InstallAndFundInput, 'ask' | 'sign' | 'journal'> = {key: 'k', policyId: 'p', agent: 'a', amount: '5', onStep() {}};
// @ts-expect-error A transaction record states whether it succeeded.
const unclear: TransactionRecord = {hash: step.hash, ledger: 7};
void [chain, hooks, input, unclear, installAndFund, OwnerFlowError];
