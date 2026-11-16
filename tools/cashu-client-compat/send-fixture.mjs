// Pinned SDK test sender only: synthetic existing proofs, no mint RPC or UI.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

try {
    const [installation]=process.argv.slice(2);
    assert(installation && process.argv.length===3);
    const raw=readFileSync(0,'utf8');
    assert(Buffer.byteLength(raw)<=65536);
    const input=JSON.parse(raw);
    const origin=new URL(input.origin);
    assert.equal(origin.protocol,'http:');
    assert.equal(origin.hostname,'127.0.0.1');
    assert.equal(origin.pathname,'/');
    assert.equal(origin.search,'');
    assert.equal(origin.hash,'');
    const originalFetch=globalThis.fetch;
    globalThis.fetch=(url,options={})=>{
        const target=new URL(url);
        assert.equal(target.origin,origin.origin);
        assert(['/api/v1/cashu','/api/v1/cashu/request'].includes(target.pathname));
        assert.equal(target.search,'');
        return originalFetch(target,{...options,redirect:'error',signal:AbortSignal.timeout(5000)});
    };
    const packageRoot=resolve(installation,'node_modules/@cashu/cashu-ts');
    assert.equal(JSON.parse(readFileSync(resolve(packageRoot,'package.json'))).version,'4.7.2');
    const {PaymentRequest,Wallet,JSONInt,sumProofs}=await import(pathToFileURL(resolve(packageRoot,'lib/cashu-ts.es.js')));
    const metadataResponse=await fetch(origin.origin+'/api/v1/cashu/request');
    assert.equal(metadataResponse.status,200);
    const metadata=await metadataResponse.json();
    const request=PaymentRequest.fromEncodedRequest(metadata.request);
    assert.equal(request.id,input.descriptor_id);
    assert.equal(request.id,metadata.id);
    assert.equal(request.amount,undefined);
    assert.equal(request.singleUse,false);
    assert.equal(request.unit,'sat');
    assert(request.mints.includes(input.mint));
    const transport=request.getTransport('post');
    assert(transport && new URL(transport.target).protocol==='https:');
    // Only this isolated fixture maps the advertised HTTPS route to loopback.
    // No TLS or real sender-application acceptance is claimed.
    const wallet=new Wallet(input.mint,{unit:'sat'});
    wallet.loadMintFromCache({name:'synthetic fixture only',nuts:{}},
        {mintUrl:input.mint,keysets:[input.keyset]});
    const selected=wallet.sendOffline(input.gross_sats,input.proofs,
        {exactMatch:true,includeFees:false,requireDleq:true});
    assert.equal(selected.keep.length,0);
    assert.equal(sumProofs(selected.send).toString(),String(input.gross_sats));
    const payload={id:request.id,mint:input.mint,unit:request.unit,proofs:selected.send};
    const body=JSONInt.stringify(payload);
    const endpoint=origin.origin+'/api/v1/cashu';
    const preflight=await fetch(endpoint,{method:'OPTIONS',headers:{
        origin:'https://sender.example','access-control-request-method':'POST',
        'access-control-request-headers':'content-type',
    }});
    assert.equal(preflight.status,204);
    assert.equal(preflight.headers.get('access-control-allow-origin'),'*');
    const submit=()=>fetch(endpoint,{method:'POST',
        headers:{'content-type':'application/json',origin:'https://sender.example'},body});
    const lost=await submit();
    assert.equal(lost.status,202);
    await lost.arrayBuffer(); // Deliberately discard the first admission body.
    const repeated=await submit();
    assert.equal(repeated.status,202);
    assert.equal(repeated.headers.get('cache-control'),'no-store');
    assert.equal(repeated.headers.get('access-control-allow-origin'),'*');
    const accepted=await repeated.json();
    assert.equal(accepted.status,'pending');
    assert.match(accepted.capability,/^[0-9a-f]{64}$/);
    const again=await submit();
    assert.equal(again.status,202);
    assert.deepEqual(await again.json(),accepted);
    // Private stdout pipe to the native fixture; never log proofs/capabilities.
    process.stdout.write(JSONInt.stringify({payload,accepted,library:'@cashu/cashu-ts',
        version:'4.7.2',offline_send:true,lost_admission_reply_retried:true}));
} catch {
    process.stderr.write('Cashu-TS synthetic sender fixture rejected.\n');
    process.exitCode=1;
}
