'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../feeder-owner-v2.js'), 'utf8');
function harness() {
  const h = {ms: 1800000000000, on: 0, off: 0, counter: 0, timers: [], rows: [], results: [], cache: new Map()};
  h.state = JSON.stringify({version:'feeder-request-ledger/v2', entries:[]}); h.persisted = h.state;
  const shared = {get(k, supplier) {if (!h.cache.has(k) && supplier) h.cache.set(k, supplier()); return h.cache.get(k) ?? null;}, put(k,v) {h.cache.set(k,v);}};
  class AtomicReference { constructor() {this.value=null;} get(){return this.value;} compareAndSet(a,b){if(this.value!==a)return false;this.value=b;return true;} }
  class JavaString {constructor(s){this.s=s;} getBytes(){return Buffer.from(this.s);}}
  const item = {get state(){h.readHook?.(); return h.state;}, postUpdate(s){h.state=s;}, persistence:{persist(){if(h.dbFail)throw Error('db');h.persisted=h.state;h.rows.push(h.state);},previousState(){if(h.readbackFail)return null;return h.persisted===null?null:{state:h.persisted};}}};
  const items = {getItem(name){switch(name){case 'GoatFeeder_ManualRequest':return item;
    case 'GoatFeeder_ManualResult':return {postUpdate(s){const r=JSON.parse(s);if(h.notificationFail && r.status==='complete')throw Error('notification');h.results.push(r);}};
    case 'Goat_Plugs_Outlet2_Switch':return {sendCommand(c){if(c==='ON')h.on++;else {h.off++;if(h.offFail)throw Error('off');}}};
    case 'GoatFeedings':return {get state(){return String(h.counter);},postUpdate(s){h.counter=Number(s);}};
    default:throw Error(name);}}};
  const instant=()=>({toEpochMilli:()=>h.ms,toString:()=>new Date(h.ms).toISOString()});
  const now=()=>({toInstant:instant,toString:()=>new Date(h.ms).toISOString(),plusSeconds:()=>now()});
  h.invoke=(id='request-0001', version='feeder-request-v2')=>vm.runInNewContext(source, {require:()=>({items,cache:{shared},time:{ZonedDateTime:{now},toInstant:instant},actions:{ScriptExecution:{createTimer(_at,f){h.timers.push(f);}}}}),Java:{type(n){return {'java.util.concurrent.atomic.AtomicReference':AtomicReference,'java.lang.Object':class {},'java.lang.String':JavaString,'java.nio.charset.StandardCharsets':{UTF_8:1},'java.lang.Thread':{sleep(){}}}[n];}},event:{itemName:'GoatFeeder_ManualRequest',receivedCommand:JSON.stringify({version,requestId:id,requestedAt:new Date(h.ms).toISOString()})}});
  h.finish=()=>{h.ms+=1000;h.timers.shift()();};
  h.restart=()=>h.cache.clear();
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
