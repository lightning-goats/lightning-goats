# Live six-program upgrade, 2026-11-11

The operator explicitly approved maintenance for the freshly prepared six-program
transaction. The [qualified source](cyberherd-financial-maintenance-checkpoint-20261111.md)
is `489057e5c9c51331dcf636c02fcdc47987882b8a`, tested as the file-identical merge
`149860f0efb54b2e93c91f55ccb5b5ae06aa0e91`.

Fresh preparation retained the current three-user capacity and existing project
operator identity. Independent readback verified the original protected files,
their retained copies, selected program hashes and unchanged service states.
The earlier preparation predating the operator change was not used.

The approved application stopped and masked all four writers, accepted a fresh
native four-journal snapshot with both original and selected compiled-schema
verifiers, compared their native status on a separate copy, and bound a fresh
private reward-backlog report. It replaced only six native programs. Financial
images and original configuration remained unchanged during application.

The first resumption attempt exhausted `/run` while making native-parser copies.
Inspection confirmed that it failed before the durable resumption fence and
before starting services. Five task-owned native-check scratch directories were
copied to protected disk storage with hash verification and synchronization, then
only their exact scratch originals were removed. The accepted backup and all
application state were retained. The same transaction then repeated its complete
pre-start verification and successfully resumed receive and membership.

Independent final verification checked all six installed hashes, actual running
receive/member executable hashes, original configuration, local health and public
HTTPS roster. Capacity remains **3** and member allocation **10%**. Payout and
signer services remain masked, inactive and without processes. No financial
configuration or spending/signing ceiling was activated by this maintenance.

The source scratch-lifecycle correction is separate from the installed qualified
tool generation. It removes disposable native-check copies on success and failure;
the selected originals and exact configurations remain retained in the protected
review. Its regression tests cover parser refusal and partial-copy exhaustion.
Qualification of that correction is tracked separately from this completed live
upgrade.

The full twelve-part port remains incomplete. Explicit financial limits,
provider/recipient acceptance, external recovery adoption and the corresponding
financial activation are still required. Public operator-site delivery and
remaining real contact/zap acceptance also remain separate.
