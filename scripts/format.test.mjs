import { readFileSync } from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import ts from 'typescript';
const code = ts.transpileModule(readFileSync(new URL('../src/format.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } }).outputText;
const { money, recordedCost, duration, rangeStart } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('unknown charges and tiny paid calls are never displayed as free', () => {
  assert.equal(money(null), 'Não informado');
  assert.equal(money(600), '< US$ 0,000001');
  assert.equal(money(2050000), 'US$ 0,00205');
  assert.equal(money(0), 'US$ 0,00');
  assert.equal(recordedCost(0, 2, 2), 'Não informado');
  assert.equal(recordedCost(0, 0, 0), 'US$ 0,00');
  assert.equal(recordedCost(600, 2, 1), '< US$ 0,000001');
});
test('local time ranges include the first day of the selected period', () => {
  const now = new Date(2026, 9, 17, 18, 20);
  assert.equal(rangeStart('month', now), new Date(2026, 9, 1).getTime());
  assert.equal(rangeStart('year', now), new Date(2026, 0, 1).getTime());
  assert.equal(rangeStart('today', now), new Date(2026, 9, 17).getTime());
  assert.equal(rangeStart('all', now), 0);
  assert.equal(duration(297000), '4:57');
});
