(() => {
    "use strict";
    const el = (id) => document.getElementById(id);
    const integer = (n) => Number.isSafeInteger(n) && n >= 0;
    const identity = (s) => typeof s === "string" && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(s);
    const fresh = (time) => Number.isFinite(time) && Date.now() - time <= 300000 && time - Date.now() <= 30000;
    const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const artworkVersion = "artwork-20261109";
    const logo = `/images/lightninggoatslogo1.png?v=${artworkVersion}`;
    const goats = new Map(["Dexter", "Rowan", "Cosmo", "Newton", "Nova"].map((name) => [name.toLowerCase(), name]));
    const timers = new Set(), animations = new Set();
    let stream = null, seq = null, credit = 0, threshold = 1000;
    let replaying = false, socket, retry = 500, reconnectTimer;
    let weatherTime = null, marketTime = null, statusTime = null;
    let feeding = false, qrBusy = false, paymentBusy = false;
    let availability = "unknown", awaitingConfirmation = false;
    let marqueeFrame = null, lastFrame = null, cardTimer = null;
    const messages = [], activeMessages = [], payments = [], feeds = [];

    function later(fn, ms) {
        const timer = setTimeout(() => { timers.delete(timer); fn(); }, ms);
        timers.add(timer);
        return timer;
    }
    function animate(node, frames, options) {
        if (reducedMotion || !node.animate) return;
        const animation = node.animate(frames, options);
        animations.add(animation);
        animation.onfinish = animation.oncancel = () => animations.delete(animation);
    }
    function queuedFeedings() { return Math.max(0, Math.floor(credit / threshold) - (awaitingConfirmation ? 1 : 0)); }
    function render() {
        el("credit").textContent = `${credit.toLocaleString()} ${credit === 1 ? "sat" : "sats"} credit`;
        const due = queuedFeedings();
        el("due").textContent = due ? `${due} feed${due === 1 ? "" : "s"} queued` : "";
        el("sats_left").textContent = due
            ? `⚡ ${due} feeding${due === 1 ? "" : "s"} queued · ${credit.toLocaleString()} sats credit ⚡`
            : awaitingConfirmation ? "⚡ Feeding awaiting confirmation · credit held ⚡"
            : `⚡${Math.max(0, threshold - credit).toLocaleString()} sats to go until the herd is fed again.⚡`;
        const percent = Math.min(100, credit / threshold * 100);
        el("progress").setAttribute("aria-valuenow", String(Math.round(percent)));
        el("progress-percentage").textContent = `${Math.round(percent)}%`;
        if (!feeding && !activeMessages.length) el("progress-fill").style.width = `${percent}%`;
        el("progress-fill").classList.toggle("is-filling", percent > 0 && percent < 100 && !feeding);
        renderFeedingBanner();
    }
    function clearPresentation() {
        for (const timer of timers) clearTimeout(timer);
        timers.clear();
        for (const animation of animations) animation.cancel();
        animations.clear();
        if (marqueeFrame !== null) cancelAnimationFrame(marqueeFrame);
        marqueeFrame = null; lastFrame = null;
        messages.length = activeMessages.length = payments.length = feeds.length = 0;
        feeding = qrBusy = paymentBusy = false;
        el("message").replaceChildren();
        el("weather").textContent = "";
        weatherTime = null;
        el("current-temperature").textContent = "-- °F";
        el("accordion-container").replaceChildren();
        for (const id of ["sats-received-overlay", "feeder-triggered", "accordion-container", "lightning-effect", "lightning-img", "progress-drainer"]) el(id).style.display = "none";
        el("qr-image").style.visibility = el("qr-brand").style.visibility = "visible";
        el("progress-drainer").style.width = "0%";
        el("progress-fill").classList.remove("shine-effect");
        el("progress-percentage").style.opacity = "1";
        el("container").classList.remove("fed");
    }
    function cleanText(text) {
        return typeof text === "string" ? text.replace(/https?:\/\/\S+|www\.\S+/gi, "").replace(/[\r\n\t]+/g, " ").trim().slice(0, 2000) : "";
    }
    function addMessage(text, expiresAt = null) {
        const cleaned = cleanText(text);
        if (!cleaned) return;
        if (messages.length >= 24) messages.shift();
        messages.push({ text: cleaned, expiresAt });
        if (marqueeFrame === null) marqueeFrame = requestAnimationFrame(scrollMessages);
    }
    function scrollMessages(now) {
        const elapsed = lastFrame === null ? 0 : Math.min(now - lastFrame, 1000);
        lastFrame = now;
        const width = el("progress-container").clientWidth;
        // Hold the ticker while the confirmed feeder effect owns the screen.
        if (!feeding) {
            for (const message of [...activeMessages]) {
                message.x -= elapsed * 0.2;
                if (message.x + message.width <= 0 || (message.expiresAt !== null && message.expiresAt < Date.now())) {
                    message.node.remove();
                    activeMessages.splice(activeMessages.indexOf(message), 1);
                } else message.node.style.transform = `translateX(${message.x}px)`;
            }
            const newest = activeMessages[activeMessages.length - 1];
            if (messages.length && activeMessages.length < 6 && (!newest || newest.x + newest.width <= width - 30)) {
                const message = messages.shift();
                if (message.expiresAt === null || message.expiresAt >= Date.now()) {
                    const node = document.createElement("span");
                    node.className = "message";
                    node.textContent = (newest ? " ⚡ " : "") + message.text;
                    node.style.transform = `translateX(${width}px)`;
                    el("message").append(node);
                    activeMessages.push({ ...message, node, x: width, width: node.offsetWidth });
                }
            }
            el("progress-percentage").style.opacity = activeMessages.length ? "0.18" : "1";
            const natural = Math.min(1, credit / threshold) * width;
            const edge = activeMessages.reduce((left, item) => Math.min(left, Math.max(0, item.x)), natural);
            el("progress-fill").style.transition = activeMessages.length ? "none" : "";
            el("progress-fill").style.width = `${edge}px`;
        }
        if (messages.length || activeMessages.length) marqueeFrame = requestAnimationFrame(scrollMessages);
        else { marqueeFrame = null; lastFrame = null; render(); }
    }
    function picture(url) {
        if (typeof url !== "string" || url.length > 2048) return logo;
        try {
            const parsed = new URL(url, location.href);
            if (parsed.origin === location.origin && /^\/images\/[a-zA-Z0-9._/-]+$/.test(parsed.pathname)) return parsed.href;
            // Future CyberHerd pictures can use public HTTPS. Never accept file,
            // data/javascript, embedded credentials or literal private-network hosts.
            if (parsed.protocol === "https:" && !parsed.username && !parsed.password
                && !parsed.port && /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(parsed.hostname)
                && !/\.(local|internal|localhost)$/i.test(parsed.hostname)) return parsed.href;
        } catch { /* Use the local logo. */ }
        return logo;
    }
    function showCards(items, members = false, newestKey = null) {
        if (!Array.isArray(items) || !items.length) return;
        const host = el("accordion-container");
        host.replaceChildren();
        if (cardTimer) { clearTimeout(cardTimer); timers.delete(cardTimer); }
        const displayed = members
            ? [...items].filter((item) => item && typeof item === "object").sort((a, b) => (integer(b.amount) ? b.amount : 0) - (integer(a.amount) ? a.amount : 0)).slice(0, 10)
            : items.slice(0, 5);
        displayed.forEach((item, index) => {
            if (!item || typeof item !== "object") return;
            const name = cleanText(item.display_name || item.name || "Anon").slice(0, 64);
            const known = goats.get(name.toLowerCase());
            if (!members && !known) return;
            const card = document.createElement("div");
            card.className = "accordion-item" + (members ? " user-profile" : "");
            card.style.animationDelay = `${index * 0.08}s`;
            if (members && (newestKey ? item.pubkey === newestKey : item.is_newest === true)) card.dataset.newest = "true";
            const content = document.createElement("div");
            content.className = "accordion-content";
            if (members) {
                const label = document.createElement("span"); label.className = "cyberherd-label";
                label.textContent = "CyberHerd"; content.append(label);
            }
            const image = document.createElement("img");
            image.alt = name; image.referrerPolicy = "no-referrer";
            image.src = members ? picture(item.picture || item.imageUrl) : `/images/${known.toLowerCase()}.png?v=${artworkVersion}`;
            image.onerror = () => { image.onerror = null; image.src = logo; };
            const title = document.createElement("span"); title.className = "user-profile-name"; title.textContent = name;
            content.append(image, title);
            if (members && integer(item.amount) && item.amount > 0) {
                const amount = document.createElement("span"); amount.className = "user-profile-zap";
                amount.textContent = `⚡ ${item.amount.toLocaleString()} sats`; content.append(amount);
            }
            card.append(content); host.append(card);
        });
        host.style.display = host.childElementCount ? "flex" : "none";
        cardTimer = later(() => { host.style.display = "none"; }, 18000);
    }
    function toggleQr() {
        if (qrBusy) return;
        qrBusy = true;
        el("qr-image").style.visibility = el("qr-brand").style.visibility = "hidden";
        el("lightning-img").style.display = "flex";
        animate(el("lightning-img"), [{ opacity: 0, transform: "scale(.8)" }, { opacity: 1, transform: "scale(1)" }], { duration: 400, easing: "cubic-bezier(.2,.8,.2,1.4)" });
        later(() => {
            el("lightning-img").style.display = "none";
            el("qr-image").style.visibility = el("qr-brand").style.visibility = "visible";
            animate(el("qr-image"), [{ opacity: 0 }, { opacity: 1 }], { duration: 300 });
            qrBusy = false;
        }, 2000);
    }
    function showPayment() {
        if (feeding || paymentBusy || !payments.length) return;
        const amount = payments.shift();
        paymentBusy = true;
        const tier = amount >= 21000 ? 4 : amount >= 5000 ? 3 : amount >= 1000 ? 2 : amount >= 100 ? 1 : 0;
        const sizes = [27, 31, 36, 42, 48], holds = [1050, 1450, 1900, 2350, 2850];
        const notice = el("sats-received-overlay");
        notice.textContent = `⚡ ${amount.toLocaleString()} ${amount === 1 ? "sat" : "sats"} ⚡`;
        notice.dataset.tier = ["spark", "zap", "boost", "surge", "goat"][tier];
        notice.classList.toggle("near-trigger", credit / threshold >= .8);
        notice.style.fontSize = `${sizes[tier]}px`;
        notice.style.padding = `${17 + tier * 4}px ${36 + tier * 10}px`;
        notice.style.display = "block";
        const center = "translate(-50%, -50%)";
        animate(notice, [{ opacity: 0, transform: `${center} scale(${.88 - tier * .075})` }, { opacity: 1, transform: `${center} scale(1)` }], { duration: 320, easing: "cubic-bezier(.2,.8,.2,1.5)" });
        later(() => {
            animate(notice, [{ opacity: 1, transform: `${center} scale(1)` }, { opacity: 0, transform: `${center} scale(${1.1 + tier * .06})` }], { duration: 380 });
            later(() => { notice.style.display = "none"; paymentBusy = false; showPayment(); }, 380);
        }, holds[tier] + 320);
    }
    function showFeed() {
        if (feeding || !feeds.length) return;
        feeds.shift(); feeding = true;
        el("sats-received-overlay").style.display = "none";
        el("container").classList.add("fed");
        const fill = el("progress-fill");
        fill.style.width = "100%"; fill.classList.remove("is-filling"); fill.classList.add("shine-effect");
        const notice = el("feeder-triggered"); notice.style.display = "block";
        const center = "translate(-50%, -50%)";
        animate(notice, [{ opacity: 0, transform: `${center} scale(.5)` }, { opacity: 1, transform: `${center} scale(1.1)`, offset: .75 }, { opacity: 1, transform: `${center} scale(1)` }], { duration: 350 });
        if (!reducedMotion) {
            el("lightning-effect").style.display = "block";
            animate(el("lightning-effect"), [{ opacity: 0 }, { opacity: 1 }, { opacity: 0 }], { duration: 150, iterations: 3 });
            animate(el("container"), [
                { transform: "translate(-14px, 10px)" }, { transform: "translate(12px, -11px)" },
                { transform: "translate(-10px, -8px)" }, { transform: "translate(9px, 12px)" },
                { transform: "translate(-7px, -5px)" }, { transform: "translate(6px, 6px)" },
                { transform: "translate(-3px, -3px)" }, { transform: "translate(2px, 1px)" }, { transform: "none" }
            ], { duration: 360 });
            later(() => { el("lightning-effect").style.display = "none"; }, 450);
        }
        later(() => {
            animate(notice, [{ opacity: 1, transform: `${center} scale(1)` }, { opacity: 0, transform: `${center} scale(1.35)` }], { duration: 500 });
        }, 1500);
        later(() => {
            notice.style.display = "none";
            const drainer = el("progress-drainer"); drainer.style.display = "block";
            animate(drainer, [{ width: "0%" }, { width: "100%" }], { duration: 800 });
            later(() => {
                drainer.style.display = "none"; fill.classList.remove("shine-effect");
                feeding = false; render();
                if (feeds.length) showFeed(); else showPayment();
            }, 800);
        }, 2000);
    }
    function acceptWeather(weather, message = null) {
        const observed = Date.parse(weather?.observed_at);
        const temperature = weather?.temperature_f;
        if (!fresh(observed) || (weatherTime !== null && observed < weatherTime)) return;
        weatherTime = observed;
        if (typeof temperature === "number" && Number.isFinite(temperature) && temperature >= -100 && temperature <= 150) el("current-temperature").textContent = `${Math.round(temperature)} °F`;
        if (typeof message === "string") {
            el("weather").textContent = cleanText(message);
            addMessage(message, observed + 300000);
        }
    }
    function resetConnection() {
        stream = null; seq = null; replaying = false;
        clearPresentation(); socket.close();
    }
    function receive(event) {
        if (typeof event.data !== "string" || event.data.length > 16384) return resetConnection();
        let data;
        try { data = JSON.parse(event.data); } catch { return resetConnection(); }
        if (!data || typeof data !== "object" || Array.isArray(data)) return resetConnection();
        if (data.type === "resume") {
            if (data.version !== 1 || data.stream_id !== stream || data.after !== seq || !integer(data.through) || data.through < seq) return resetConnection();
            replaying = true; return;
        }
        if (data.type === "snapshot") {
            if (data.version !== 1 || !identity(data.stream_id) || !integer(data.seq) || !integer(data.feed_credit_sats) || !integer(data.threshold_sats) || data.threshold_sats === 0) return resetConnection();
            if (data.stream_id === stream && data.seq < seq && !data.reset_reason) return;
            if (stream !== data.stream_id || data.reset_reason) clearPresentation();
            stream = data.stream_id; seq = data.seq; credit = data.feed_credit_sats; threshold = data.threshold_sats;
            awaitingConfirmation = typeof data.unresolved_feed_attempt === "string";
            replaying = false; el("connection").dataset.state = "live"; el("connection").textContent = ""; el("connection").ariaLabel = "Connected"; render(); return;
        }
        if (!integer(data.seq) || seq === null || stream === null) return resetConnection();
        if (data.seq <= seq) return;
        if (data.seq !== seq + 1) return resetConnection();
        if (typeof data.feeding?.awaiting_confirmation === "boolean") awaitingConfirmation = data.feeding.awaiting_confirmation;
        if (data.source_type === "feeder_confirmed") awaitingConfirmation = false;
        if (data.source_type === "processing_error" && typeof data.feed_attempt_id === "string") awaitingConfirmation = true;
        if (integer(data.feed_credit_sats)) { credit = data.feed_credit_sats; render(); }
        // Replays advance durable state without replaying announcements/effects.
        if (!replaying && data.type !== "event_skipped") {
            const kind = data.source_type || data.type;
            if (typeof kind !== "string" || kind.length > 80) return resetConnection();
            const isPayment = kind === "payment_received" || kind === "operator_credit_added";
            // Match the server's per-credit announcement threshold. Progress
            // still advances for sub-threshold payments and replay skip markers.
            if (isPayment && (!integer(data.amount_sats) || data.amount_sats < 10)) { seq = data.seq; return; }
            if (kind === "weather_status") acceptWeather(data.data, data.message);
            else addMessage(data.message);
            if (Array.isArray(data.goats)) showCards(data.goats);
            if (isPayment) {
                if (payments.length >= 12) payments.shift();
                payments.push(data.amount_sats); toggleQr(); showPayment();
            }
            if (kind === "feeder_confirmed") {
                if (feeds.length < 8) feeds.push(data.seq);
                showFeed();
            }
            if (kind.startsWith("cyberherd_") && Array.isArray(data.members)) showCards(data.members, true, data.newest_pubkey);
            if (kind === "cyberherd_daily_reset") { el("accordion-container").replaceChildren(); el("accordion-container").style.display = "none"; }
        }
        seq = data.seq;
    }
    function connect() {
        clearTimeout(reconnectTimer);
        const url = new URL("/ws/overlay", location.href);
        url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
        if (stream !== null && seq !== null) { url.searchParams.set("version", "1"); url.searchParams.set("stream", stream); url.searchParams.set("after", String(seq)); }
        const current = socket = new WebSocket(url.href);
        current.onopen = () => { if (socket === current) { retry = 500; el("connection").dataset.state = "connecting"; el("connection").textContent = "Synchronizing…"; } };
        current.onmessage = (event) => { if (socket === current) receive(event); };
        current.onclose = () => {
            current.onmessage = null;
            if (socket !== current) return;
            el("connection").dataset.state = "disconnected";
            el("connection").textContent = "Disconnected · reconnecting";
            clearPresentation(); replaying = false;
            reconnectTimer = setTimeout(connect, retry); retry = Math.min(retry * 2, 30000);
        };
        current.onerror = () => current.close();
    }
    async function readJson(path) {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 8000);
        try {
            const response = await fetch(path, { cache: "no-store", credentials: "omit", redirect: "error", signal: controller.signal });
            if (!response.ok || !response.headers.get("content-type")?.includes("application/json")) throw new Error("Display data unavailable");
            const body = await response.text();
            if (body.length > 16384) throw new Error("Display data too large");
            return JSON.parse(body);
        } finally { clearTimeout(timeout); }
    }
    async function displayData() {
        try {
            const data = await readJson("/overlay-data.json");
            if (data.weather) acceptWeather(data.weather);
            const market = data.market, observed = Date.parse(market?.observed_at);
            if (fresh(observed) && (marketTime === null || observed >= marketTime) && typeof market.price_usd === "number" && Number.isFinite(market.price_usd) && market.price_usd > 0) {
                marketTime = observed;
                el("btc-price").textContent = `BTC: ${market.price_usd.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 })}`;
                el("btc-price").title = "Indicative BTC/USD · CoinGecko · display only";
                const change = market.change_24h_percent;
                const valid = typeof change === "number" && Number.isFinite(change) && change >= -100 && change <= 10000;
                el("btc-change").textContent = valid ? `(24h: ${change >= 0 ? "+" : ""}${change.toFixed(2)}%)` : "(24h: --)";
                el("btc-change").classList.toggle("btc-change-positive", valid && change > 0);
                el("btc-change").classList.toggle("btc-change-negative", valid && change < 0);
            }
        } catch { /* Cached observations expire independently; money is unaffected. */ }
    }
    function renderFeedingBanner() {
        const banner = el("feeding-state");
        banner.dataset.state = availability;
        banner.hidden = availability === "ready";
        el("feeding-state-label").textContent = availability === "paused" ? "⏸ Feeder Paused"
            : availability === "unknown" ? "⏸ Feeder Paused · status unavailable"
            : availability === "pending" ? "Feeding pending confirmation" : "";
        const queued = queuedFeedings();
        let note = queued ? `${queued} feeding${queued === 1 ? "" : "s"} queued to run when feeding resumes.`
            : "0 feedings queued.";
        if (awaitingConfirmation) note += " An earlier feeding is awaiting confirmation.";
        el("feeding-queue").textContent = availability === "ready" ? "" : note;
    }
    function setFeedingState(state) { availability = state; renderFeedingBanner(); }
    async function status() {
        try {
            const data = await readJson("/api/v1/status");
            if (!["shadow", "canary", "active"].includes(data.mode)) return;
            statusTime = Date.now();
            awaitingConfirmation = typeof data.unresolved_feed_attempt === "string";
            const paused = data.mode === "shadow" || !data.gateway_reachable || data.remote_feeding_enabled !== true || data.feeder_override_active !== false;
            setFeedingState(paused ? "paused" : data.unresolved_feed_attempt ? "pending" : "ready");
            el("feeding-state").title = data.feeder_override_active === true
                ? "FeederOverride is on."
                : paused ? "Local feeding controls are paused." : "";
            // HTTP status is for availability only. Credit/progress come from the
            // ordered socket; a slower HTTP response must never regress the ledger.
        } catch { setFeedingState("unknown"); }
    }
    function makeQr() {
        // LUD-16 discovery encoded as LNURL with the Bech32 checksum. Uppercase
        // uses QR alphanumeric mode; H correction protects the small center logo.
        const text = "https://lightning-goats.com/.well-known/lnurlp/herd";
        const chars = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        const words = []; let bits = 0, value = 0;
        for (const byte of new TextEncoder().encode(text)) { value = (value << 8) | byte; bits += 8; while (bits >= 5) { bits -= 5; words.push((value >>> bits) & 31); } }
        if (bits) words.push((value << (5 - bits)) & 31);
        const expanded = [3, 3, 3, 3, 3, 0, 12, 14, 21, 18, 12];
        let checksum = 1;
        for (const word of [...expanded, ...words, 0, 0, 0, 0, 0, 0]) {
            const top = checksum >>> 25; checksum = ((checksum & 0x1ffffff) << 5) ^ word;
            [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3].forEach((generator, i) => { if ((top >>> i) & 1) checksum ^= generator; });
        }
        checksum ^= 1;
        const payload = "LNURL1" + [...words, ...[25, 20, 15, 10, 5, 0].map((shift) => (checksum >>> shift) & 31)].map((word) => chars[word]).join("").toUpperCase();
        const qr = qrcode(0, "H"); qr.addData(payload, "Alphanumeric"); qr.make();
        const count = qr.getModuleCount(), ns = "http://www.w3.org/2000/svg";
        const svg = document.createElementNS(ns, "svg"); svg.setAttribute("viewBox", `0 0 ${count + 8} ${count + 8}`); svg.setAttribute("shape-rendering", "crispEdges");
        const white = document.createElementNS(ns, "rect"); white.setAttribute("width", "100%"); white.setAttribute("height", "100%"); white.setAttribute("fill", "white"); svg.append(white);
        let path = "";
        for (let y = 0; y < count; y++) for (let x = 0; x < count; x++) if (qr.isDark(y, x)) path += `M${x + 4},${y + 4}h1v1h-1z`;
        const modules = document.createElementNS(ns, "path"); modules.setAttribute("d", path); modules.setAttribute("fill", "black"); svg.append(modules);
        el("qr-image").append(svg); el("qr-code").href = `lightning:${payload}`; el("qr-code").dataset.lnurl = payload;
    }
    function tick() {
        const now = new Date(); el("current-time").textContent = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        el("current-time").dateTime = now.toISOString();
        if (weatherTime !== null && !fresh(weatherTime)) { weatherTime = null; el("current-temperature").textContent = "-- °F"; el("weather").textContent = ""; }
        if (marketTime !== null && !fresh(marketTime)) { marketTime = null; el("btc-price").textContent = "BTC: --"; el("btc-change").textContent = "(24h: --)"; el("btc-change").classList.remove("btc-change-positive", "btc-change-negative"); }
        if (statusTime !== null && Date.now() - statusTime > 30000) setFeedingState("unknown");
    }
    if (new URLSearchParams(location.search).get("fit") === "1") {
        const fit = () => {
            document.documentElement.style.width = "100%";
            document.documentElement.style.height = "100%";
            document.body.style.transformOrigin = "top left";
            document.body.style.transform = `scale(${Math.min(innerWidth / 1920, innerHeight / 1080)})`;
        };
        fit(); addEventListener("resize", fit);
    }
    makeQr(); tick(); connect(); displayData(); status();
    setInterval(tick, 1000);
    setInterval(displayData, 15000);
    setInterval(status, 10000);
})();
