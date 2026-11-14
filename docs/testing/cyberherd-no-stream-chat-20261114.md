# CyberHerd stream chat opt-out — 2026-11-14

The operator requested that CyberHerd messages stop appearing in stream chat.
The independent `[messaging] stream_chat = false` policy preserves note-thread
publishing, standalone feeding/lifecycle notes and WebSocket/OBS presentation.
Existing configurations retain their prior behavior when the setting is omitted;
the example recommends explicitly disabling chat.

The policy is bound to the runtime store before it is cloned into workers. New
member, feeding and lifecycle messages omit stream destinations, and stream-only
invitations are not generated. Reservation queries exclude previously queued
stream destinations before the candidate limit, so retained chat cannot starve
note delivery. Workers also refuse kind-1311 publication with chat disabled.
Existing signed events, frozen text, leases, receipts and financial history are
retained; no chat is falsely marked delivered or erased. Re-enabling the policy
would deliberately permit retained chat retries again.

Regression coverage checks retained signed chat is never selected/sent, note
updates still publish, member overlays remain available, lifecycle notes remain
while invitations stop, and confirmed feeding notes/overlays survive with an
actual stream present. No financial schema or accounting change is required.

Source qualification and live application are pending at this checkpoint.
