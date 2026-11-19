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
    let loading = false;

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
        requestWrap.hidden = copy.hidden = retry.hidden = true;
        requestValue.value = "";
        image.removeAttribute("src");
        message.textContent = "Loading Cashu payment request…";
        try {
            const descriptor = await getJson("/api/v1/cashu/request", 4096);
            image.src = cashuQr(descriptor);
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
