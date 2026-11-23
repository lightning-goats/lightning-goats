// Decode only synthetic test tokens. No HTTP or wallet access is needed.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const [installation] = process.argv.slice(2);
const sdk = resolve(installation, 'node_modules/@cashu/cashu-ts');
const {getDecodedToken} = await import(pathToFileURL(resolve(sdk, 'lib/cashu-ts.es.js')));
const {token, keyset, amount, recipient} = JSON.parse(readFileSync(0, 'utf8'));
const decoded = getDecodedToken(token, [keyset]);
assert.equal(decoded.mint, 'https://mint.lightning-goats.com');
assert.equal(decoded.unit, 'sat');
assert.equal(decoded.proofs.reduce((sum, p) => sum + (typeof p.amount === 'number' ? p.amount : p.amount.toNumber()), 0), amount);
for (const proof of decoded.proofs) {
    const [kind, lock] = JSON.parse(proof.secret);
    assert.equal(kind, 'P2PK');
    assert.equal(lock.data, recipient);
}
