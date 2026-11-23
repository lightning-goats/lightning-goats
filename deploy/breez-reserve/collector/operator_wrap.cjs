// Only ciphertext leaves this process. Proofs enter on stdin, never in argv.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
function sdk() {
    const bundle = fs.readFileSync(path.join(__dirname, 'nostr.bundle.js'), 'utf8');
    return vm.runInThisContext(bundle + '\nNostrTools;');
}
if (require.main === module) {
    try {
        const {recipient, payload} = JSON.parse(fs.readFileSync(0, 'utf8'));
        const tools = sdk();
        const key = tools.generateSecretKey();
        try {
            const event = tools.nip59.wrapEvent({
                kind: 14, tags: [['p', recipient]], content: JSON.stringify(payload)
            }, key, recipient);
            if (event.kind !== 1059 || !tools.verifyEvent(event) ||
                JSON.stringify(event.tags) !== JSON.stringify([['p', recipient]])) throw Error();
            process.stdout.write(JSON.stringify(event));
        } finally { key.fill(0); }
    } catch { process.exitCode = 1; }
}
module.exports = {sdk};
