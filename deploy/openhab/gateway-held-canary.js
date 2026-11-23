'use strict';
// Separate, unlinked acceptance fixture. No physical bindings or external I/O.
// The journal records every delivered request; release never delivers a request.
const {items, cache, time} = require('openhab');
const PREFIX = 'LightningGoatsHeldCanary2';
const lock = cache.shared.get('lightning-goats.held-canary2.lock',
  () => new (Java.type('java.util.concurrent.locks.ReentrantLock'))());
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const MAX_ROWS = 128;
const MAX_BYTES = 65536;
const Thread = Java.type('java.lang.Thread');
function item(suffix) { return items.getItem(PREFIX + suffix); }
function stamp() { return time.ZonedDateTime.now().toInstant().toString(); }
function sameState(actual, expected, numeric) {
  if (!numeric) return actual === expected;
  return /^(0|[1-9][0-9]*)(?:\.0+)?$/.test(actual)
    && Number.isSafeInteger(Number(actual)) && Number(actual) === Number(expected);
}
function persist(target, value, numeric = false) {
  target.postUpdate(value);
  let visible = false;
  for (let i = 0; i < 20; i++) {
    Thread.sleep(50);
    if (sameState(target.state.toString(), value, numeric)) { visible = true; break; }
  }
  if (!visible) throw Error('state update uncertain');
  target.persistence.persist('jdbc');
  for (let i = 0; i < 20; i++) {
    const row = target.persistence.previousState(false, 'jdbc');
    if (row !== null && sameState(row.state.toString(), value, numeric)) return;
    Thread.sleep(50);
  }
  throw Error('persistence uncertain');
}
function readJournal() {
  const target = item('Journal');
  const raw = target.state.toString();
  const previous = target.persistence.previousState(false, 'jdbc');
  if (raw.length > MAX_BYTES || previous === null || previous.state.toString() !== raw) {
    throw Error('journal restore conflict');
  }
  const journal = JSON.parse(raw);
  if (journal.version !== 'held-canary/v1' || !Array.isArray(journal.deliveries)
      || journal.deliveries.length > MAX_ROWS) throw Error('journal invalid');
  for (let i = 0; i < journal.deliveries.length; i++) {
    const row = journal.deliveries[i];
    if (row.sequence !== i + 1 || !['held','released','invalid'].includes(row.status)
        || (row.requestId !== null && !UUID.test(row.requestId))
        || !Number.isFinite(Date.parse(row.receivedAt))
        || (row.status === 'invalid') !== (row.requestId === null)
        || (row.status === 'released' && !Number.isFinite(Date.parse(row.releasedAt)))) {
      throw Error('journal row invalid');
    }
  }
  return journal;
}
function save(journal) {
  const raw = JSON.stringify(journal);
  if (raw.length > MAX_BYTES) throw Error('journal full');
  persist(item('Journal'), raw);
}
function run() {
  const source = String(event.itemName);
  if (![PREFIX+'Request', PREFIX+'Release'].includes(source)) return;
  const command = String(event.receivedCommand ?? event.itemCommand).trim();
  lock.lock();
  try {
    if (source === PREFIX+'Request') {
      // Independent monotonically increasing delivery counter, before validation.
      const countItem = item('Count');
      const currentCount = countItem.state.toString();
      const savedCount = countItem.persistence.previousState(false, 'jdbc');
      if (savedCount === null || !sameState(savedCount.state.toString(), currentCount, true)) throw Error('counter restore conflict');
      const count = Number(currentCount);
      if (!Number.isSafeInteger(count) || count < 0 || count >= Number.MAX_SAFE_INTEGER) {
        throw Error('counter invalid');
      }
      persist(item('Count'), String(count + 1), true);
    }
    const fault = item('Fault');
    const savedFault = fault.persistence.previousState(false, 'jdbc');
    if (fault.state.toString() !== 'OFF' || savedFault === null || savedFault.state.toString() !== 'OFF') throw Error('fixture fault');
    const journal = readJournal();
    const expectedCount = journal.deliveries.length + (source === PREFIX+'Request' ? 1 : 0);
    if (Number(item('Count').state.toString()) !== expectedCount) throw Error('delivery journal gap');
    if (source === PREFIX+'Request') {
      if (journal.deliveries.length >= MAX_ROWS) throw Error('journal full');
      const valid = UUID.test(command);
      const hold = item('Hold').state.toString();
      if (!['ON','OFF'].includes(hold)) throw Error('hold invalid');
      const released = valid && (hold === 'OFF'
        || journal.deliveries.some(row => row.requestId === command && row.status === 'released'));
      const row = {sequence:journal.deliveries.length+1, requestId:valid?command:null,
        status:valid?(released?'released':'held'):'invalid', receivedAt:stamp()};
      if (released) row.releasedAt = stamp();
      journal.deliveries.push(row);
      save(journal);
      if (released) item('Ack').postUpdate(command);
    } else {
      if (!UUID.test(command)) throw Error('release UUID invalid');
      const matching = journal.deliveries.filter(row => row.requestId === command);
      if (matching.length === 0) throw Error('release UUID absent');
      for (const row of matching) {
        if (row.status === 'held') { row.status = 'released'; row.releasedAt = stamp(); }
      }
      save(journal);
      item('Ack').postUpdate(command);
    }
  } catch (_) {
    // Sticky fault: no automatic clearing or new acknowledgement on uncertainty.
    try { persist(item('Fault'), 'ON'); } catch (_) { item('Fault').postUpdate('ON'); }
  } finally { lock.unlock(); }
}
run();
