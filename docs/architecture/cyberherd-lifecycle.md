# Optional availability notices and stream invitations

The isolated CyberHerd source adds `messaging.lifecycle`, default `false`. Enabling
it requires enabled messaging and its separate pinned NIP-46 authentication
client. It never loads a project signing key, spend key or OpenHAB token. There
is no feeder, stream-stop or household administration command in this observer.

The existing two-second core safety poll now also caches the actual observed
`FeederOverride` value. The private intake-only `LifecycleContext` read exposes
that value separately from runtime mode and `RemoteEnabled` state. A missing,
future or older-than-five-second reading is unknown. Unknown readings do not
advance the last confirmed state or announce resumption. Initial OFF establishes
a quiet baseline; initial ON announces a pause. Repeated readings, sunrise changes
and process restarts cannot produce another transition. Optional category deletion
is respected when the transition is queued and remains deleted on default seeding.

A transition and its complete rendered destinations commit in the same CyberHerd
transaction. Each enabled transition gets one standalone kind-1 note and, when
currently authenticated discovery supplies a live stream, one kind-1311 chat.
The chat stores its stable `a` root and the native signed stream/authority evidence.
No joining-member anchor or fake `p` tag is introduced. If discovery is unavailable,
the standalone note still works; no unverified chat target is guessed. A queued
chat keeps its original stream if the broadcast changes later.

`notice_events`, destinations, unsigned fields, signatures and relay receipts are
retained independently of member conversations. Signature fields cannot be
replaced. Network retries reuse the exact signed event, and only a matching real
relay OK records delivery. Work is bounded to 2,048 unfinished destinations and
32 selection candidates, with leases and backoff. Pause/resume order is enforced
per destination and per relay, allowing a working relay to progress while another
is unavailable. The worker refreshes confirmed override/public-mode permission
before both signing and publication. Pause notices are permitted while the switch
is ON; ordinary member publication remains gated on its existing Ready check.

Invitations use only an authenticated currently live stream. They freeze the
configured minimum/member allocation and filled/open places; the full-capacity
footer explains that likes cannot headbutt. No regular-note or overlay copy is
created for an invitation. Queue times and actual relay acknowledgment times both
respect the two-hour per-stream cooldown. Expired, replaced-stream or ended-stream
invitations cannot publish; observing a pause cancels queued invitations so that
they do not block that chat's pause notice. Cancellation retains signed audit data.
Removing `cyber_herd_invitation` opts out without restoring its rows later.

The narrow trusted gateway adds GET `/v1/solar`. Its fixed read set is
`Sun_SunPhaseName`, `Sun_Set_Start` and `Sun_DirectRadiation`. It returns only
sanitized phase, sunset epoch and bounded integer milliwatts per square metre,
plus observation time. Unknown/missing Items remain optional. There is no
caller-selected item, URL or command path. An independent one-minute core loop
runs only when the private core bridge is configured, using the existing gateway
address/port. A slow Astro read cannot delay the safety poll, settlement or feeder.
The core caches this context for the same authenticated intake-only read; neither
CyberHerd process connects to generic OpenHAB REST.

In the afternoon, authenticated dusk/night, today's passed sunset, or at most one
hour before today's sunset with radiation at or below 0.5 W/m² selects evening
wording. If useful fresh solar data is unavailable, fallback begins at 18:00 in
`America/Denver`, including DST. Confirmed daylight or positive direct radiation
overrides that fallback. Morning low light does not imply evening. Solar context
older than 90 seconds cannot override local fallback. Evening is selected only
on the first pause transition and can use a current authenticated stream that
ended within 15 minutes; it never changes stream eligibility for membership.

The six ordinary pause, six resume and eight evening variants are imported from
`cyberherd_messaging/defaults.py` at source tree
`672ef13ee9d8786841377cd265fcb705aed9d904`; the existing uploaded legacy export is
unchanged. Canonical categories are `feeder_requests_paused`,
`feeder_requests_enabled`, and `feeder_requests_paused_night`. Their simple safe
placeholder rendering and deletion tombstones reuse the existing template store.
No diagnostics, information, weather or reset is added to this public outbox.

This is optional source implementation. No production unit, gateway, credential,
relay event, payment or physical feeding is changed by the patch. It does not yet
supply the confirmed-feeding stream copy/herd summary, roster/OBS/operator UI,
recovery/retention or installed qualification required by the complete port.
