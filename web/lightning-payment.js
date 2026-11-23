/* Shared browser payment transport. Provider settlement remains server authority. */
(() => {
    "use strict";
    const users = new Set(["herd", "dexter", "rowan", "cosmo", "newton", "nova"]);

    async function getJson(url, maximum = 32768) {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 10000);
        try {
            const response = await fetch(url, {
                signal: controller.signal, redirect: "error", credentials: "omit",
                cache: "no-store", headers: { Accept: "application/json" }
            });
            if (!response.ok || !/^application\/json(?:;|$)/i.test(response.headers.get("content-type") || "")) {
                throw new Error("Payment service unavailable.");
            }
            const reader = response.body.getReader(), chunks = [];
            let size = 0;
            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                size += value.byteLength;
                if (size > maximum) {
                    await reader.cancel();
                    throw new Error("Payment response exceeds its limit.");
                }
                chunks.push(value);
            }
            const bytes = new Uint8Array(size);
            let offset = 0;
            for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
            const result = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
            if (!result || typeof result !== "object" || result.status === "ERROR") throw new Error("Payment request refused.");
            return result;
        } finally {
            clearTimeout(timer);
            controller.abort();
        }
    }

    function wallet() {
        return [window.webln, window.alby?.webln].find((candidate) =>
            typeof candidate?.enable === "function" && typeof candidate?.sendPayment === "function") || null;
    }

    function validateInvoice(invoice) {
        // Constrain wallet URI construction. The backend verifies BOLT11 signature,
        // network, amount and description; this syntax check is not cryptography.
        if (typeof invoice !== "string" || invoice.length > 16000 ||
            !/^lnbc[0-9]*[munp]?1[023456789acdefghjklmnpqrstuvwxyz]+$/.test(invoice)) {
            throw new Error("The service returned an invalid Lightning invoice.");
        }
        return invoice;
    }

    async function createInvoice(user, sats, zap = null) {
        if (!users.has(user) || !Number.isSafeInteger(sats) || sats <= 0 || !Number.isSafeInteger(sats * 1000)) {
            throw new Error("Choose a listed address and a positive whole number of sats.");
        }
        const amount = sats * 1000;
        const discovery = await getJson(`/.well-known/lnurlp/${user}`);
        if (discovery.tag !== "payRequest" || !Number.isSafeInteger(discovery.minSendable) ||
            !Number.isSafeInteger(discovery.maxSendable) || discovery.minSendable < 1 ||
            amount < discovery.minSendable || amount > discovery.maxSendable) {
            throw new Error("Amount is outside this address's limits.");
        }
        const callback = new URL(discovery.callback);
        if (callback.protocol !== "https:" || callback.origin !== location.origin || callback.username || callback.password ||
            callback.pathname !== `/lnurlp/${user}/callback` || callback.search || callback.hash) {
            throw new Error("Unexpected payment service address.");
        }
        callback.searchParams.set("amount", String(amount));
        if (zap) {
            if (discovery.allowsNostr !== true) throw new Error("Nostr zaps are unavailable.");
            callback.searchParams.set("nostr", JSON.stringify(zap.request));
            callback.searchParams.set("lnurl", zap.lnurl);
        }
        const response = await getJson(callback.href);
        return validateInvoice(response.pr);
    }

    function drawQr(canvas, invoice) {
        if (!window.qrcode || !canvas) return;
        const qr = window.qrcode(0, "M");
        qr.addData(invoice.toUpperCase());
        qr.make();
        const count = qr.getModuleCount(), scale = Math.max(2, Math.floor(320 / (count + 8)));
        canvas.width = canvas.height = (count + 8) * scale;
        const ctx = canvas.getContext("2d");
        ctx.fillStyle = "#fff";
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.fillStyle = "#000";
        for (let row = 0; row < count; row++) for (let col = 0; col < count; col++) {
            if (qr.isDark(row, col)) ctx.fillRect((col + 4) * scale, (row + 4) * scale, scale, scale);
        }
    }

    window.LightningGoatsPayment = Object.freeze({ getJson, wallet, createInvoice, validateInvoice, drawQr });
})();
