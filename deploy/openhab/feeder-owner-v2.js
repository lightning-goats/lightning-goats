'use strict';

const { actions, cache, items, time } = require('openhab');

const EARTHSHIP_FEEDER_OWNER_VERSION = 'feeder-request-v2';
const LEDGER_VERSION = 'feeder-request-ledger/v2';
const REQUEST_ITEM = 'GoatFeeder_ManualRequest';
const RESULT_ITEM = 'GoatFeeder_ManualResult';
const ACTUATOR_ITEM = 'Goat_Plugs_Outlet2_Switch';
const COUNTER_ITEM = 'GoatFeedings';
const BUSY_KEY = 'earthship.feeder-owner.v2.guard';
const AtomicReference = Java.type('java.util.concurrent.atomic.AtomicReference');
const JavaObject = Java.type('java.lang.Object');
const guard = cache.shared.get(BUSY_KEY, () => new AtomicReference());
const LAST_START_KEY = 'earthship.feeder-owner.last-start-ms';
const MAX_LEDGER_BYTES = 8192;
const MAX_LEDGER_ENTRIES = 32;
const COOLDOWN_MS = 5000;
const MAX_REQUEST_AGE_MS = 2 * 60 * 1000;
const MAX_REQUEST_FUTURE_SKEW_MS = 30 * 1000;
const LEDGER_READBACK_ATTEMPTS = 20;
const LEDGER_READBACK_POLL_MS = 50;
const NULL_STATES = new Set(['NULL', 'UNDEF']);
const TERMINAL_STATUSES = new Set(['complete', 'failed', 'denied']);

function now() {
  return time.ZonedDateTime.now();
}

function nowText() {
  return now().toString();
}

// UTC instant string (ends in 'Z'); strict-parseable by real Instant.parse().
// All durable ledger and result timestamps are written in this form so that a
// production readback never re-parses a bracketed ZonedDateTime rendering.
function nowInstantText() {
  return now().toInstant().toString();
}

function nowMillis() {
  return epochMillis(now());
}

// Real time.ZonedDateTime.now().toString() ends in a bracketed zone id
// (e.g. ...-06:00[America/Denver]) and openHAB renders DateTime item states
// with colon-less offsets (e.g. ...-0600); real time.toInstant()/Instant.parse()
// is strict and accepts only 'Z' instants. Normalize both non-Z forms so any
// historical, bracketed, or item-state timestamp still parses. Belt-and-braces
// to the instant-form writes above.
function normalizeTimestamp(value) {
  return String(value)
    .replace(/\[[^\]]+\]$/, '')
    .replace(/([+-]\d{2})(\d{2})$/, '$1:$2');
}

function epochMillis(value) {
  if (value && typeof value.toInstant === 'function') {
    try {
      return Number(time.toInstant(value).toEpochMilli());
    } catch {
      return Number.NaN;
    }
  }
  const ms = Date.parse(normalizeTimestamp(value));
  return Number.isFinite(ms) ? ms : Number.NaN;
}

function postResult(requestId, status, reason, at = nowInstantText()) {
  items.getItem(RESULT_ITEM).postUpdate(JSON.stringify({
    version: EARTHSHIP_FEEDER_OWNER_VERSION,
    requestId,
    status,
    reason,
    at,
  }));
}

function settleItemUpdate(delayMs = LEDGER_READBACK_POLL_MS) {
  if (typeof Java !== 'undefined') {
    Java.type('java.lang.Thread').sleep(delayMs);
  } else if (typeof java !== 'undefined') {
    java.lang.Thread.sleep(delayMs);
  }
}

function utf8Bytes(value) {
  if (typeof Java !== 'undefined') {
    const JavaString = Java.type('java.lang.String');
    const StandardCharsets = Java.type('java.nio.charset.StandardCharsets');
    return new JavaString(String(value)).getBytes(StandardCharsets.UTF_8).length;
  }
  if (typeof java !== 'undefined') {
    return new java.lang.String(String(value))
      .getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
  }
  return new TextEncoder().encode(String(value)).length;
}

function parseRequest(raw) {
  if (typeof raw !== 'string' || raw.length === 0 || raw.length > 2048) {
    throw new Error('request_invalid');
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error('request_invalid');
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('request_invalid');
  }
  if (
    typeof parsed.requestId !== 'string'
    || !/^[A-Za-z0-9][A-Za-z0-9._:-]{7,127}$/.test(parsed.requestId)
  ) {
    throw new Error('request_invalid');
  }
  if (
    (typeof parsed.requestedAt !== 'string' || !Number.isFinite(epochMillis(parsed.requestedAt)))
  ) {
    throw new Error('request_invalid');
  }
  if (parsed.version !== EARTHSHIP_FEEDER_OWNER_VERSION) throw new Error('request_version');
  return {
    requestId: parsed.requestId,
    requestedAt: parsed.requestedAt,
    requestedAtMs: epochMillis(parsed.requestedAt),
  };
}

function validLedgerEntry(entry) {
  return Boolean(
    entry
    && typeof entry === 'object'
    && !Array.isArray(entry)
    && entry.version === EARTHSHIP_FEEDER_OWNER_VERSION
    && typeof entry.requestId === 'string'
    && /^[A-Za-z0-9][A-Za-z0-9._:-]{7,127}$/.test(entry.requestId)
    && typeof entry.status === 'string'
    && ['accepted', 'running', ...TERMINAL_STATUSES].includes(entry.status)
    && typeof entry.reason === 'string'
    && typeof entry.at === 'string'
    && Number.isFinite(epochMillis(entry.at))
    && (entry.updatedAt === undefined || (
      typeof entry.updatedAt === 'string' && Number.isFinite(epochMillis(entry.updatedAt))
    ))
  );
}

function parseLedger(requestItem) {
  const raw = requestItem.state.toString();
  if (NULL_STATES.has(raw)) {
    throw new Error('ledger_restore_missing');
  }
  if (raw.length === 0 || utf8Bytes(raw) > MAX_LEDGER_BYTES) {
    throw new Error('ledger_invalid');
  }
  let ledger;
  try {
    ledger = JSON.parse(raw);
  } catch {
    throw new Error('ledger_invalid');
  }
  const uniqueIds = new Set(ledger?.entries?.map((entry) => entry?.requestId));
  if (
    !ledger
    || typeof ledger !== 'object'
    || Array.isArray(ledger)
    || ledger.version !== LEDGER_VERSION
    || !Array.isArray(ledger.entries)
    || ledger.entries.length > MAX_LEDGER_ENTRIES
    || uniqueIds.size !== ledger.entries.length
    || !ledger.entries.every(validLedgerEntry)
  ) {
    throw new Error('ledger_invalid');
  }
  const persisted = requestItem.persistence.previousState(false, 'jdbc');
  if (persisted === null || persisted.state.toString() !== raw) {
    throw new Error('ledger_restore_conflict');
  }
  return ledger;
}

function writeLedger(requestItem, ledger, requestId, expectedStatus) {
  const encoded = JSON.stringify({
    version: LEDGER_VERSION,
    entries: ledger.entries.slice(0, MAX_LEDGER_ENTRIES),
  });
  if (utf8Bytes(encoded) > MAX_LEDGER_BYTES) throw new Error('ledger_invalid');
  requestItem.postUpdate(encoded);
  settleItemUpdate();
  requestItem.persistence.persist('jdbc');

  for (let attempt = 0; attempt < LEDGER_READBACK_ATTEMPTS; attempt += 1) {
    try {
      const readback = JSON.parse(requestItem.state.toString());
      const previous = requestItem.persistence.previousState(false, 'jdbc');
      const persistedReadback = previous === null
        ? null
        : JSON.parse(previous.state.toString());
      if (
        readback?.version === LEDGER_VERSION
        && readback.entries?.[0]?.requestId === requestId
        && readback.entries?.[0]?.status === expectedStatus
        && persistedReadback?.version === LEDGER_VERSION
        && persistedReadback.entries?.[0]?.requestId === requestId
        && persistedReadback.entries?.[0]?.status === expectedStatus
        && JSON.stringify(readback) === encoded
        && JSON.stringify(persistedReadback) === encoded
      ) return;
    } catch {
      // Persistence is asynchronous; retry only until the bounded deadline.
    }
    if (attempt + 1 < LEDGER_READBACK_ATTEMPTS) settleItemUpdate();
  }
  throw new Error('ledger_readback_failed');
}

function acceptedLedger(ledger, requestId, at) {
  return {
    version: LEDGER_VERSION,
    entries: [{
      version: EARTHSHIP_FEEDER_OWNER_VERSION,
      requestId,
      status: 'accepted',
      reason: 'accepted',
      at,
    }, ...ledger.entries.filter((entry) => entry.requestId !== requestId)].slice(0, MAX_LEDGER_ENTRIES),
  };
}

function terminalLedger(ledger, requestId, status, reason, updatedAt) {
  return {
    version: LEDGER_VERSION,
    entries: ledger.entries.map((entry) => (
      entry.requestId === requestId
        ? { ...entry, status, reason, updatedAt }
        : entry
    )).slice(0, MAX_LEDGER_ENTRIES),
  };
}

function releaseBusy(token) {
  guard.compareAndSet(token, null);
}

function safeOff(actuator) {
  if (!actuator) return;
  try {
    actuator.sendCommand('OFF');
  } catch {
    // The preserved one-second expire metadata is the independent OFF backstop.
  }
}

function safeResult(requestId, status, reason) {
  try {
    postResult(requestId, status, reason);
  } catch {
    // The durable request ledger remains the authoritative receipt.
  }
}

// Candidate only: activate through a separately reviewed disabled-feeder migration.
// No legacy uncorrelated trigger is permitted to bypass durable admission.
function runFeederOwner(triggerEvent) {
  if (!triggerEvent || triggerEvent.itemName !== REQUEST_ITEM
      || typeof triggerEvent.receivedCommand !== 'string') return;
  let request;
  try { request = parseRequest(triggerEvent.receivedCommand); }
  catch (error) { safeResult('unknown', 'denied', error.message); return; }

  // Java identity, not UUID/time equality; the token survives the asynchronous timer.
  const token = new JavaObject();
  if (!guard.compareAndSet(null, token)) {
    safeResult(request.requestId, 'denied', 'busy');
    return;
  }
  let requestItem;
  let ledger;
  let currentMs;
  try {
    currentMs = nowMillis();
    if (!Number.isFinite(currentMs)) throw new Error('clock_invalid');
    requestItem = items.getItem(REQUEST_ITEM);
    ledger = parseLedger(requestItem);
    const previous = ledger.entries.find(e => e.requestId === request.requestId);
    if (previous) {
      // Exact durable v2 receipt replay is notification only, never actuation.
      safeResult(previous.requestId, previous.status, previous.reason);
      releaseBusy(token);
      return;
    }
    if (ledger.entries.some(e => e.status !== 'complete')) {
      throw new Error('restart_uncertain');
    }
    if (currentMs - request.requestedAtMs > MAX_REQUEST_AGE_MS
        || request.requestedAtMs - currentMs > MAX_REQUEST_FUTURE_SKEW_MS) {
      throw new Error('request_stale');
    }
    // No silent eviction of an idempotency key. Retention expansion is a separate review.
    if (ledger.entries.length >= MAX_LEDGER_ENTRIES) throw new Error('ledger_full');
    const durableStart = ledger.entries.length ? epochMillis(ledger.entries[0].at) : -Infinity;
    const cachedStart = cache.shared.get(LAST_START_KEY);
    const lastStart = Math.max(durableStart, cachedStart === null ? -Infinity : Number(cachedStart));
    if (!Number.isFinite(lastStart) && lastStart !== -Infinity) throw new Error('clock_invalid');
    if (currentMs - lastStart < COOLDOWN_MS) throw new Error('cooldown');
  } catch (error) {
    safeResult(request.requestId, 'denied', error.message);
    releaseBusy(token);
    return;
  }

  ledger = acceptedLedger(ledger, request.requestId, nowInstantText());
  // Reserve terminal timestamp headroom before any durable admission or ON.
  const completionBudget = terminalLedger(ledger, request.requestId, 'complete', 'complete', 'X'.repeat(64));
  if (utf8Bytes(JSON.stringify(completionBudget)) > MAX_LEDGER_BYTES) {
    safeResult(request.requestId, 'denied', 'ledger_full');
    releaseBusy(token);
    return;
  }
  try { writeLedger(requestItem, ledger, request.requestId, 'accepted'); }
  catch {
    // May have committed. Keep ownership held; no ON and no automatic retry.
    safeResult(request.requestId, 'failed', 'ledger_persist_uncertain');
    return;
  }

  let actuator;
  const fail = () => {
    safeOff(actuator);
    // Accepted remains unresolved, including after restart. Never rewrite completion.
    safeResult(request.requestId, 'failed', 'execution_error');
  };
  try {
    actuator = items.getItem(ACTUATOR_ITEM);
    const counter = items.getItem(COUNTER_ITEM);
    const before = Number(counter.state.toString());
    if (!Number.isSafeInteger(before) || before < 0 || before === Number.MAX_SAFE_INTEGER) {
      throw new Error('counter_invalid');
    }
    cache.shared.put(LAST_START_KEY, String(currentMs));
    safeResult(request.requestId, 'accepted', 'accepted');
    actuator.sendCommand('ON');
    safeResult(request.requestId, 'running', 'pulse_started');
    actions.ScriptExecution.createTimer(now().plusSeconds(1), () => {
      // An unloaded/replaced cache owner cannot authorize an old callback.
      if (cache.shared.get(BUSY_KEY) !== guard || guard.get() !== token) {
        safeOff(actuator);
        return;
      }
      try {
        actuator.sendCommand('OFF');
        if (Number(counter.state.toString()) !== before) throw new Error('counter_changed');
        counter.postUpdate(String(before + 1));
        settleItemUpdate();
        if (Number(counter.state.toString()) !== before + 1) throw new Error('counter_receipt_failed');
      } catch { fail(); return; }

      // This candidate is final once constructed. Persistence uncertainty and
      // notification failure must never reach an actuation-failure writer.
      const complete = terminalLedger(ledger, request.requestId, 'complete', 'complete', nowInstantText());
      try { writeLedger(requestItem, complete, request.requestId, 'complete'); }
      catch {
        safeResult(request.requestId, 'failed', 'completion_persist_uncertain');
        return; // bounded work; hold until separately verified restore/reconciliation
      }
      safeResult(request.requestId, 'complete', 'complete');
      releaseBusy(token);
    });
  } catch { fail(); }
}

runFeederOwner(typeof event === 'undefined' ? null : event);
