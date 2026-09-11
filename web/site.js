(() => {
    "use strict";
    const byId = (id) => document.getElementById(id);
    const enabled = window.LightningGoatsSite?.paymentsEnabled === true;
    const users = new Set(["herd", "dexter", "rowan", "cosmo", "newton", "nova"]);
    let pending = false;
    let generation = 0;

    async function copy(id, status) {
        try {
            await navigator.clipboard.writeText(byId(id).value);
            byId(status).textContent = "Copied.";
        } catch {
            byId(id).focus();
            byId(id).select();
            byId(status).textContent = "Select and copy the text above.";
        }
    }
    byId("copyContact").addEventListener("click", () => copy("contactNpub", "contactStatus"));
    byId("copyInvoice").addEventListener("click", () => copy("invoiceText", "paymentStatus"));

    function clearInvoice() {
        generation += 1;
        byId("invoiceWrap").hidden = true;
        byId("invoiceText").value = "";
        byId("invoiceWallet").hidden = true;
        byId("invoiceWallet").removeAttribute("href");
    }
    for (const id of ["feedAddress", "feedAmount"]) {
        byId(id).addEventListener("input", () => {
            clearInvoice();
            byId("paymentStatus").textContent = "";
        });
    }

    // Bound both the wait and decoded response body; never follow redirects.
    async function getJson(url) {
        const controller = new AbortController();
        const timer = window.setTimeout(() => controller.abort(), 10000);
        try {
            const response = await fetch(url, {
                signal: controller.signal, redirect: "error", credentials: "omit",
                cache: "no-store", headers: { Accept: "application/json" }
            });
            if (!response.ok || !/^application\/json(?:;|$)/i.test(response.headers.get("content-type") || "")) {
                throw new Error("Invoice service unavailable.");
            }
            const reader = response.body.getReader();
            const chunks = [];
            let size = 0;
            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                size += value.byteLength;
                if (size > 32768) {
                    await reader.cancel();
                    throw new Error("Invoice response too large.");
                }
                chunks.push(value);
            }
            const bytes = new Uint8Array(size);
            let offset = 0;
            for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
            const result = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
            if (!result || typeof result !== "object" || result.status === "ERROR") {
                throw new Error("Invoice request refused.");
            }
            return result;
        } finally {
            window.clearTimeout(timer);
            controller.abort();
        }
    }

    byId("feedLink").disabled = !enabled;
    byId("createInvoice").disabled = !enabled;
    if (enabled) byId("paymentAvailability").textContent = "Choose a Lightning Address and amount to create an invoice.";

    byId("feedForm").addEventListener("submit", async (event) => {
        event.preventDefault();
        if (!enabled || pending) return;
        clearInvoice();
        const current = generation;
        const user = byId("feedAddress").value;
        const raw = byId("feedAmount").value;
        const sats = Number(raw);
        const msat = sats * 1000;
        if (!users.has(user) || !/^[1-9][0-9]*$/.test(raw) || !Number.isSafeInteger(msat)) {
            byId("paymentStatus").textContent = "Choose a listed address and a positive whole number of sats.";
            return;
        }
        pending = true;
        byId("createInvoice").disabled = true;
        byId("paymentStatus").textContent = "Creating invoice…";
        try {
            const discovery = await getJson(`/.well-known/lnurlp/${user}`);
            if (current !== generation) return;
            if (discovery.tag !== "payRequest" || !Number.isSafeInteger(discovery.minSendable)
                || !Number.isSafeInteger(discovery.maxSendable) || discovery.minSendable < 1
                || msat < discovery.minSendable || msat > discovery.maxSendable) {
                throw new Error("Amount is outside this address's limits.");
            }
            const callback = new URL(discovery.callback);
            if (callback.origin !== window.location.origin || callback.username || callback.password
                || callback.pathname !== `/lnurlp/${user}/callback` || callback.search || callback.hash) {
                throw new Error("Unexpected invoice service address.");
            }
            callback.searchParams.set("amount", String(msat));
            const invoice = await getJson(callback.href);
            if (current !== generation) return;
            // The daemon verifies BOLT11 cryptography and amount before returning it.
            // This check only constrains the wallet URI; it is not invoice verification.
            if (typeof invoice.pr !== "string" || invoice.pr.length > 16000
                || !/^lnbc[0-9]*[munp]?1[023456789acdefghjklmnpqrstuvwxyz]+$/.test(invoice.pr)) {
                throw new Error("Invoice service returned an invalid invoice.");
            }
            byId("invoiceText").value = invoice.pr;
            byId("invoiceWallet").href = `lightning:${invoice.pr}`;
            byId("invoiceWallet").hidden = false;
            byId("invoiceWrap").hidden = false;
            byId("paymentStatus").textContent = "Pay once in your wallet. Creating an invoice does not confirm payment or a feed.";

        } catch {
            if (current === generation) byId("paymentStatus").textContent = "Could not create an invoice. No automatic retry was made. Check your wallet before trying again.";
        } finally {
            pending = false;
            byId("createInvoice").disabled = !enabled;
        }
    });
})();
