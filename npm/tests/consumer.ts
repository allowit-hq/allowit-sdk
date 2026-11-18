// A host's view of the SDK types. The owner workflows' rail hooks, the catalog and the param values are Core's contract.
import type {OwnerChain, OwnerStep, TransactionRecord, RailPlugin, InstallAndFundInput} from '../src/registry.js';
import type {CatalogEntry, SearchResult, Ranking, ParamValue, AccessKind} from '../src/registry.js';
import type * as Core from '@allowit/core';
import {installAndFund, rails, OwnerFlowError} from '../src/registry.js';

const step: OwnerStep = {action: 'fund', payload: 'p', hash: 'ab'.repeat(32), expiresAt: 1};
const record: TransactionRecord = {hash: step.hash, ledger: 7, successful: true};
const chain: OwnerChain = {transaction: async () => record, expiredRecord: async () => null};
const plugin: RailPlugin | undefined = rails()[0];
const hooks: OwnerChain | undefined = plugin?.ownerChain;
const input: Omit<InstallAndFundInput, 'ask' | 'sign' | 'journal'> = {key: 'k', policyId: 'p', agent: 'a', amount: '5', onStep() {}};
// @ts-expect-error A transaction record states whether it succeeded.
const unclear: TransactionRecord = {hash: step.hash, ledger: 7};

// The SDK names Core's catalog and value types; it declares no copy of them.
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
const sameTypes: [
  Same<CatalogEntry, Core.CatalogEntry>,
  Same<SearchResult, Core.SearchResult>,
  Same<Ranking, Core.CatalogRanking>,
  Same<ParamValue, Core.PolicyValue>,
  Same<AccessKind, Core.AccessKind>,
] = [true, true, true, true, true];
const access: AccessKind = 'proof_of_work';
const page: SearchResult = {
  entries: [{
    id: 'e', name: 'n', description: '', provider: 'p', resource: 'https://seller.example/r', methods: ['GET'],
    networks: [], assets: [], tags: [], access: [{kind: access, detail: 'one hash'}],
    observed: [{fact: 'response', origin: 'registry', at: '2026-11-18T00:00:00Z', status: 402}],
    rank: {score: 0.7, relevance: 1, reputation: 0, terms: 0},
  }],
  ranking: 'hybrid',
  catalog: 'h',
};
const value: ParamValue = {List: [{Int: '5'}, {Addr: 'a'}]};
// @ts-expect-error A policy value holds exactly one storage tag.
const twoTags: ParamValue = {Int: '5', Bool: true};
void [chain, hooks, input, unclear, installAndFund, OwnerFlowError, sameTypes, page, value, twoTags];
