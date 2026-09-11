(() => {
    "use strict";

    const LG_PUBKEY = "669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6";
    const LIVE_EVENT_KIND = 30311;
    const LIVE_CHAT_KIND = 1311;
    const LIVE_EVENT_MAX_AGE_SECONDS = 36 * 60 * 60;
    const MAX_CHAT_MESSAGES = 300;
    const MAX_MESSAGE_LENGTH = 500;

    // These include the relays advertised by the Lightning Goats zap.stream events.
    const DISCOVERY_RELAYS = [
        "wss://relay.zap.stream",
        "wss://relay.snort.social",
        "wss://nos.lol",
        "wss://relay.damus.io",
        "wss://relay.nostr.band",
        "wss://nostr.land",
        "wss://nostr.oxtr.dev",
        "wss://nostr-pub.wellorder.net"
    ];

    // Add event IDs or pubkeys here if an operator-side hide becomes necessary.
    const HIDDEN_EVENT_IDS = new Set([]);
    const MUTED_PUBKEYS = new Set([]);

    const elements = {
        authButton: document.getElementById("nostrAuthButton"),
        eventTitle: document.getElementById("chatEventTitle"),
        form: document.getElementById("chatForm"),
        input: document.getElementById("chatInput"),
        messages: document.getElementById("chatMessages"),
        notice: document.getElementById("chatNotice"),
        send: document.getElementById("chatSend"),
        state: document.getElementById("chatState"),
        statusDot: document.getElementById("chatStatusDot"),
    };

    if (!window.NostrTools) {
        setChatState("The Nostr library could not be loaded. The video is still available.");
        elements.eventTitle.textContent = "Nostr unavailable";
        return;
    }

    const { SimplePool, nip19, verifyEvent } = window.NostrTools;
    const nostrLoginSigner = window.nostr || null;
    const pool = new SimplePool({ trackRelays: true });
    const liveEventsByAddress = new Map();
    const chatEvents = new Map();
    const profiles = new Map();
    const pendingProfiles = new Set();

    let activeLiveEvent = null;
    let activeAddress = "";
    let activeRelays = [...DISCOVERY_RELAYS];
    let chatSubscription = null;
    let discoverySubscriptions = [];
    let currentUserPubkey = "";
    let activeNostrSigner = null;
    let profileTimer = null;
    let discoverySettled = false;
    let isPublishing = false;

    function getTag(event, name) {
        return event.tags.find((tag) => tag[0] === name)?.[1] || "";
    }

    function hasLightningGoatsHostTag(event) {
        return event.tags.some((tag) => {
            if (tag[0] !== "p" || tag[1] !== LG_PUBKEY) return false;
            const role = String(tag[3] || "").toLowerCase();
            return !role || role === "host";
        });
    }

    function isLightningGoatsLiveEvent(event) {
        return event.pubkey === LG_PUBKEY || hasLightningGoatsHostTag(event);
    }

    function getAddress(event) {
        const identifier = getTag(event, "d");
        return identifier ? `${event.kind}:${event.pubkey}:${identifier}` : "";
    }

    function isSafeRelay(value) {
        if (typeof value !== "string" || !value.startsWith("wss://")) return false;
        try {
            const url = new URL(value);
            return url.protocol === "wss:";
        } catch (_) {
            return false;
        }
    }

    function getEventRelays(event) {
        const advertised = event.tags
            .filter((tag) => tag[0] === "relays")
            .flatMap((tag) => tag.slice(1))
            .filter(isSafeRelay);

        return [...new Set(["wss://relay.zap.stream", ...advertised, ...DISCOVERY_RELAYS])].slice(0, 12);
    }

    function isCurrentLiveEvent(event) {
        const now = Math.floor(Date.now() / 1000);
        return event.kind === LIVE_EVENT_KIND &&
            isLightningGoatsLiveEvent(event) &&
            getTag(event, "status").toLowerCase() === "live" &&
            event.created_at >= now - LIVE_EVENT_MAX_AGE_SECONDS &&
            event.created_at <= now + 300;
    }

    function rememberLiveEvent(event) {
        if (!verifyEvent(event) || !isLightningGoatsLiveEvent(event)) return;
        const address = getAddress(event);
        if (!address) return;
        const previous = liveEventsByAddress.get(address);
        if (!previous || event.created_at > previous.created_at) {
            liveEventsByAddress.set(address, event);
            selectActiveLiveEvent();
        }
    }

    function selectActiveLiveEvent() {
        const candidates = [...liveEventsByAddress.values()]
            .filter(isCurrentLiveEvent)
            .sort((a, b) => b.created_at - a.created_at);
        const next = candidates[0] || null;
        const nextAddress = next ? getAddress(next) : "";

        if (!next) {
            if (activeLiveEvent) clearActiveLiveEvent();
            else if (discoverySettled) showOfflineState();
            return;
        }

        if (nextAddress === activeAddress) {
            activeLiveEvent = next;
            activeRelays = getEventRelays(next);
            elements.eventTitle.textContent = getTag(next, "title") || "Lightning Goats live";
            return;
        }

        activateLiveEvent(next);
    }

    function activateLiveEvent(event) {
        activeLiveEvent = event;
        activeAddress = getAddress(event);
        activeRelays = getEventRelays(event);
        chatEvents.clear();

        elements.statusDot.classList.add("live");
        elements.eventTitle.textContent = getTag(event, "title") || "Lightning Goats live";
        setChatState("Loading live chat…");
        updateComposer();

        if (chatSubscription) chatSubscription.close("live event changed");
        chatSubscription = pool.subscribe(
            activeRelays,
            { kinds: [LIVE_CHAT_KIND], "#a": [activeAddress], limit: 200 },
            {
                onevent: rememberChatEvent,
                oneose: () => {
                    if (chatEvents.size === 0) {
                        setChatState("The goats are live. Be the first to say hello!");
                    }
                }
            }
        );

    }

    function clearActiveLiveEvent() {
        activeLiveEvent = null;
        activeAddress = "";
        activeRelays = [...DISCOVERY_RELAYS];
        chatEvents.clear();
        if (chatSubscription) chatSubscription.close("live event ended");
        chatSubscription = null;
        showOfflineState();
        updateComposer();
    }

    function showOfflineState() {
        elements.statusDot.classList.remove("live");
        elements.eventTitle.textContent = "No current Nostr livestream";
        setChatState("Chat opens automatically when the Lightning Goats Nostr event goes live.");
    }

    function rememberChatEvent(event) {
        if (!verifyEvent(event) || event.kind !== LIVE_CHAT_KIND) return;
        if (!event.tags.some((tag) => tag[0] === "a" && tag[1] === activeAddress)) return;
        if (HIDDEN_EVENT_IDS.has(event.id) || MUTED_PUBKEYS.has(event.pubkey)) return;
        if (typeof event.content !== "string" || event.content.length > 2000) return;

        const now = Math.floor(Date.now() / 1000);
        if (event.created_at > now + 600) return;

        chatEvents.set(event.id, event);
        if (chatEvents.size > MAX_CHAT_MESSAGES) {
            const oldest = [...chatEvents.values()].sort((a, b) => a.created_at - b.created_at)[0];
            if (oldest) chatEvents.delete(oldest.id);
        }

        queueProfile(event.pubkey);
        renderMessages();
    }

    function queueProfile(pubkey) {
        if (profiles.has(pubkey)) return;
        pendingProfiles.add(pubkey);
        window.clearTimeout(profileTimer);
        profileTimer = window.setTimeout(fetchPendingProfiles, 180);
    }

    async function fetchPendingProfiles() {
        const authors = [...pendingProfiles].slice(0, 100);
        authors.forEach((pubkey) => pendingProfiles.delete(pubkey));
        if (!authors.length) return;

        try {
            const events = await pool.querySync(activeRelays, { kinds: [0], authors, limit: authors.length });
            const newest = new Map();
            events.forEach((event) => {
                if (!verifyEvent(event)) return;
                const previous = newest.get(event.pubkey);
                if (!previous || event.created_at > previous.created_at) newest.set(event.pubkey, event);
            });

            newest.forEach((event, pubkey) => {
                try {
                    const profile = JSON.parse(event.content);
                    profiles.set(pubkey, profile && typeof profile === "object" ? profile : {});
                } catch (_) {
                    profiles.set(pubkey, {});
                }
            });

            authors.forEach((pubkey) => {
                if (!profiles.has(pubkey)) profiles.set(pubkey, {});
            });
            renderMessages(false);
        } catch (_) {
            // Profile metadata is optional; chat remains usable without it.
        }

        if (pendingProfiles.size) profileTimer = window.setTimeout(fetchPendingProfiles, 250);
    }

    function safeImageUrl(value) {
        if (typeof value !== "string") return "";
        try {
            const url = new URL(value);
            return url.protocol === "https:" && !url.username && !url.password ? url.href : "";
        } catch (_) {
            return "";
        }
    }

    function shortNpub(pubkey) {
        try {
            const encoded = nip19.npubEncode(pubkey);
            return `${encoded.slice(0, 10)}…${encoded.slice(-5)}`;
        } catch (_) {
            return `${pubkey.slice(0, 8)}…`;
        }
    }

    function profileName(pubkey) {
        const profile = profiles.get(pubkey) || {};
        return profile.display_name || profile.displayName || profile.name || profile.nip05 || shortNpub(pubkey);
    }

    function renderMessages(allowScroll = true) {
        const nearBottom = elements.messages.scrollHeight - elements.messages.scrollTop - elements.messages.clientHeight < 90;
        const fragment = document.createDocumentFragment();
        const items = [...chatEvents.values()]
            .map((event) => ({ event, id: event.id, created_at: event.created_at }))
            .sort((a, b) => a.created_at - b.created_at || a.id.localeCompare(b.id));

        if (!items.length) return;

        function makeAvatar(pubkey, name) {
            const profile = profiles.get(pubkey) || {};
            const avatar = document.createElement("div");
            avatar.className = "chat-avatar";
            avatar.setAttribute("aria-hidden", "true");
            const imageUrl = safeImageUrl(profile.picture || profile.image);
            if (imageUrl) {
                const image = document.createElement("img");
                image.src = imageUrl;
                image.alt = "";
                image.loading = "lazy";
                image.referrerPolicy = "no-referrer";
                image.addEventListener("error", () => {
                    avatar.replaceChildren(document.createTextNode(name.slice(0, 1) || "N"));
                }, { once: true });
                avatar.appendChild(image);
            } else {
                avatar.textContent = name.slice(0, 1) || "N";
            }
            return avatar;
        }

        items.forEach((item) => {
            const { event } = item;
            const name = profileName(event.pubkey);
            const row = document.createElement("article");
            row.className = "chat-message";
            row.dataset.eventId = event.id;

            const body = document.createElement("div");
            body.className = "chat-message-body";
            const meta = document.createElement("div");
            meta.className = "chat-message-meta";
            const author = document.createElement("span");
            author.className = "chat-author";
            author.textContent = name;
            author.title = shortNpub(event.pubkey);
            const time = document.createElement("time");
            time.className = "chat-time";
            time.dateTime = new Date(event.created_at * 1000).toISOString();
            time.textContent = new Date(event.created_at * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
            const content = document.createElement("div");
            content.className = "chat-content";
            content.textContent = event.content;

            meta.append(author, time);
            body.append(meta, content);
            row.append(makeAvatar(event.pubkey, name), body);
            fragment.appendChild(row);
        });

        elements.messages.replaceChildren(fragment);
        if (allowScroll && nearBottom) elements.messages.scrollTop = elements.messages.scrollHeight;
    }

    function setChatState(message) {
        const state = document.createElement("div");
        state.id = "chatState";
        state.className = "chat-state";
        state.textContent = message;
        elements.state = state;
        elements.messages.replaceChildren(state);
    }

    function setNotice(message, type = "") {
        elements.notice.textContent = message;
        elements.notice.className = `chat-notice${type ? ` ${type}` : ""}`;
    }

    function updateComposer() {
        const enabled = Boolean(activeLiveEvent && currentUserPubkey && !isPublishing);
        elements.input.disabled = !enabled;
        elements.send.disabled = !enabled;

        if (!activeLiveEvent) elements.input.placeholder = "Chat opens when the goats go live";
        else if (!currentUserPubkey) elements.input.placeholder = "Connect Nostr to chat";
        else elements.input.placeholder = "Say something to the herd…";
    }

    async function refreshAuthenticatedUser() {
        try {
            if (!window.nostr?.getPublicKey) throw new Error("No Nostr signer is available");
            activeNostrSigner = window.nostr;
            currentUserPubkey = await window.nostr.getPublicKey();
            elements.authButton.textContent = shortNpub(currentUserPubkey);
            elements.authButton.title = "Switch Nostr account";
            queueProfile(currentUserPubkey);
            updateComposer();
        } catch (_) {
            activeNostrSigner = null;
            currentUserPubkey = "";
            elements.authButton.textContent = "Connect Nostr";
            elements.authButton.title = "";
            updateComposer();
        }
    }

    async function connectNostrInteractive() {
        try {
            // Prefer the browser's injected NIP-07 provider. This produces the
            // native Alby/nos2x/etc. approval prompt with no intermediary form.
            const extension = window.__lightningGoatsNip07 ||
                (window.nostr && window.nostr !== nostrLoginSigner ? window.nostr : null);
            if (extension?.getPublicKey && extension?.signEvent) {
                activeNostrSigner = extension;
            } else if (window.nostr?.getPublicKey) {
                // nostr-login supplies the NIP-46 fallback when no extension exists.
                activeNostrSigner = window.nostr;
            } else {
                document.dispatchEvent(new CustomEvent("nlLaunch", { detail: "login" }));
                throw new Error("Choose a NIP-07 extension or Nostr Connect signer.");
            }
            currentUserPubkey = await activeNostrSigner.getPublicKey();
            if (!/^[0-9a-f]{64}$/.test(currentUserPubkey)) throw new Error("The signer returned an invalid public key.");
            elements.authButton.textContent = shortNpub(currentUserPubkey);
            elements.authButton.title = "Switch Nostr account";
            queueProfile(currentUserPubkey);
            updateComposer();
            return currentUserPubkey;
        } catch (error) {
            if (!currentUserPubkey) {
                activeNostrSigner = null;
                elements.authButton.textContent = "Connect Nostr";
                elements.authButton.title = "";
                updateComposer();
            }
            throw error;
        }
    }

    async function publishMessage(content) {
        if (!activeLiveEvent || !activeAddress) throw new Error("The Nostr livestream is not currently active.");
        if (!activeNostrSigner?.signEvent) throw new Error("Connect a Nostr signer first.");

        const primaryRelay = activeRelays[0] || "wss://relay.zap.stream";
        const template = {
            kind: LIVE_CHAT_KIND,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["a", activeAddress, primaryRelay, "root"]],
            content
        };

        const signedEvent = await activeNostrSigner.signEvent(template);
        if (!verifyEvent(signedEvent)) throw new Error("The signer returned an invalid event.");

        let userRelays = [];
        try {
            const advertised = await activeNostrSigner.getRelays?.();
            if (advertised && typeof advertised === "object") {
                userRelays = Object.entries(advertised)
                    .filter(([, policy]) => policy?.write !== false)
                    .map(([relay]) => relay)
                    .filter(isSafeRelay);
            }
        } catch (_) {
            // Publishing to the activity relays is sufficient.
        }

        const publishRelays = [...new Set([...activeRelays, ...userRelays])].slice(0, 10);
        const results = await Promise.allSettled(pool.publish(publishRelays, signedEvent));
        if (!results.some((result) => result.status === "fulfilled")) {
            throw new Error("No relay accepted the message. Please try again.");
        }

        rememberChatEvent(signedEvent);
    }

    elements.authButton.addEventListener("click", async () => {
        if (currentUserPubkey) {
            document.dispatchEvent(new CustomEvent("nlLaunch", { detail: "switch-account" }));
            return;
        }
        setNotice("Waiting for your Nostr signer…");
        try {
            await connectNostrInteractive();
            setNotice("Nostr signer connected.", "success");
        } catch (error) {
            setNotice(error?.message || "Nostr connection was cancelled.", "error");
        }
    });

    elements.form.addEventListener("submit", async (event) => {
        event.preventDefault();
        const content = elements.input.value.trim();
        if (!content || content.length > MAX_MESSAGE_LENGTH || isPublishing) return;

        isPublishing = true;
        updateComposer();
        setNotice("Requesting your signature…");

        try {
            await publishMessage(content);
            elements.input.value = "";
            setNotice("Message published to Nostr.", "success");
        } catch (error) {
            setNotice(error?.message || "The message could not be published.", "error");
        } finally {
            isPublishing = false;
            updateComposer();
            if (!elements.input.disabled) elements.input.focus();
        }
    });

    elements.input.addEventListener("keydown", (event) => {
        if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            elements.form.requestSubmit();
        }
    });

    document.addEventListener("nlAuth", (event) => {
        if (event.detail?.type === "login" || event.detail?.type === "signup") {
            refreshAuthenticatedUser();
        } else if (event.detail?.type === "logout") {
            activeNostrSigner = null;
            currentUserPubkey = "";
            elements.authButton.textContent = "Connect Nostr";
            elements.authButton.title = "";
            updateComposer();
        }
    });

    function startDiscovery() {
        const callbacks = {
            onevent: rememberLiveEvent,
            oneose: () => {
                discoverySettled = true;
                selectActiveLiveEvent();
            }
        };

        // The current self-hosted publisher authors 30311 directly as LG.
        discoverySubscriptions.push(pool.subscribe(
            DISCOVERY_RELAYS,
            { kinds: [LIVE_EVENT_KIND], authors: [LG_PUBKEY], limit: 30 },
            callbacks
        ));

        // Keep compatibility with older/provider-authored events that tag LG as host.
        discoverySubscriptions.push(pool.subscribe(
            DISCOVERY_RELAYS,
            { kinds: [LIVE_EVENT_KIND], "#p": [LG_PUBKEY], limit: 30 },
            callbacks
        ));

        window.setTimeout(() => {
            discoverySettled = true;
            selectActiveLiveEvent();
        }, 6500);
    }

    startDiscovery();
    window.setInterval(selectActiveLiveEvent, 60 * 1000);

    window.addEventListener("beforeunload", () => {
        discoverySubscriptions.forEach((subscription) => subscription.close("page unloading"));
        chatSubscription?.close("page unloading");
        pool.close([...new Set([...DISCOVERY_RELAYS, ...activeRelays])]);
    });
})();
