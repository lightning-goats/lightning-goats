'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../feeder-owner-v2.js'), 'utf8');
function harness() {
  const h = {ms: 1800000000000, on: 0, off: 0, counter: 0, timers: [], rows: [], results: [], cache: new Map()};
  h.state = JSON.stringify({version:'feeder-request-ledger/v2', entries:[]}); h.persisted = h.state; h.commandState = h.state; h.commandPersisted = h.state; h.pending = []; h.commandUpdates = [];
  const shared = {get(k, supplier) {if (!h.cache.has(k) && supplier) h.cache.set(k, supplier()); return h.cache.get(k) ?? null;}, put(k,v) {h.cache.set(k,v);}};
  class AtomicReference { constructor() {this.value=null;} get(){return this.value;} compareAndSet(a,b){if(this.value!==a)return false;this.value=b;return true;} }
  class JavaString {constructor(s){this.s=s;} getBytes(){return Buffer.from(this.s);}}
  const item = {get state(){h.readHook?.(); return h.state;}, postUpdate(s){h.defer(h.postDelay??0,()=>{h.state=s;});}, persistence:{persist(){h.onPersist?.();h.defer(h.persistDelay??0,()=>{if(h.dbFail)return;h.persisted=h.state;h.rows.push(h.state);h.afterPersist?.();});},previousState(){if(h.readbackFail)return null;return h.persisted===null?null:{state:h.persisted};}}};
  const ingress = {get state(){return h.commandState;},postUpdate(s){h.pending.push(()=>{h.commandState=s;});},persistence:{persist(){h.onPersist?.();h.pending.push(()=>{h.commandPersisted=h.commandState;});},previousState(){return {state:h.commandPersisted};}}};
  h.defer=(ticks,fn)=>h.pending.push(()=>{if(ticks>0)h.defer(ticks-1,fn);else fn();});
  h.tick=()=>{h.ms+=h.tickMs??0;const commands=h.commandUpdates.splice(0);const pending=h.pending.splice(0);for(const f of commands)f();for(const f of pending)f();};
  const items = {getItem(name){switch(name){case 'GoatFeeder_ManualRequest':return ingress;
    case 'GoatFeeder_OwnerLedgerV2':return item;
    case 'GoatFeeder_ManualResult':return {postUpdate(s){const r=JSON.parse(s);if(h.notificationFail && r.status==='complete')throw Error('notification');h.results.push(r);}};
    case 'Goat_Plugs_Outlet2_Switch':return {sendCommand(c){if(c==='ON')h.on++;else {h.off++;if(h.offFail)throw Error('off');}}};
    case 'GoatFeedings':return {get state(){return String(h.counter);},postUpdate(s){h.pending.push(()=>{h.counter=Number(s);});}};
    default:throw Error(name);}}};
  const instant=()=>({toEpochMilli:()=>h.ms,toString:()=>new Date(h.ms).toISOString()});
  const now=()=>({toInstant:instant,toString:()=>new Date(h.ms).toISOString(),plusSeconds:()=>now()});
  h.invoke=(id='request-0001', version='feeder-request-v2')=>vm.runInNewContext(source, {require:()=>({items,cache:{shared},time:{ZonedDateTime:{now},toInstant:instant},actions:{ScriptExecution:{createTimer(_at,f){h.timers.push(f);}}}}),Java:{type(n){return {'java.util.concurrent.atomic.AtomicReference':AtomicReference,'java.lang.Object':class {},'java.lang.String':JavaString,'java.nio.charset.StandardCharsets':{UTF_8:1},'java.lang.Thread':{sleep(){h.tick();}}}[n];}},event:{itemName:'GoatFeeder_ManualRequest',receivedCommand:JSON.stringify({version,requestId:id,requestedAt:new Date(h.ms).toISOString()})}});
  h.finish=()=>{h.ms+=1000;h.timers.shift()();};
  h.restart=()=>h.cache.clear();
  h.restore=()=>{h.cache.clear();h.pending=[];h.commandUpdates=[];h.state=h.persisted;};
  return h;
}
test('one ON across overlapping distinct invocations, then two confirmed operations',()=>{const h=harness();h.invoke();h.invoke('request-0002');assert.equal(h.on,1);h.finish();h.invoke();assert.equal(h.on,1);h.ms+=5000;h.invoke('request-0002');h.finish();assert.equal(h.on,2);assert.equal(h.counter,2);assert.equal(JSON.parse(h.persisted).entries.filter(e=>e.status==='complete').length,2);});
test('ownership precedes first ledger read even for reentrant calls',()=>{const h=harness();h.readHook=()=>{h.readHook=null;h.invoke('request-0002');};h.invoke();assert.equal(h.on,1);assert.equal(h.results[0].reason,'busy');});
test('notification failure never downgrades a durable complete receipt',()=>{const h=harness();h.notificationFail=true;h.invoke();h.finish();assert.equal(JSON.parse(h.persisted).entries[0].status,'complete');assert.equal(h.rows.length,2);h.notificationFail=false;h.invoke();assert.equal(h.results.at(-1).status,'complete');assert.equal(h.on,1);assert.equal(h.counter,1);});
test('accepted persistence failure gives zero ON and retains admission hold',()=>{const h=harness();h.dbFail=true;h.invoke();h.dbFail=false;h.ms+=6000;h.invoke('request-0002');assert.equal(h.on,0);h.restart();h.invoke('request-0002');assert.equal(h.on,0);});
test('complete persisted but readback lost never produces a failed ledger',()=>{const h=harness();h.invoke();h.readbackFail=true;h.finish();assert.equal(JSON.parse(h.persisted).entries[0].status,'complete');h.ms+=6000;h.invoke('request-0002');assert.equal(h.on,1);h.readbackFail=false;h.restart();h.invoke();assert.equal(h.results.at(-1).status,'complete');assert.equal(h.on,1);assert.equal(h.counter,1);});
test('restart after ON refuses a distinct UUID',()=>{const h=harness();h.invoke();h.restart();h.ms+=6000;h.invoke('request-0002');h.finish();assert.equal(h.on,1);assert.equal(h.counter,0);assert.equal(JSON.parse(h.persisted).entries[0].status,'accepted');});
test('OFF failure retains unresolved accepted state across restart',()=>{const h=harness();h.invoke();h.offFail=true;h.finish();h.restart();h.ms+=6000;h.invoke('request-0002');assert.equal(h.on,1);assert.equal(h.counter,0);});
test('missing restore, v1 ledger, and wrong request version cannot actuate',()=>{for(const state of ['NULL','UNDEF',JSON.stringify({version:'feeder-request-ledger/v1',entries:[]})]){const h=harness();h.state=state;h.persisted=null;h.invoke();assert.equal(h.on,0);}const h=harness();h.invoke('request-0001','feeder-request-v1');assert.equal(h.on,0);});
test('cooldown refusal does not consume an ID',()=>{const h=harness();h.invoke();h.finish();h.invoke('request-0002');assert.equal(h.on,1);assert.equal(h.results.at(-1).reason,'cooldown');h.ms+=5000;h.invoke('request-0002');assert.equal(h.on,2);});

test('terminal receipt capacity is reserved before ON',()=>{const h=harness();let denied=false;for(let i=0;i<32;i++){const before=h.on;h.invoke('request-'+String(i).padStart(3,'0')+'x'.repeat(103));if(h.on===before){assert.equal(h.results.at(-1).reason,'ledger_full');denied=true;break;}h.finish();assert.equal(JSON.parse(h.persisted).entries[0].status,'complete');h.ms+=5000;}assert.equal(denied,true);assert.equal(h.on,h.counter);});


test('late command prediction cannot overwrite admitted or completed durable ledger',()=>{
  const h=harness();
  h.onPersist=()=>h.commandUpdates.push(()=>{h.commandState=JSON.stringify({version:'feeder-request-v2',requestId:'request-0001',requestedAt:new Date(h.ms).toISOString()});});
  h.invoke();
  assert.equal(h.on,1);
  assert.equal(JSON.parse(h.persisted).entries[0].status,'accepted');
  h.finish();
  assert.equal(JSON.parse(h.persisted).entries[0].status,'complete');
  assert.equal(JSON.parse(h.commandState).requestId,'request-0001');
  h.restart();h.invoke();
  assert.equal(h.on,1);
  assert.equal(h.results.at(-1).status,'complete');
});

test('33rd unique request fails closed and oldest UUID remains remembered after restart',()=>{
  const h=harness();
  for(let i=0;i<32;i++) {h.invoke('req-'+String(i).padStart(4,'0'));assert.equal(h.on,i+1);h.finish();h.ms+=5000;}
  const before=h.persisted;
  h.invoke('req-0032');
  assert.equal(h.results.at(-1).reason,'ledger_full');
  assert.equal(h.on,32);assert.equal(h.persisted,before);
  h.restart();h.ms+=86400000;h.invoke('req-0000');
  assert.equal(h.on,32);assert.equal(h.results.at(-1).status,'complete');
  assert.equal(h.persisted,before);
});

test('unresolved admission is never evicted even after the freshness horizon',()=>{
  const h=harness();h.invoke();const before=h.persisted;
  h.restart();h.ms+=86400000;h.invoke('request-0002');
  assert.equal(h.on,1);assert.equal(h.persisted,before);
  assert.equal(h.results.at(-1).reason,'restart_uncertain');
});


test('delayed Item update and asynchronous JDBC capture both finish before ON',()=>{
  const h=harness();h.postDelay=3;h.persistDelay=3;
  h.invoke();assert.equal(h.on,1);assert.equal(JSON.parse(h.persisted).entries[0].status,'accepted');
  h.finish();assert.equal(h.counter,1);assert.equal(JSON.parse(h.persisted).entries[0].status,'complete');
});

test('process loss after durable admission restores accepted and blocks new work',()=>{
  const h=harness();h.afterPersist=()=>{throw Error('simulated process loss');};
  h.invoke();assert.equal(h.on,0);assert.equal(JSON.parse(h.persisted).entries[0].status,'accepted');
  h.afterPersist=null;h.restore();h.ms+=6000;h.invoke('request-0002');assert.equal(h.on,0);
  assert.equal(h.results.at(-1).reason,'restart_uncertain');
});

test('restoring after completion loses result notification but preserves duplicate receipt',()=>{
  const h=harness();h.notificationFail=true;h.invoke();h.finish();
  h.restore();h.results=[];h.notificationFail=false;h.invoke();
  assert.equal(h.on,1);assert.equal(h.results.at(-1).status,'complete');
});

for (const restored of [false, true]) test(`slow admission preserves actual start cooldown (restore=${restored})`,()=>{
  const h=harness();const initial=h.ms;h.tickMs=50;h.postDelay=15;h.persistDelay=15;
  h.invoke();const firstStart=h.ms;assert.equal(h.on,1);h.finish();
  h.postDelay=0;h.persistDelay=0;if(restored)h.restore();
  h.ms=initial+5000;h.invoke('request-0002');
  assert.equal(h.on,1);assert.equal(h.results.at(-1).reason,'cooldown');
  h.ms=Math.max(firstStart+5000,Date.parse(JSON.parse(h.persisted).entries[0].updatedAt)+5000);
  h.invoke('request-0002');assert.equal(h.on,2);
});
