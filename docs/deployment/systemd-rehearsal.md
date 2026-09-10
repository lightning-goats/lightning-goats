# Isolated systemd and encrypted-credential rehearsal

Production remains HOLD. This test runs the actual release binaries under the
shipped canary service sandbox with synthetic encrypted credentials. It does not
start a physical gateway, contact Strike, enable a service or change host routing.
Use only a trusted archive and the exact source SHA recorded in its BUILD-INFO.

On a staging host with systemd 250 or newer, inspect/initialize its host credential
store using the guarded helper. It never replaces an existing key, rejects
insecure/symlink keys and refuses initialization when encrypted credentials exist
without the original key. Recover that original key instead of discarding it.
The newly initialized root-owned key stays on the staging host after the test;
only its ownership/mode metadata is printed. Do not use this as a production
credential migration procedure.

```sh
sudo python3 -B deploy/scripts/initialize-staging-credential-store.py
sudo unshare --net -- python3 -B deploy/scripts/rehearse-systemd.py \
  /path/to/trusted-release.tar.gz FULL_SOURCE_SHA > SYSTEMD-REHEARSAL.json
```

The harness requires a fresh namespace containing only loopback. Its transient
system services join that same namespace using `NetworkNamespacePath`. It reserves
unique `lg-rehearsal-<uuid>` names and new configuration/state/runtime directories.
It uses the existing non-root `daemon` and `bin` fixture identities by default;
no accounts are created. Final production identities remain a separate gate.
It preserves the canary templates' sandbox, capability restrictions, directory
modes, restart policy and repeated gateway IP allow properties. Only installation
paths, fixture identities, logging and synthetic credential sources are remapped.
The `[Unit]` network dependencies and `[Install]` enablement are not applied.

It encrypts synthetic values under their exact credential names, removes the
plaintext input files, and delivers only `LoadCredentialEncrypted` through the
actual system manager. An `ExecStartPre` probe verifies exact credential names and
values without printing them, inaccessible other-service credentials, read-only
code/config, writable state, namespace identity, PrivateTmp, ProtectHome and zero
effective capabilities. An additional gateway restart while the daemon stays
running checks the opposite credential direction. Each role must have a startup
probe while the other application is already running; an absent credential path
at gateway-first startup alone cannot establish isolation. Wrong-name and corrupt ciphertext must fail at systemd's
CREDENTIALS step (exit 243) before `/usr/bin/true` can execute successfully.

The common installed-release checks run unchanged against the real processes:
six discovery routes, zero daemon credit, sanitized temperature conversion and
one total mock UUID-echo owner command across duplicate requests and stop/restart.
All five service starts must have zero automatic restarts. State/runtime modes,
actual UID/GID and source/archive/binary/template/harness hashes are recorded.
Success is emitted only after the temporary units are collected and test files
are removed. Unit journals may retain synthetic test diagnostics.

On SELinux hosts, disposable binary labels are matched to the default shipped
`/usr/local/bin/<binary>` destination using `matchpathcon` and `chcon` on only the
temporary copies. No SELinux policy, boolean or enforcement mode is changed.
Fedora's default `bin_t` executables run in `unconfined_service_t`; this verifies
the systemd sandbox, not an application-specific SELinux confinement policy.
Keep SELinux enforcing and use `restorecon` on final installed paths as part of
reviewed installation. Moving a binary with a `/run` or home label is not equivalent
to installing it at its correctly labelled destination. See [Red Hat's SELinux
service-domain explanation](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/7/html/selinux_users_and_administrators_guide/sect-security-enhanced_linux-targeted_policy-unconfined_processes).

CI retains the Ubuntu 22.04 package checks and runs this additional systemd job on
Ubuntu 24.04 because encrypted credentials need systemd >=250. It downloads the
same tested package, checks its complete outer SHA256SUMS and inner provenance,
and retains separate `staging-systemd-<merge SHA>` evidence with its checksums.
The PR head and GitHub synthetic merge/artifact source must both be recorded.

This closes a mock installation gap. Final production identities, credential
scopes, removal of broad deployment privileges, actual owner contract, trusted
network containment, authoritative website/browser, public TLS/IPv6 and signer
acceptance remain open. The host maintenance record also requires the pending
kernel reboot before final acceptance; no reboot is performed by this harness.
