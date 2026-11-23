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
                pending_feed: "A feeding is awaiting confirmation."};
            if (Object.hasOwn(labels, result.state)) {
                status.textContent = labels[result.state];
                return;
            }
            if (result.state !== "ready" || !Number.isSafeInteger(result.amount_sats) || result.amount_sats < 1 ||
                !/^lnbc[0-9a-z]+$/.test(result.invoice) || !/^CREQB1[0-9A-Z]+$/.test(result.cashu) ||
                result.uri !== `bitcoin:?lightning=${result.invoice}&creq=${result.cashu}`) throw Error();
            const qr = window.qrcode(0, "M");
            qr.addData(result.uri, "Byte");
            qr.make();
            el("finishQr").src = qr.createDataURL(4, 16);
            el("finishWallet").href = result.uri;
            el("finishLightning").href = "lightning:" + result.invoice;
            el("finishCashu").href = "cashu:" + result.cashu;
            quote = result;
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
