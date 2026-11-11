# Current-schema core update checkpoint — 2026-11-11

The [feed-intent correction](feed-intent-contention-20261111.md) passed all ten
workflows on source `55ed77b8a3f0c7cb50b031536cb53161de6d5e89`. Production
still runs its previously installed schema-24 binaries with private contact
enabled. The older schema-23 upgrade controller is intentionally unsuitable.

The [program-only maintenance mode](../deployment/current-schema-core-patch.md)
now has source, protected-file regressions and a guarded actual-systemd extension.
Local non-root boundary tests pass; privileged file cases are explicitly skipped
locally and require isolated CI. Exact CI/native installed qualification and actual
production application of this new maintenance mode remain pending. No production
service, configuration, database or program was changed by this source work.

The native operator fixture is separately qualified by the
[installed operator checkpoint](cyberherd-installed-operator-checkpoint-20261111.md).
The full twelve-part implementation remains open. Public membership/lifecycle,
operator ingress and rewards/payout activation are not enabled by this update.
