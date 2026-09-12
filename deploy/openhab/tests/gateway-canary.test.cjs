const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const script = fs.readFileSync(path.join(__dirname, '..', 'gateway-canary.js'), 'utf8');
const uuid = '12345678-1234-4234-8234-123456789abc';
function fixture() {
  const state = { LightningGoatsCanaryCount: '0', LightningGoatsCanaryAck: 'NULL' };
  const cached = new Map();
  return { state, run(command) {
    vm.runInNewContext(script, { Java: { type(name) { assert.equal(name, 'java.util.concurrent.atomic.AtomicLong'); return class { constructor(n) { this.n = n; } incrementAndGet() { return ++this.n; } }; } }, event: { itemCommand: command }, require(name) {
      assert.equal(name, 'openhab');
      return { cache: { private: { get(key, supplier) { if (!cached.has(key)) cached.set(key, supplier()); return cached.get(key); } } }, items: { getItem(key) {
        assert.ok(Object.hasOwn(state, key), `unexpected Item ${key}`);
        return { state: state[key], postUpdate(value) { state[key] = value; } };
      } } };
    } });
  } };
}
test('same UUID twice counts two deliveries, exposing gateway resend', () => {
  const f = fixture(); f.run(uuid); f.run(uuid);
  assert.equal(f.state.LightningGoatsCanaryCount, '2');
  assert.equal(f.state.LightningGoatsCanaryAck, uuid);
});
test('invalid command counted but never acknowledged', () => {
  const f = fixture(); f.run('ON');
  assert.equal(f.state.LightningGoatsCanaryCount, '1');
  assert.equal(f.state.LightningGoatsCanaryAck, 'NULL');
});
test('unknown counter refuses rather than silently resetting evidence', () => {
  const f = fixture(); f.state.LightningGoatsCanaryCount = 'NULL';
  assert.throws(() => f.run(uuid));
  assert.equal(f.state.LightningGoatsCanaryAck, 'NULL');
});
