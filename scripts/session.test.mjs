import { readFileSync } from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import ts from 'typescript';
const code = ts.transpileModule(readFileSync(new URL('../src/session.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } }).outputText;
const { initialSession, reconcileSession, sessionBusy, sessionLabel, deliveryMessage } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('late snapshot cannot replace a newer session', () => {
  const current = { ...initialSession, id: 'new', generation: 3, sequence: 2, phase: 'recording' };
  assert.equal(reconcileSession(current, { ...current, id: 'old', generation: 2, sequence: 99 }), current);
});
test('duplicate and out-of-order events do not rewind progress', () => {
  const current = { ...initialSession, generation: 1, sequence: 8, completed: 3 };
  assert.equal(reconcileSession(current, { ...current, sequence: 7, completed: 2 }), current);
  assert.equal(reconcileSession(current, { ...current }), current);
});
test('new session resets final text even when its sequence is lower', () => {
  const current = { ...initialSession, generation: 1, sequence: 99, phase: 'ready', text: 'old' };
  const next = { ...initialSession, generation: 2, sequence: 1, phase: 'recording' };
  assert.equal(reconcileSession(current, next), next);
});
test('stop does not mean ready', () => {
  assert.equal(sessionBusy({ ...initialSession, phase: 'finalizing' }), true);
  assert.equal(sessionBusy({ ...initialSession, phase: 'processing_command' }), true);
  assert.notEqual(sessionLabel({ ...initialSession, phase: 'transcribing' }), 'Texto pronto');
});
test('native input success is not presented as confirmed paste', () => {
  assert.match(deliveryMessage('shortcut_sent'), /Atalho.*enviado/);
  assert.doesNotMatch(deliveryMessage('shortcut_sent'), /colado com sucesso/i);
});
