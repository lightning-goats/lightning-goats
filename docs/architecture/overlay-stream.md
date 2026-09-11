# Overlay stream version 1

The read-only endpoint is `/ws/overlay`. It never accepts application commands.
A new connection without a cursor receives the existing financial `snapshot`,
now including `version: 1` and a durable UUID `stream_id`. That identity belongs
to the daemon ledger and survives ordinary process restart. A different ledger
has a different identity. After offline restore, before restarting any daemon,
run `lightning-goatsctl --config <restored-config> reset-overlay-stream`. This
local command rotates only overlay identity; financial and outbox state remain
intact. Rotation is mandatory because events appended after an older backup can
reuse sequences acknowledged from the abandoned future. Raw database replacement
without this restore step is unsupported; it cannot be distinguished reliably
from unchanged history using a numeric cursor alone. Browser compatibility remains an acceptance gate
until the authoritative website source is available and tested.

A client reconnects with `?version=1&stream=<stream_id>&after=<last_applied_seq>`.
Unknown query keys, unsupported versions, out-of-range cursors or cursors without
a version/stream are rejected before upgrade. A usable cursor receives:

1. `resume` with `version`, `stream_id`, `after`, and `through` (no `seq`).
2. Events strictly in ascending order in `(after, through]`.
3. A consistent financial `snapshot` at `seq=through`.
4. Live events after that checkpoint.

The current checkpoint is deliberately sent after replay. Clients apply each
event once using its sequence and advance their cursor only after application.
Apply the checkpoint when its sequence equals the last replayed sequence; ignore
older snapshots. Do not display historical events as new physical actions, and
never infer completion from the progress bar. Only confirmed feeder outcomes
represent completed actuation.

A foreign/future cursor, missing history, or excessive replay receives a new
snapshot with `reset_reason: "resume_unavailable"`. Clear previous stream state
and use that checkpoint. A live gap/rendering failure similarly emits a snapshot
with `reset_reason: "event_window_unavailable"`. Resets explicitly abandon the
unavailable presentation interval; financial state comes from the ledger snapshot.
They do not alter durable events or publication recovery.

Replay is limited to 1,000 contiguous events and 512 KiB of stored payload/type
bytes, then bounded again to 16 KiB per rendered event and 512 KiB of queued text
including the checkpoint. Live polls load at most 100 events per batch. At most
32 connections hold upgrade permits. Incoming frames/messages are limited to
1 KiB and 20 frames per ten seconds; application text/binary frames close the
connection. Output writes have a five-second deadline. Ping every 25 seconds,
with an exact-payload Pong required within ten seconds, keeps quiet connections
active through nginx's shipped 75-second timeout and releases dead clients.
Web browsers handle Ping/Pong themselves; the website must reconnect on close
with bounded backoff and the last applied cursor.

Weather events whose observation timestamp is missing, invalid, older than five
minutes, or more than 30 seconds ahead are emitted as `event_skipped` with the
original `seq`, `source_type: "weather_status"`, and a reason. Advance the cursor
without displaying their old message. Clients must also expire visible weather
when its `observed_at` ages beyond five minutes; a connection does not make an old
observation current. Weather and interface events remain overlay-only.
