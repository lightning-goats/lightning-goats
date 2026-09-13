// Harmless fixture: only four fixed, unlinked LightningGoatsCanary Items.
// Count EVERY delivered command before validation; no deduplication hides resend.
// Failure/delay injection belongs in offline fixtures, not the household runtime.
const { items, cache } = require('openhab');
const count = items.getItem('LightningGoatsCanaryCount');
const counter = cache.private.get('delivery-count', () => {
  const previous = Number(count.state);
  if (!Number.isSafeInteger(previous) || previous < 0) throw new Error('Invalid canary count');
  return new (Java.type('java.util.concurrent.atomic.AtomicLong'))(previous);
});
count.postUpdate(String(counter.incrementAndGet()));
const id = String(event.itemCommand).trim();
if (/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id)) {
  items.getItem('LightningGoatsCanaryAck').postUpdate(id);
}
