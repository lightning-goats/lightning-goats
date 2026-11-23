'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname,'../gateway-held-canary.js'),'utf8');
const ID = '00000000-0000-4000-8000-000000000001';
function fixture() {
  const h={states:{Count:'0',Journal:JSON.stringify({version:'held-canary/v1',deliveries:[]}),Fault:'OFF',Hold:'ON',Ack:'NULL'},cache:new Map(),acks:[],pending:[],ms:1800000000000};
  h.saved={...h.states};
  const shared={get(k,supplier){if(!h.cache.has(k))h.cache.set(k,supplier());return h.cache.get(k);}};
  const items={getItem(name){assert.ok(name.startsWith('LightningGoatsHeldCanary2'));const key=name.slice('LightningGoatsHeldCanary2'.length);assert.ok(key in h.states);return {
    get state(){return h.states[key];},postUpdate(value){h.pending.push(()=>{h.states[key]=value;if(key==='Ack')h.acks.push(value);});},
    persistence:{persist(){h.pending.push(()=>{if(h.fail!==key)h.saved[key]=key==='Count'&&h.numericJdbc?h.states[key]+'.0':h.states[key];});},previousState(){return h.saved[key]===undefined?null:{state:h.saved[key]};}}
  };}};
  h.tick=()=>{h.ms+=50;for(const f of h.pending.splice(0))f();};
  class Lock {lock(){assert.ok(!h.locked);h.locked=true;}unlock(){h.locked=false;}}
  h.invoke=(suffix='Request',command=ID)=>{vm.runInNewContext(source,{require:()=>({items,cache:{shared},time:{ZonedDateTime:{now:()=>({toInstant:()=>({toString:()=>new Date(h.ms).toISOString()})})}}}),Java:{type(n){if(n==='java.util.concurrent.locks.ReentrantLock')return Lock;if(n==='java.lang.Thread')return {sleep:h.tick};throw Error(n);}},event:{itemName:'LightningGoatsHeldCanary2'+suffix,receivedCommand:command}});h.tick();};
  h.restore=()=>{h.states={...h.saved};h.pending=[];h.cache.clear();};
  h.rows=()=>JSON.parse(h.saved.Journal).deliveries;
  return h;
}
test('held request remains unacknowledged until exact UUID release without another delivery',()=>{
 const h=fixture();h.invoke();assert.equal(h.states.Count,'1');assert.equal(h.acks.length,0);assert.equal(h.rows()[0].status,'held');
 h.invoke('Release');assert.deepEqual(h.acks,[ID]);assert.equal(h.states.Count,'1');assert.equal(h.rows()[0].status,'released');
 h.invoke('Release');assert.equal(h.states.Count,'1');assert.equal(h.rows().length,1);
});
test('every duplicate delivery is counted and ordered, never deduplicated out of evidence',()=>{
 const h=fixture();h.invoke();h.invoke();assert.equal(h.states.Count,'2');assert.deepEqual(h.rows().map(r=>r.sequence),[1,2]);assert.equal(h.acks.length,0);
 h.invoke('Release');assert.ok(h.rows().every(r=>r.status==='released'));h.invoke();assert.equal(h.states.Count,'3');assert.equal(h.rows().length,3);assert.equal(h.acks.length,2);
});
test('held and released receipts survive restored state with no synthetic request',()=>{
 const h=fixture();h.invoke();h.restore();h.invoke('Release');assert.equal(h.states.Count,'1');assert.equal(h.rows()[0].status,'released');
 h.restore();h.invoke('Release');assert.equal(h.states.Count,'1');assert.deepEqual(h.acks,[ID,ID]);
});
test('journal persistence uncertainty faults without acknowledgment and survives restore',()=>{
 const h=fixture();h.fail='Journal';h.invoke();assert.equal(h.acks.length,0);assert.equal(h.saved.Fault,'ON');assert.equal(h.saved.Count,'1');
 h.restore();h.fail=null;h.invoke('Release');assert.equal(h.acks.length,0);assert.equal(h.saved.Count,'1');
});
test('counter/journal crash gap cannot resume as an empty fresh fixture',()=>{
 const h=fixture();h.saved.Count='1';h.restore();h.invoke();assert.equal(h.states.Fault,'ON');assert.equal(h.acks.length,0);assert.equal(h.rows().length,0);
});
test('malformed requests are counted and recorded; unknown release cannot acknowledge',()=>{
 const h=fixture();h.invoke('Request','not-a-uuid');assert.equal(h.states.Count,'1');assert.equal(h.rows()[0].status,'invalid');assert.equal(h.rows()[0].requestId,null);
 h.invoke('Release');assert.equal(h.states.Fault,'ON');assert.equal(h.acks.length,0);
});
test('automatic completion requires explicit Hold OFF and durable journal before acknowledgment',()=>{
 const h=fixture();h.states.Hold='OFF';h.invoke();assert.deepEqual(h.acks,[ID]);assert.equal(h.rows()[0].status,'released');
 const failed=fixture();failed.states.Hold='OFF';failed.fail='Journal';failed.invoke();assert.equal(failed.acks.length,0);
});
test('capacity never silently evicts delivery evidence',()=>{
 const h=fixture();for(let i=0;i<128;i++)h.invoke();const before=h.saved.Journal;h.invoke();assert.equal(h.states.Count,'129');assert.equal(h.states.Fault,'ON');assert.equal(h.saved.Journal,before);assert.equal(h.acks.length,0);
});

test('JDBC DecimalType numeric representation preserves exact integer counts',()=>{
 const h=fixture();h.numericJdbc=true;h.saved.Count='0.0';h.invoke();
 assert.equal(h.states.Fault,'OFF');assert.equal(h.states.Count,'1');assert.equal(h.rows().length,1);
 h.invoke('Release');assert.deepEqual(h.acks,[ID]);
});
