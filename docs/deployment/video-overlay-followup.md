# Video and QR overlay follow-up

Production remains HOLD. The operator confirmed that video plays automatically
on the activated staging page. A fresh public source comparison showed the
production page still matches the imported source SHA256
`d880e262683788f8c5cbda599684b64c55573bd8c1ab5ef66d659e20f2465ac1`.
The staging YouTube iframe URL, muted autoplay parameters, permissions and
referrer policy were identical. The embed is unchanged by this correction.

The operator reported the feed button was obscured and requested the original
QR-area layer. The original source uses `.qr-code-overlay`, top/right 10px,
180x190 desktop and 100x120 on small screens, with z-index 10. The migrated
bottom-right button lacked this explicit stacking rule and used opacity .55
while disabled. This change restores the original QR-area target geometry,
adds an opaque readable label and focus outline, and places the iframe in a
lower stacking context inside an isolated video container.

The desktop/mobile browser regression raises the harmless iframe's z-index and
checks that the QR target is hit above it, stays within the player and remains
fully opaque while disabled. Before the fix it failed on desktop: the QR-area
hit did not reach the button and opacity was .55. All browser HTTP/WebSocket
traffic in this regression is intercepted. It does not establish live playback;
the operator's observation supplies that separate evidence.

The initial automated live-player comparison was stopped because it consumed
excessive resources on the small VPS without producing a completed observation.
No playback success is inferred from that run. No payment or Nostr publication
was initiated. The staging server continues to force payments disabled, so the
visible overlay does not authorize invoices, payments or physical feeding.
