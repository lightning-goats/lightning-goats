// Offline independent-library parsing only; no wallet, mint or relay operations.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const [installation, fixturePath] = process.argv.slice(2);
assert(installation && fixturePath && process.argv.length === 4,
    'Usage: node verify-requests.mjs <prepared-directory> <request-fixtures.json>');
const packageRoot=resolve(installation,'node_modules/@cashu/cashu-ts');
assert.equal(JSON.parse(readFileSync(resolve(packageRoot,'package.json'))).version,'4.7.2');
// Fail any unexpected runtime network use, independently of netns containment.
globalThis.fetch=()=>{throw new Error('network forbidden in offline request fixture');};
const {PaymentRequest}=await import(pathToFileURL(resolve(packageRoot,'lib/cashu-ts.es.js')));
const fixtures=JSON.parse(readFileSync(fixturePath));
const results=[];
for(const [name,fixture] of Object.entries(fixtures.requests)) {
    try {
        const request=PaymentRequest.fromEncodedRequest(fixture.request);
        assert.equal(request.id,fixture.id);
        assert.equal(request.amount,undefined);
        assert.equal(request.unit,'sat');
        assert.equal(request.singleUse,false);
        const long=name.startsWith('long_');
        assert.deepEqual(request.mints,long
            ? Array.from({length:4},(_,i)=>`https://mint-${i}-compatibility-fixture.example`)
            : ['https://mint-one.example','https://mint-two.example']);
        assert.equal(request.transport.length,1);
        assert.equal(request.transport[0].type,'post');
        assert.equal(request.transport[0].target,long
            ? 'https://lightning-goats.example/'+'request-fixture-'.repeat(6)+'cashu'
            : 'https://lightning-goats.example/c');
        assert.equal(request.description,'Lightning Goats — shared herd contribution');
        results.push({name,status:'parsed',encoding:fixture.encoding,payload_length:fixture.request.length});
    }catch(error){results.push({name,status:'failed',reason:error.message});process.exitCode=1;}
}
console.log(JSON.stringify({library:'@cashu/cashu-ts',version:'4.7.2',results,
    scope:'Independent offline parsing only; no sender payment or phone scan',live_payment:false}));
