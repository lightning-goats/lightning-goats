(() => {
    "use strict";
    const el = (id) => document.getElementById(id);
    const integer = (n) => Number.isSafeInteger(n) && n >= 0;
    const identity = (s) => typeof s === "string" && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(s);
    let stream = null, seq = null, credit = 0, threshold = 1000;
    let replaying = false, socket, retry = 500, reconnectTimer, messageTimer;
    let weatherTime = null;
    const messages = [];

    function render() {
        el("credit").textContent = `${credit.toLocaleString()} sats credit`;
        const due = Math.floor(credit / threshold);
        el("due").textContent = due ? `${due} feed${due === 1 ? "" : "s"} queued` : "";
        el("target").textContent = `${threshold.toLocaleString()} sats per feeding · herd@${location.hostname}`;
        const percent = Math.min(100, credit / threshold * 100);
        el("fill").style.width = `${percent}%`;
        el("progress").setAttribute("aria-valuenow", String(Math.round(percent)));
    }
    function clearPresentation() {
        messages.length = 0;
        clearTimeout(messageTimer);
        messageTimer = undefined;
        el("message").textContent = "";
        el("weather").textContent = "";
        weatherTime = null;
    }
    function showNext() {
        const message = messages.shift();
        el("message").textContent = message || "";
        messageTimer = message ? setTimeout(showNext, 8000) : undefined;
    }
    function fresh(time) {
        return Number.isFinite(time) && Date.now() - time <= 300000 && time - Date.now() <= 30000;
    }
    function resetConnection() {
        stream = null;
        seq = null;
        replaying = false;
        clearPresentation();
        socket.close();
    }
    function receive(event) {
        if (typeof event.data !== "string" || event.data.length > 16384) return resetConnection();
        let data;
        try { data = JSON.parse(event.data); } catch { return resetConnection(); }
        if (!data || typeof data !== "object") return resetConnection();
        if (data.type === "resume") {
            if (data.version !== 1 || data.stream_id !== stream || data.after !== seq
                || !integer(data.through) || data.through < seq) return resetConnection();
            replaying = true;
            return;
        }
        if (data.type === "snapshot") {
            if (data.version !== 1 || !identity(data.stream_id) || !integer(data.seq)
                || !integer(data.feed_credit_sats) || !integer(data.threshold_sats)
                || data.threshold_sats === 0) return resetConnection();
            if (data.stream_id === stream && data.seq < seq && !data.reset_reason) return;
            if (stream !== data.stream_id || data.reset_reason) clearPresentation();
            stream = data.stream_id;
            seq = data.seq;
            credit = data.feed_credit_sats;
            threshold = data.threshold_sats;
            replaying = false;
            el("connection").textContent = "Live ledger";
            render();
            return;
        }
        if (!integer(data.seq) || seq === null || stream === null) return resetConnection();
        if (data.seq <= seq) return;
        if (data.seq !== seq + 1) return resetConnection();
        if (integer(data.feed_credit_sats)) { credit = data.feed_credit_sats; render(); }
        // Replayed events update state without replaying old feed animations.
        if (!replaying && data.type !== "event_skipped") {
            if (data.source_type === "weather_status") {
                const observed = Date.parse(data.data?.observed_at);
                if (fresh(observed) && typeof data.message === "string") {
                    weatherTime = observed;
                    el("weather").textContent = data.message.slice(0, 2000);
                }
            } else if (typeof data.message === "string" && data.message.length <= 2000) {
                if (messages.length < 6) messages.push(data.message);
                if (!messageTimer) showNext();
            }
            if (data.source_type === "feeder_confirmed") {
                const card = document.querySelector("main");
                card.classList.remove("fed");
                void card.offsetWidth;
                card.classList.add("fed");
            }
        }
        seq = data.seq;
    }
    function connect() {
        clearTimeout(reconnectTimer);
        const url = new URL("/ws/overlay", location.href);
        url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
        if (stream !== null && seq !== null) {
            url.searchParams.set("version", "1");
            url.searchParams.set("stream", stream);
            url.searchParams.set("after", String(seq));
        }
        socket = new WebSocket(url.href);
        socket.onopen = () => { retry = 500; el("connection").textContent = "Synchronizing…"; };
        socket.onmessage = receive;
        socket.onclose = () => {
            socket.onmessage = null;
            el("connection").textContent = "Disconnected · reconnecting";
            clearPresentation();
            replaying = false;
            reconnectTimer = setTimeout(connect, retry);
            retry = Math.min(retry * 2, 30000);
        };
        socket.onerror = () => socket.close();
    }
    setInterval(() => {
        if (weatherTime !== null && !fresh(weatherTime)) {
            el("weather").textContent = "";
            weatherTime = null;
        }
    }, 1000);
    connect();
})();
