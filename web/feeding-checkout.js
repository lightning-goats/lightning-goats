(() => {
    "use strict";
    if (window.LightningGoatsSite?.paymentsEnabled !== true) return;
    const el = id => document.getElementById(id);
    const modal = el("finishFeedingModal");
    let id;
    let quote;

    async function load() {
        const current = id;
        const status = el("finishStatus");
        quote = undefined;
        el("finishChoices").hidden = el("finishRetry").hidden = el("finishCopy").hidden = true;
        status.textContent = "Checking the shared feeder balance…";
        try {
            const response = await fetch("/api/v1/feeding/checkout", {
                method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
                headers: {"Content-Type": "application/json"}, signal: AbortSignal.timeout(30000),
                body: JSON.stringify({id: current, user: el("finishGoat").value})
            });
            if (!response.ok) throw Error();
            const result = await response.json();
            if (id !== current) return;
            const labels = {funded: "This feeding is already funded.", paused: "Feeding is paused.",
                pending_feed: "A feeding is awaiting confirmation.", waiting_for_price: "Waiting for a BTC/USD price"};
            if (Object.hasOwn(labels, result.state)) {
                status.textContent = labels[result.state];
                el("finishRetry").hidden = result.state !== "waiting_for_price";
                return;
            }
            if (result.state !== "ready" || !Number.isSafeInteger(result.amount_sats) || result.amount_sats < 1 ||
                !/^lnbc[0-9a-z]+$/.test(result.invoice) || !/^CREQB1[0-9A-Z]+$/.test(result.cashu) ||
                result.uri !== `bitcoin:?lightning=${result.invoice}&creq=${result.cashu}`) throw Error();
            const qr = window.qrcode(0, "M");
            qr.addData(`bitcoin:?lightning=${result.invoice}&creq=`, "Byte");
            qr.addData(result.cashu, "Alphanumeric");
            qr.make();
            el("finishQr").src = qr.createDataURL(4, 16);
            el("finishWallet").href = result.uri;
            el("finishLightning").href = "lightning:" + result.invoice;
            el("finishCashu").href = "bitcoin:?creq=" + result.cashu;
            quote = result;
            el("feedLightningAmount").value = result.amount_sats;
            status.textContent = `${result.amount_sats} sats remained when you opened checkout. Pay once with either option.`;
            el("finishChoices").hidden = false;
        } catch {
            if (id !== current) return;
            status.textContent = "Checkout is temporarily unavailable.";
            el("finishRetry").hidden = false;
        }
    }
    function open() { id = crypto.randomUUID(); load(); }
    modal.addEventListener("show.bs.modal", open);
    modal.addEventListener("hidden.bs.modal", () => { id = undefined; quote = undefined; });
    el("finishGoat").addEventListener("change", open);
    el("finishRetry").addEventListener("click", load);
    let walletPending = false;
    el("finishLightning").addEventListener("click", async event => {
        const wallet = window.LightningGoatsPayment.wallet();
        if (!wallet) return;
        event.preventDefault();
        if (!quote || walletPending) return;
        const selected = quote;
        walletPending = true;
        try {
            await wallet.enable();
            if (selected !== quote) return;
            await wallet.sendPayment(selected.invoice);
            if (selected === quote) el("finishStatus").textContent = "Wallet reports payment sent. Waiting for confirmation…";
        } catch {
            if (selected === quote) el("finishStatus").textContent = "Check your wallet before retrying this invoice.";
        } finally {
            walletPending = false;
        }
    });
    for (const [button, field] of [["copyFinishLightning", "invoice"], ["copyFinishCashu", "cashu"]]) {
        el(button).addEventListener("click", async () => {
            if (!quote) return;
            const value = quote[field];
            try {
                await navigator.clipboard.writeText(value);
                el("finishStatus").textContent = "Copied to clipboard.";
            } catch {
                const input = el("finishCopy");
                input.value = value;
                input.hidden = false;
                input.focus();
                input.select();
            }
        });
    }
})();
