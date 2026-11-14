/* NIP-17 contact: visitor-owned seal, one-use gift wrapper, no project key. */
(() => {
    "use strict";
    const RECIPIENT = "669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6";
    const DISCOVERY = ["wss://nos.lol", "wss://relay.primal.net", "wss://relay.damus.io", "wss://nostr.chaima.info", "wss://relay.artx.market"];
    const PENDING = "lightning-goats-contact-pending-v1";
    const tools = window.NostrTools;

    function safeRelay(value) {
        try {
            const url = new URL(value);
            return url.protocol === "wss:" && !url.username && !url.password && !url.hash &&
                (!url.port || url.port === "443") && url.hostname.includes(".") &&
                !/^(?:[0-9.]+|\[.*\]|.*\.(?:local|localhost))$/i.test(url.hostname);
        } catch (_) { return false; }
    }
    function deadline(promise, milliseconds = 15000) {
        let timer;
        return Promise.race([promise, new Promise((_, reject) => {
            timer = setTimeout(() => reject(new Error("The relay did not respond.")), milliseconds);
        })]).finally(() => clearTimeout(timer));
    }
    function validEnvelope(event) {
        return event?.kind === 1059 && tools.verifyEvent(event) &&
            event.tags.length === 1 && event.tags[0].length === 2 &&
            event.tags[0][0] === "p" && event.tags[0][1] === RECIPIENT &&
            typeof event.content === "string" && event.content.length <= 24000;
    }
    function messageContent(message, email = "") {
        if (typeof message !== "string" || !message.trim() || message.length > 3000) throw new Error("Enter a message of 1–3,000 characters.");
        if (email && (email.length > 254 || !/^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/.test(email))) throw new Error("Enter a valid email address.");
        return `Website contact\n\n${message.trim()}${email ? `\n\nReply email: ${email}` : ""}`;
    }

    async function signedEnvelope(signer, selectedPubkey, message) {
        if (!/^[0-9a-f]{64}$/.test(selectedPubkey) || !signer?.nip44?.encrypt || !signer?.signEvent) {
            throw new Error("Your signer needs NIP-44 encryption and permission to sign a private-message seal.");
        }
        if (await signer.getPublicKey() !== selectedPubkey) throw new Error("The selected Nostr account changed. Reconnect before sending.");
        const rumor = { kind: 14, pubkey: selectedPubkey, created_at: Math.floor(Date.now() / 1000),
            tags: [["p", RECIPIENT]], content: messageContent(message) };
        rumor.id = tools.getEventHash(rumor);
        const encrypted = await signer.nip44.encrypt(RECIPIENT, JSON.stringify(rumor));
        if (typeof encrypted !== "string" || encrypted.length < 132 || encrypted.length > 16000 ||
            !/^[A-Za-z0-9+/]+={0,2}$/.test(encrypted) || atob(encrypted).charCodeAt(0) !== 2) throw new Error("The signer returned invalid encrypted content.");
        const random = crypto.getRandomValues(new Uint32Array(1))[0] / 2 ** 32;
        const template = { kind: 13, created_at: Math.floor(Date.now() / 1000 - random * 172800), tags: [], content: encrypted };
        const seal = await signer.signEvent(template);
        if (!tools.verifyEvent(seal) || seal.pubkey !== selectedPubkey || seal.kind !== 13 ||
            seal.created_at !== template.created_at || seal.content !== encrypted || JSON.stringify(seal.tags) !== "[]" ||
            await signer.getPublicKey() !== selectedPubkey) throw new Error("The signer changed the private message or its identity.");
        const event = tools.nip59.createWrap(seal, RECIPIENT);
        if (!validEnvelope(event)) throw new Error("Private message wrapping failed.");
        return event;
    }

    function anonymousEnvelope(message, email) {
        if (!email) throw new Error("An email address is required when you do not log in.");
        const content = messageContent(message, email);
        const ephemeral = tools.generateSecretKey();
        try {
            const event = tools.nip59.wrapEvent({ kind: 14, tags: [["p", RECIPIENT]], content }, ephemeral, RECIPIENT);
            if (!validEnvelope(event)) throw new Error("Private message wrapping failed.");
            return event;
        } finally { ephemeral.fill(0); }
    }

    async function inboxRelays(pool) {
        const events = await deadline(pool.querySync(DISCOVERY, { kinds: [10050], authors: [RECIPIENT], limit: 10 }));
        const now = Math.floor(Date.now() / 1000);
        const current = events.filter((event) => event.kind === 10050 && event.pubkey === RECIPIENT &&
            tools.verifyEvent(event) && event.created_at <= now + 300).sort((a, b) => b.created_at - a.created_at || a.id.localeCompare(b.id))[0];
        const relays = [...new Set((current?.tags || []).filter((tag) => tag[0] === "relay" && safeRelay(tag[1])).map((tag) => tag[1]))].slice(0, 8);
        if (!relays.length) throw new Error("The Lightning Goats inbox relays are temporarily unavailable. Your message has not been sent.");
        return relays;
    }

    async function publish(event) {
        if (!validEnvelope(event)) throw new Error("The saved private message is invalid.");
        const pool = new tools.SimplePool();
        let relays = [...DISCOVERY];
        try {
            relays = await inboxRelays(pool);
            await Promise.any(pool.publish(relays, event).map((promise) => deadline(promise)));
        } catch (_) {
            throw new Error("No inbox relay confirmed acceptance. Your encrypted message is saved for an explicit retry.");
        } finally { pool.close([...new Set([...DISCOVERY, ...relays])]); }
    }

    window.LightningGoatsContact = Object.freeze({ signedEnvelope, anonymousEnvelope, publish, validEnvelope });
    const form = document.getElementById("contactForm");
    if (!form || !tools) return;
    const byId = (id) => document.getElementById(id);
    let busy = false;
    let pending = null;
    try {
        const raw = localStorage.getItem(PENDING);
        const saved = raw && raw.length <= 32768 ? JSON.parse(raw) : null;
        if (saved && /^[0-9a-f]{64}$/.test(saved.sender || "") && validEnvelope(saved.event)) pending = saved;
    } catch (_) { /* Storage availability cannot substitute for a valid envelope. */ }

    const fee = window.LightningGoatsContactFee.create({anonymousEnvelope,validEnvelope,
        status: text => {byId("contactStatus").textContent = text;}, changed: () => update()});

    function update() {
        const pubkey = window.LightningGoatsIdentity?.getPublicKey() || "";
        const paidPending = fee.hasPending();
        byId("contactIdentity").textContent = paidPending ? "Saved email contact · separate 100-sat fee" : pubkey ? `From your Nostr account (${pubkey.slice(0, 8)}…) · free` : "Connect Nostr to send for free, or use email with a 100-sat contact fee.";
        byId("contactEmailRow").hidden = Boolean(pubkey) && !paidPending;
        byId("contactEmail").required = !pubkey && !paidPending;
        byId("contactSend").textContent = pending ? "Retry encrypted message" : paidPending ? fee.caption() : pubkey ? "Send private message" : "Pay 100 sats and send";
        byId("contactSend").disabled = busy || (Boolean(pending) && !pubkey) || (!pubkey && !fee.enabled() && !paidPending);
        byId("contactMessage").disabled = busy || Boolean(pending) || paidPending;
        byId("contactEmail").disabled = busy;
        byId("contactReset").hidden = !pending && !paidPending;
        if (!pubkey && !paidPending && !fee.enabled() && !byId("contactStatus").textContent) byId("contactStatus").textContent = "Paid email contact is being prepared. Nostr contact is available for free.";
    }
    byId("contactConnect").addEventListener("click", async () => {
        try { await window.LightningGoatsIdentity?.connect(); byId("contactStatus").textContent = ""; update(); }
        catch (_) { byId("contactStatus").textContent = "Connect a Nostr signer to send as yourself."; }
    });
    byId("contactReset").addEventListener("click", () => {
        if (busy) return;
        try {localStorage.removeItem(PENDING);fee.reset();pending = null;}
        catch (_) {byId("contactStatus").textContent = "The browser could not clear the saved contact.";return;}
        byId("contactStatus").textContent = "The local pending copy was cleared. It does not retract a submitted contact or undo a payment.";
        update();
    });
    form.addEventListener("submit", async (event) => {
        event.preventDefault();
        if (busy) return;
        const identity = window.LightningGoatsIdentity;
        const pubkey = identity?.getPublicKey();
        if (fee.hasPending() || !pubkey) {
            if (pending) {byId("contactStatus").textContent = "Reconnect the Nostr account that owns your saved message.";return;}
            if (!fee.enabled() && !fee.hasPending()) {update();return;}
            busy = true;update();
            try {await fee.send(byId("contactMessage").value,byId("contactEmail").value);}
            catch (error) {byId("contactStatus").textContent = error.message || "The saved private contact can be checked again.";}
            finally {busy = false;update();}
            return;
        }
        busy = true;
        update();
        byId("contactStatus").textContent = "Encrypting your private message…";
        try {
            if (pending && pending.sender !== pubkey) throw new Error("The saved message belongs to another Nostr account. Reconnect that account to retry it.");
            if (!pending) {
                const wrapped = await signedEnvelope(identity.getSigner(), pubkey, byId("contactMessage").value);
                pending = { sender: pubkey, event: wrapped };
                // Persist only ciphertext and a public identity, never plaintext
                // or visitor keys. Failure stops before any relay publication.
            }
            localStorage.setItem(PENDING, JSON.stringify(pending));
            byId("contactStatus").textContent = "Sending to the Lightning Goats inbox relays…";
            await publish(pending.event);
            localStorage.removeItem(PENDING);
            pending = null;
            byId("contactMessage").value = "";
            byId("contactStatus").textContent = "An inbox relay accepted your encrypted message. This does not confirm it has been read.";
        } catch (error) {
            byId("contactStatus").textContent = error.message || "The private message could not be sent.";
        } finally { busy = false; update(); }
    });
    document.getElementById("contactModal").addEventListener("show.bs.modal", update);
    document.addEventListener("nlAuth", () => setTimeout(update, 0));
    update();
})();
