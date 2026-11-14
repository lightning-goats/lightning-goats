(() => {
    "use strict";
    const byId = (id) => document.getElementById(id);
    const enabled = window.LightningGoatsSite?.paymentsEnabled === true;
    const users = new Set(["herd", "dexter", "rowan", "cosmo", "newton", "nova"]);
    if (byId("cyberherdLink")) byId("cyberherdLink").hidden = window.LightningGoatsSite?.cyberherdEnabled !== true;
    let pending = false;
    let generation = 0;
    let pendingInvoice = null;

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
        pendingInvoice = null;
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

    byId("feedLink").disabled = !enabled;
    byId("createInvoice").disabled = !enabled;
    if (enabled) byId("paymentAvailability").textContent = window.LightningGoatsSite?.feedingAvailable === true
        ? "Choose a Lightning Address and amount, then confirm in your wallet."
        : "Lightning payments are available. Feed credit is saved; automatic feeding is currently paused.";

    byId("feedForm").addEventListener("submit", async (event) => {
        event.preventDefault();
        if (!enabled || pending) return;
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
        byId("paymentStatus").textContent = "Waiting for your Lightning wallet…";
        byId("feedAddress").disabled = byId("feedAmount").disabled = true;
        try {
            const wallet = window.LightningGoatsPayment.wallet();
            if (wallet) await wallet.enable();
            if (current !== generation) return;
            if (!pendingInvoice) pendingInvoice = await window.LightningGoatsPayment.createInvoice(user, sats);
            if (current !== generation) return;
            const invoice = pendingInvoice;
            byId("invoiceText").value = invoice;
            byId("invoiceWallet").href = `lightning:${invoice}`;
            byId("invoiceWallet").hidden = false;
            byId("invoiceWrap").hidden = false;
            window.LightningGoatsPayment.drawQr(byId("invoiceQr"), invoice);
            if (wallet) {
                byId("paymentStatus").textContent = "Confirm the payment in your wallet…";
                await wallet.sendPayment(invoice);
                byId("paymentStatus").textContent = "Wallet reports payment sent. Waiting for confirmation…";
            } else {
                byId("paymentStatus").textContent = "Scan the QR code or open your Lightning wallet.";
            }

        } catch {
            if (current === generation) byId("paymentStatus").textContent = pendingInvoice
                ? "Payment was cancelled or could not be confirmed. No automatic retry was made. Check your wallet before retrying this same invoice."
                : "Could not start payment. No automatic retry was made. Check your wallet before trying again.";
        } finally {
            pending = false;
            byId("createInvoice").disabled = !enabled;
            byId("feedAddress").disabled = byId("feedAmount").disabled = false;
        }
    });
})();
