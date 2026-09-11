# Unapplied SSH hardening candidate

Production remains **HOLD**. This prepares the #19/#15 SSH policy change for the
observed Fedora VPS. It does not apply it or establish operator login/recovery,
account/key cleanup, final privilege revocation or production acceptance.

## Concrete proposed change

Replace the reviewed `/etc/ssh/sshd_config.d/00-local-hardening.conf` with
`deploy/ssh/00-local-hardening.conf.example`. Preserve the current empty-password,
X11, authentication-attempt and login-grace restrictions. The effective changes
observed in the private candidate configuration are:

| Directive | Current | Candidate |
| --- | --- | --- |
| `PermitRootLogin` | `prohibit-password` | `no` |
| `PasswordAuthentication` | `yes` | `no` |
| `AuthenticationMethods` | `any` | `publickey` |
| `PubkeyAuthentication` | `yes` | `yes` |
| `KbdInteractiveAuthentication` | `no` | `no` |

OpenSSH uses the first applicable value for most directives; a late snippet does
not reliably replace an earlier setting. The current early local snippet allows
root keys, and `50-cloud-init.conf` enables password authentication. The proposed
replacement belongs in the reviewed early position. It does not edit cloud-init,
vendor crypto policy, key files, account membership, forwarding or network policy.

`AuthenticationMethods publickey` makes a public key the required authentication
method, including when an alternative method is enabled elsewhere in global
defaults. A `Match` block can change per-connection policy, so checking global
output alone is insufficient. See the primary
[OpenSSH configuration manual](https://man.openbsd.org/sshd_config).

## Repeat the read-only candidate review

From the reviewed repository source, using existing read/test privileges:

```sh
sudo python3 -B deploy/scripts/review-sshd-policy.py \
  deploy/ssh/00-local-hardening.conf.example
```

There is deliberately no apply/reload option. The helper:

- verifies the observed main/include graph, absence of existing `Match` blocks,
  the Fedora service invocation and empty `/etc/sysconfig/sshd` options;
- refuses unreviewed includes, service overrides and candidate directives that
  could add commands, keys or conditional policy;
- copies configuration only into a private temporary directory, substitutes the
  candidate early snippet and preserves the vendor crypto-policy include;
- runs the installed `sshd -t` and `sshd -T`, comparing the complete effective
  policy and allowing changes only to the five authentication directives above;
- checks nine contexts combining the existing deployment/runtime users and root
  with synthetic IPv4/IPv6 and the known laptop source address;
- runs real `sshd` negative controls for a late snippet and a per-user `Match`
  relaxation, both of which must be detected as unaccepted;
- rechecks source bytes, include inventory, service options and on-disk effective
  policy, then removes its temporary files.

The checker invokes no listening daemon and copies no host/private keys. Syntax
testing uses the installed daemon's normal read-only host-key checks. Connection
contexts are configuration inputs, not network connections, operator identities
or tests of VPN source authentication. Existing `Match` blocks require a separate
complete policy review; these nine examples are not an exhaustive Match verifier.
The helper intentionally targets the observed Fedora layout. A different distro,
service command or include arrangement must be reviewed separately.

## Observed evidence, 2026-09-11

Continuation base: draft #43 head
`109ddbed760f703992a1d3eef9ec3cbc22a4601a`, with all three workflows passed.
[Fedora candidate evidence](../testing/evidence/ssh-candidate-fedora-20260911.json)
records the exact helper/candidate/source hashes, original and proposed policy,
service invocation, nine contexts and both negative controls. Syntax and policy
checks passed. Only the three changed effective settings in the table changed.
The on-disk policy remained byte-for-byte/effectively unchanged; no reload occurred.
`sshd -T` interprets configuration files; it does not inspect the running listener's
loaded policy or prove successful/denied authentication.

The original early snippet SHA256 is
`0144b1e57b930062e81cd37e0fdc8d0b90db1a269993614f336f8d80558ea00f`.
The proposed snippet SHA256 is
`8e595d0fbd0221312775e600d08a427b5f5e37c6c2c3092d521fff2b8c68302d`.
The evidence manifest also pins the main file, every included snippet, the
effective crypto-policy bytes and service-options file. Recheck all of them
before installation; an earlier successful review does not authorize overwriting
new operator work.

Four unprivileged regressions cover early refusal for missing review privileges,
candidate command/key/include/Match directives, unexpected host include/Match
layouts and service/environment overrides. They never inspect live host policy
or invoke root commands. The actual Fedora execution supplies the `sshd` semantic
evidence; ordinary CI unit tests do not prove operator access.

## Application and rollback gate

On 2026-09-11 the operator confirmed fresh key-authenticated logins for `sat` and
`linuxuser`, and working provider-console recovery. These are operator-reported
checks, distinct from the earlier configuration-only evidence. The operator also
required `sat` to retain SSH access and an approved public key to be provisioned
first.

A subsequent read-only host check still found no `/home/sat/.ssh/authorized_keys`
or `.ssh` directory; `linuxuser` has a mode-0600 authorized-key file in a mode-0700
`.ssh` directory. Do not infer a provisioned key for `sat` from the reported login.
Application remains pending the approved SSH public key, its safe installation
for `sat`, and a fresh key login using that installed key. The website Nostr key
is unrelated and must never be installed as an SSH key. Keep a verified second
administrative session and console recovery available for the change.

Once the retained account's key and access checks are satisfied, perform the reviewed SSH-only
change within the authorized staging scope:

1. Rerun the candidate review and compare the complete source manifest. Inspect
   any drift, new include, `Match` condition or service option before proceeding.
2. Save the exact original local snippet in a root-controlled backup outside the
   active include glob and verify its hash. Keep console recovery available.
3. Install the reviewed early snippet root-owned and non-writable by other users;
   preserve other files. Run syntax and effective-policy/context checks against
   the actual installed path before reloading. On failure restore the backup and
   validate the original policy before any reload.
4. Reload only the reviewed SSH service. Independently establish a new non-root
   key session and verify password and direct-root authentication are denied.
   A policy dump is not a successful authentication or reachability test.
5. If fresh access fails, use the retained administrative session or console to
   restore the exact backup, validate it and reload. Do not improvise wider access
   or delete keys to recover.

Stale-key/account review and final Codex/deployment privilege reduction remain
separate gates. The runtime account must stay locked/non-admin; final production
secrets must wait for the required privilege review. This SSH candidate does not
change DNS, WireGuard, household policy, payment or feeder authority.
