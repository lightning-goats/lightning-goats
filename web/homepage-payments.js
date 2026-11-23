(() => {
    "use strict";
    if (window.LightningGoatsSite?.paymentsEnabled !== true) return;

    const element = (id) => document.getElementById(id);
    const { getJson } = window.LightningGoatsPayment;

    const modal = element("cashuModal");
    const requestWrap = element("cashuRequestWrap");
    const requestValue = element("cashuRequest");
    const image = element("cashuQr");
    const message = element("cashuStatus");
    const copy = element("cashuCopyButton");
    const retry = element("cashuRetryButton");
    const paste = element("cashuPaste");
    const tender = element("cashuTender");
    const send = element("cashuSendToken");
    const tokenStatus = element("cashuTokenStatus");
    let capability;
    let timer;
    let loading = false;

    async function privatePost(path, body) {
        return fetch(path, {
            method: "POST", cache: "no-store", credentials: "omit", redirect: "error",
            referrerPolicy: "no-referrer", headers: {"Content-Type": "application/json"},
            body: JSON.stringify(body), signal: AbortSignal.timeout(12000)
        });
    }

    function showStatus(status) {
        const labels = {
            pending: "Token received. Checking with the mint…",
            acquired: "Ecash acquired. Waiting for feeder credit…",
            held: "Token received. Settlement is on hold.",
            credited: "Payment credited to the feeder."
        };
        if (!Object.hasOwn(labels, status)) throw new Error("Invalid receipt");
        tokenStatus.textContent = labels[status];
        if (status === "credited") capability = undefined;
        if (capability && modal.classList.contains("show")) {
            timer = setTimeout(checkReceipt, status === "held" ? 15000 : 3000);
        }
        send.disabled = Boolean(capability);
    }

    async function checkReceipt() {
        clearTimeout(timer);
        const current = capability;
        if (!current) return;
        try {
            const reply = await privatePost("/api/v1/cashu/status", {capability: current});
            if (!reply.ok) throw new Error("Status unavailable");
            const result = await reply.json();
            if (current === capability) showStatus(result.status);
        } catch {
            tokenStatus.textContent = "Receipt status unavailable. Checking again shortly…";
            if (modal.classList.contains("show")) timer = setTimeout(checkReceipt, 10000);
        }
    }

    send.addEventListener("click", async () => {
        const token = tender.value.trim();
        if (!/^cashu[AB][A-Za-z0-9_-]+={0,2}$/.test(token)) {
            tokenStatus.textContent = "Paste a complete cashuA or cashuB token.";
            return;
        }
        send.disabled = true;
        try {
            const reply = await privatePost("/api/v1/cashu", token);
            if (reply.status === 400 || reply.status === 413) {
                tokenStatus.textContent = "Token rejected. Use one mint group, an accepted mint, and sat proofs within the receipt limit.";
                return;
            }
            if (reply.status !== 202) throw new Error("Admission uncertain");
            const result = await reply.json();
            if (!/^[0-9a-f]{64}$/.test(result.capability)) throw new Error("Missing receipt");
            capability = result.capability;
            tender.value = "";
            showStatus(result.status);
        } catch {
            tokenStatus.textContent = "Receipt unconfirmed. Retry with the same token.";
        } finally {
            send.disabled = Boolean(capability);
        }
    });
    modal.addEventListener("hidden.bs.modal", () => {
        clearTimeout(timer);
        tender.value = "";
    });
    modal.addEventListener("shown.bs.modal", checkReceipt);

    function cashuQr(descriptor) {
        const { id, version, enabled, request, encoding } = descriptor;
        if (version !== 1 || enabled !== true || typeof id !== "string" || !/^lg1-[0-9a-f]{32}$/.test(id) ||
            typeof request !== "string" || request.length > 2048) {
            throw new Error("Invalid Cashu payment request");
        }
        const creqA = encoding === "creqA" && /^creqA[A-Za-z0-9_-]+={0,2}$/.test(request);
        const creqB = encoding === "creqB" && /^CREQB1[023456789ACDEFGHJKLMNPQRSTUVWXYZ]+$/.test(request);
        if (!(creqA || creqB)) {
            throw new Error("Invalid Cashu payment request");
        }
        const qr = window.qrcode(0, "M");
        qr.addData(request, creqA ? "Byte" : "Alphanumeric");
        qr.make();
        return qr.createDataURL(4, 16);
    }

    async function loadCashuRequest() {
        if (loading) return;
        loading = true;
        requestWrap.hidden = paste.hidden = copy.hidden = retry.hidden = true;
        element("cashuMints").textContent = "";
        requestValue.value = "";
        image.removeAttribute("src");
        message.textContent = "Loading Cashu payment request…";
        try {
            const descriptor = await getJson("/api/v1/cashu/request", 4096);
            image.src = cashuQr(descriptor);
            if (Array.isArray(descriptor.mints) && descriptor.mints.length) {
                element("cashuMints").textContent = "Accepted mints: " + descriptor.mints.join(", ");
                paste.hidden = false;
            }
            requestValue.value = descriptor.request;
            requestWrap.hidden = copy.hidden = false;
            message.textContent = "";
        } catch {
            message.textContent = "Cashu is temporarily unavailable. You can still pay with Lightning.";
            retry.hidden = false;
        } finally {
            loading = false;
        }
    }

    modal.addEventListener("show.bs.modal", loadCashuRequest);
    retry.addEventListener("click", loadCashuRequest);
    copy.addEventListener("click", async () => {
        const request = requestValue.value;
        if (!request) return;
        try {
            await navigator.clipboard.writeText(request);
            message.textContent = "Payment request copied.";
        } catch {
            requestValue.focus();
            requestValue.select();
            message.textContent = "Copy the selected request, then paste it into your wallet.";
        }
    });

    element("feedPanel").hidden = false;
    document.querySelector(".cashu-code-overlay").hidden = false;
})();
