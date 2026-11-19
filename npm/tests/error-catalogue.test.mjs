// Every typed error code that the SDK names has an entry in the error catalogue of Core. A host shows the entry's
// title with the sender's own message and the help page of the code, so a code without an entry would reach an owner
// without a title.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import {errorCatalogue, errorEntry} from '@allowit/core';
import * as workflow from '../src/owner-workflow.js';

/** Constants of the owner workflow with a code-like value that name an owner step, not a refusal. */
const STEPS = new Set(['UPDATE_PARAMS', 'SET_UP_WALLET', 'APPROVE_POLICY']);

test('every code constant of the owner workflow has a catalogue entry', () => {
  const constants = Object.entries(workflow).filter(([name, value]) => /^[A-Z][A-Z0-9_]+$/.test(name) && typeof value === 'string');
  const codes = constants.filter(([name]) => !STEPS.has(name));
  assert.ok(codes.length >= 8, 'the code constants were not found');
  for (const [name, code] of codes) assert.ok(errorEntry(code), `${name} = '${code}' has no entry in the error catalogue of @allowit/core`);
  // A step name is listed here because it is no error code: it has no entry.
  for (const name of STEPS) assert.equal(errorEntry(workflow[name]), null, name);
});

test('every code that the declarations name has a catalogue entry', async () => {
  const declarations = await readFile(new URL('../src/registry.d.ts', import.meta.url), 'utf8');
  const union = name => {
    const body = new RegExp(`export type ${name} =([^;]+);`).exec(declarations)?.[1];
    assert.ok(body, `${name} was not found`);
    // A member in single or double quotes.
    return [...body.matchAll(/(['"])([a-z][a-z0-9_]+)\1/g)].map(match => match[2]);
  };
  const codes = [...union('OwnerFlowCode'), ...union('WalletKindRefusalCode')];
  assert.ok(codes.length >= 8, 'the declared codes were not found');
  for (const code of codes) assert.ok(errorEntry(code), `'${code}' has no entry in the error catalogue of @allowit/core`);
  assert.ok(errorCatalogue.entries.length > 100);
});
