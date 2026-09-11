# SSH hardening review and application

Production remains **HOLD**. The reviewed #19/#15 SSH policy was applied to the
Fedora staging VPS on 2026-09-11 after operator-confirmed key login and console
recovery. The application evidence below supersedes the earlier unapplied state.
Account/key cleanup, final privilege revocation and production acceptance remain
separate gates.

## Reviewed change

Replace the reviewed `/etc/ssh/sshd_config.d/00-local-hardening.conf` with
`deploy/ssh/00-local-hardening.conf.example`. Preserve the current empty-password,
X11, authentication-attempt and login-grace restrictions. The effective changes
observed in the private candidate configuration are:

| Directive | Reviewed baseline | Installed candidate |
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

This helper checks the original pre-application layout, including a negative
control that expects the old permissive baseline. It is not a post-application
health check: after application use actual `sshd -t`/`sshd -T` and the installed
manifest below. Do not restore permissive settings merely to rerun this helper.

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

The subsequent host check initially found no `/home/sat/.ssh/authorized_keys` or
`.ssh` directory. The operator then supplied an Ed25519 public key explicitly for
`sat`. It was installed on 2026-09-11 at 14:00 UTC, with fingerprint
`SHA256:XtmTHGF0GHiIOw1Ui9bBX4/DGCLkTfIAi5zIc0iLUGY`.
[Installation evidence](../testing/evidence/sat-key-installation-fedora-20260911.json)
records the actual filesystem and syntax checks. Key comments and contents are
omitted from the public record; no private key was requested or accessed.

The installation validated the SSH Ed25519 wire format and SHA256 fingerprint,
verified the existing account/home identity, and exclusively created the missing
`.ssh` directory and key file. Directory-relative operations pinned the home and
new directory; existing entries were refused rather than overwritten. The new
directory is owned by `sat:sat` mode 0700 and `authorized_keys` mode 0600. File and
directory writes were synchronized. Default SELinux labels were restored and
verified as `ssh_home_t`; the account can read its key file. Installed bytes and
the `ssh-keygen` fingerprint match the supplied key. `sshd -t` passed and hashes
of the existing SSH configuration snippets and root/linuxuser key files were
unchanged. No accounts, groups or SSH daemon policy were changed or reloaded.

The effective `sat` configuration uses `.ssh/authorized_keys`, with public-key
authentication and StrictModes enabled and no AuthorizedKeysCommand. These are
configuration checks, not authentication evidence. After installation the operator
confirmed a fresh login using this specific key, with password and
keyboard-interactive fallback disabled and connection sharing bypassed. This
post-installation confirmation satisfied the pre-change login dependency. The
operator was instructed to keep that session and console recovery available.

On the machine holding the matching private key, substitute its local path:

```sh
ssh -o ControlPath=none -o IdentitiesOnly=yes -o PreferredAuthentications=publickey \
  -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no \
  -i /path/to/matching_private_key sat@64.177.40.118
```

The website Nostr identity is unrelated to SSH access. Final account/key cleanup
and privilege reduction remain separate acceptance gates.

The reviewed SSH-only application/rollback procedure is:

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

## Applied staging evidence, 2026-09-11

The exact candidate was installed and `sshd.service` reloaded at 14:07 UTC.
[Application evidence](../testing/evidence/ssh-policy-applied-fedora-20260911.json)
pins the original, installed snippet and one-time application helper. Immediately
before mutation, the original reviewer and all original configuration hashes
matched the published review, and its syntax/context/negative checks passed again.
The replacement was atomic, root-owned mode 0600, synchronized and restored to
its default SELinux label. Unrelated SSH files and account keys were unchanged.

Before and after reload, syntax and effective-policy checks passed for twelve
contexts: `sat`, `linuxuser`, `root` and the non-login runtime account, each with
synthetic IPv4/IPv6 and the known laptop address. The complete before/after
configuration comparison permitted only the reviewed authentication changes.
SSH remains active. The helper would restore the original snippet and validate/
reload it on an application failure; rollback was not needed.

The original is retained at
`/etc/ssh/lg-remediation-backup-20260911/00-local-hardening.conf`, root-owned mode
0600 in a root-only mode-0700 directory outside the active include glob. Its hash
is the original snippet SHA256 above. The directory also holds an application
record. Preserve these files; do not rerun the fresh-only application helper.

[Live method-negotiation evidence](../testing/evidence/ssh-method-probes-fedora-20260911.json)
records actual loopback connections to the reloaded listener for `sat`,
`linuxuser` and `root`, with its Ed25519 host key pinned from the local public host
key. Only `publickey` was offered and unauthenticated attempts were denied. No
password or private key was supplied. This verifies that password and
keyboard-interactive methods are not offered on those connections. It does not
test a valid root key, prove remote reachability or establish successful login;
root prohibition is verified by installed policy/context checks.

The operator confirmed a fresh external public-key-only `sat` login after reload.
A bounded SSH service journal read independently found an accepted public-key
login for `sat` with the approved fingerprint from a non-loopback source after
reload. Only matching success metadata is retained; source addresses are omitted.
This corroborates the account/key login, while the operator supplies the client
options and identity confirmation. Preserve console recovery and the backup.
If fresh access later fails, use retained access to verify the current snippet still matches
the installed hash, restore the exact original backup root-owned mode 0600,
restore its default SELinux label, run `sshd -t`, and reload `sshd.service`.
Inspect unexpected drift rather than overwriting it. No production gate is lifted.
