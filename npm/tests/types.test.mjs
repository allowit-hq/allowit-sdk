// The declarations of the SDK compile against Core's types, as a host compiles them.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {readFile, rm, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import test from 'node:test';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
test('the declarations compile in a strict host and take the owner hooks from Core', () => {
  const tsc = fileURLToPath(new URL('../node_modules/typescript/bin/tsc', import.meta.url));
  const result = spawnSync(process.execPath, [tsc, '--noEmit', '--strict', '--skipLibCheck', 'false', '--target', 'ES2022', '--module', 'NodeNext',
    '--moduleResolution', 'NodeNext', '--allowJs', 'false', 'tests/consumer.ts'], {cwd: root, encoding: 'utf8'});
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});
test('the declarations keep their Core types when a host loads a registered rail first', async () => {
  const tsc = fileURLToPath(new URL('../node_modules/typescript/bin/tsc', import.meta.url));
  // The registered rail modules, as src/registered.js names them. A rail's declarations load the rail's own Core.
  const registered = await readFile(new URL('../src/registered.js', import.meta.url), 'utf8');
  const modules = [...registered.matchAll(/from '([^']+)'/g)].map(match => match[1]);
  assert.ok(modules.length > 0);
  for (const [index, module] of modules.entries()) {
    const wrapper = join(root, 'tests', `rail-first-${process.pid}-${index}.ts`);
    await writeFile(wrapper, `import type * as Rail from '${module}';\nimport './consumer-after-rail.js';\nexport type RailModule = typeof Rail;\n`);
    try {
      const result = spawnSync(process.execPath, [tsc, '--noEmit', '--strict', '--skipLibCheck', 'false', '--target', 'ES2022', '--module', 'NodeNext',
        '--moduleResolution', 'NodeNext', '--allowJs', 'false', wrapper], {cwd: root, encoding: 'utf8'});
      assert.equal(result.status, 0, `${module}\n${result.stdout}\n${result.stderr}`);
    } finally {
      await rm(wrapper, {force: true});
    }
  }
});
