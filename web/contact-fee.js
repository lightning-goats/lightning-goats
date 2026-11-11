/* Anonymous contact checkout. Only server-verified fees authorize DM delivery. */
(() => {
    "use strict";
    const KEY = "lightning-goats-contact-fee-v1";
    const STATES = ["new", "issuing", "uncertain", "issued", "paid", "delivered"];
    const rank = (state) => ({new:0,issuing:1,uncertain:1,issued:2,paid:3,delivered:4})[state] ?? -1;

    async function post(path, value, missing = false) {
        const controller = new AbortController(), timer = setTimeout(() => controller.abort(), 10000);
        try {
            const response = await fetch(path, {method:"POST",body:JSON.stringify(value),
                headers:{"Content-Type":"application/json",Accept:"application/json"},
                credentials:"omit",redirect:"error",cache:"no-store",signal:controller.signal});
            if (missing && response.status === 404) return null;
            if (!response.ok || !/^application\/json(?:;|$)/i.test(response.headers.get("content-type") || "")) throw new Error("Contact service unavailable. Your saved contact can be checked again.");
            const reader = response.body.getReader(), chunks = []; let size = 0;
            while (true) {
                const {done,value} = await reader.read(); if (done) break;
                size += value.byteLength;
                if (size > 32768) {await reader.cancel();throw new Error("Invalid contact response.");}
                chunks.push(value);
            }
            const bytes = new Uint8Array(size);let offset = 0;
            for (const part of chunks) {bytes.set(part,offset);offset += part.length;}
            return JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(bytes));
        } finally {clearTimeout(timer);controller.abort();}
    }
    const digest = async (text) => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256",new TextEncoder().encode(text))), b=>b.toString(16).padStart(2,"0")).join("");
    const token = () => Array.from(crypto.getRandomValues(new Uint8Array(32)),b=>b.toString(16).padStart(2,"0")).join("");

    function create({anonymousEnvelope,validEnvelope,status,changed}) {
        const byId = (id) => document.getElementById(id);
        let pending = null, timer = null, polls = 0, checking = false;
        try {
            const raw = localStorage.getItem(KEY);
            const saved = raw && raw.length <= 32768 ? JSON.parse(raw) : null;
            if (saved && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(saved.id || "") &&
                /^[0-9a-f]{64}$/.test(saved.capability || "") && /^[0-9a-f]{64}$/.test(saved.emailHash || "") && STATES.includes(saved.state) && validEnvelope(saved.envelope)) {
                if (saved.invoice) {
                    window.LightningGoatsPayment.validateInvoice(saved.invoice);
                    if (window.NostrTools.nip57.getSatoshisAmountFromBolt11(saved.invoice) !== 100) throw new Error("Invalid saved fee invoice.");
                }
                pending = saved;
            }
        } catch (_) { /* Never turn damaged saved state into a payment request. */ }
        function enabled() {return window.LightningGoatsSite?.contactPaymentsEnabled === true;}
        function save() {localStorage.setItem(KEY,JSON.stringify(pending));}
        function render() {
            const invoice = pending?.invoice;
            const unpaid = pending?.state === "issued";
            byId("contactInvoiceWrap").hidden = !invoice;
            byId("contactWallet").hidden = !invoice || !unpaid;
            if (invoice) {
                window.LightningGoatsPayment.validateInvoice(invoice);
                byId("contactInvoice").value = invoice;
                byId("contactWallet").href = `lightning:${invoice}`;
                window.LightningGoatsPayment.drawQr(byId("contactQr"),invoice);
            }
            byId("contactCheck").hidden = !pending;
        }
        function reset() {
            clearTimeout(timer);timer = null;polls = 0;
            localStorage.removeItem(KEY);pending = null;
            render();
        }
        function consume(value, id) {
            if (!pending || pending.id !== id) return;
            if (!value || value.id !== id || value.fee_sats !== 100 || !STATES.includes(value.state)) throw new Error("The contact service returned an invalid status.");
            if (value.invoice != null) {
                window.LightningGoatsPayment.validateInvoice(value.invoice);
                if (window.NostrTools.nip57.getSatoshisAmountFromBolt11(value.invoice) !== 100) throw new Error("The contact invoice must be exactly 100 sats.");
                if (pending.invoice && pending.invoice !== value.invoice) throw new Error("The contact service changed the saved invoice.");
            }
            if (["issued","paid","delivered"].includes(value.state) && !value.invoice) throw new Error("The contact service omitted the saved invoice.");
            if (rank(value.state) < rank(pending.state || "new")) return;
            pending.state = value.state;
            if (value.invoice) pending.invoice = value.invoice;
            save();render();
            if (value.state === "delivered") {
                reset();
                byId("contactMessage").value = byId("contactEmail").value = "";
                status("An inbox relay accepted your encrypted message. This does not confirm it has been read.");
            } else if (value.state === "paid") {
                status("Payment confirmed. Your encrypted message is queued for the Lightning Goats inbox.");
            } else if (["issuing","uncertain"].includes(value.state)) {
                status("Invoice creation has not been confirmed. Your encrypted message is saved; no replacement payment is created automatically.");
            }
            changed();
        }
        async function check() {
            if (!pending || checking) return;
            checking = true;
            const id = pending.id, capability = pending.capability;
            try {
                const result = await post("/api/v1/contact/status",{id,capability},true);
                if (result) consume(result,id);
                return result;
            } finally {checking = false;}
        }
        function schedule() {
            clearTimeout(timer);
            if (!pending || polls >= 30) return;
            timer = setTimeout(async () => {
                polls++;
                try {await check();} catch (_) {status("Contact confirmation is temporarily unavailable. Your encrypted message and invoice are saved.");}
                schedule();
            },3000);
        }
        async function send(message,email) {
            if (pending) {
                await check();
                if (!pending) return; // Acknowledgment must never start a new fee.
                if (["issuing","uncertain","paid"].includes(pending.state)) {polls = 0;schedule();return;}
            }
            if (!enabled()) {status("New contact payments are paused. The saved contact has been retained.");return;}
            const wallet = window.LightningGoatsPayment.wallet();
            if (wallet) {status("Waiting for your Lightning wallet…");await wallet.enable();}
            email = email.trim();
            if (!pending) {
                const envelope = anonymousEnvelope(message,email);
                pending = {id:crypto.randomUUID(),capability:token(),emailHash:await digest(email),envelope,state:"new"};
            }
            // Ciphertext, status capability and email hash only. A storage
            // failure stops before provider creation or wallet submission.
            save();
            if (!pending.invoice) {
                if (!email || await digest(email) !== pending.emailHash) throw new Error("Re-enter the same reply email to finish creating the saved contact invoice.");
                status("Preparing your separate 100-sat contact payment…");
                const id = pending.id;
                const result = await post("/api/v1/contact",{id,capability:pending.capability,reply_email:email,envelope:pending.envelope});
                consume(result,id);
            }
            if (!pending || pending.state !== "issued" || !pending.invoice) {schedule();return;}
            byId("contactMessage").value = byId("contactEmail").value = "";
            if (wallet) {
                status("Confirm the 100-sat contact fee in your wallet…");
                try {
                    await wallet.sendPayment(pending.invoice);
                    status("Wallet reports payment sent. Waiting for Strike confirmation before delivering your message…");
                } catch (_) {
                    status("Payment was cancelled or could not be confirmed. Check your wallet before explicitly retrying this same invoice.");
                }
            } else {status("Pay the 100-sat invoice with the QR code or your Lightning wallet. It adds no feeder credit.");}
            polls = 0;schedule();
        }
        byId("contactCheck").addEventListener("click",async () => {
            try {await check();polls = 0;schedule();} catch (_) {status("Contact confirmation is unavailable. The saved encrypted contact is retained.");}
        });
        byId("contactCopy").addEventListener("click",async () => {
            try {await navigator.clipboard.writeText(byId("contactInvoice").value);status("Contact invoice copied.");}
            catch (_) {byId("contactInvoice").select();}
        });
        render();
        if (pending) setTimeout(() => {check().catch(()=>{}).finally(schedule);},0);
        return Object.freeze({enabled,hasPending:()=>Boolean(pending),caption:()=>pending?.state === "issued" ? "Pay 100 sats and send" : "Check saved contact",send,reset});
    }
    window.LightningGoatsContactFee = Object.freeze({create});
})();
