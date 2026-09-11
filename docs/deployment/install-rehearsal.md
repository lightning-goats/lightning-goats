# Isolated installed-release rehearsal

Production remains HOLD. This rehearsal verifies a trusted release package under
root-owned files and separate unprivileged processes. It creates no host users or
services, modifies no host network interface/rule, and reads no real credentials.
It does not substitute for final systemd/TLS/credential/household acceptance.

Use only an archive produced from an explicitly reviewed/trusted source. Hashes
and a matching BUILD-INFO establish consistency; they do not authenticate an
untrusted supplier. The archive is verified before any binary executes, and no
archive binary executes as root. The existing archive smoke verifier now exposes
its extraction/validation separately so both smoke and installed rehearsal share
the same path, size, checksum and source checks.

```sh
sudo unshare --net -- python3 deploy/scripts/rehearse-install.py \
  /path/to/trusted-release.tar.gz FULL_SOURCE_SHA
```

The launcher refuses the host network namespace or any namespace containing an
interface other than loopback. It brings up only that new namespace's loopback.
The default fixture identities are existing `daemon` and `bin` accounts; override
with `--daemon-user` and `--gateway-user` if those distinct non-root users/groups
are unavailable. It creates no accounts and does not change their privileges.
They are test identities, not the eventual production service users.

Inside a disposable directory it:

- verifies and extracts the complete archive, copies root-owned mode-0755 binaries
  and creates root-owned configuration files readable only by their runtime group;
- creates separate mode-0700 runtime state directories and root-owned synthetic
  credential files readable only by the appropriate runtime group;
- verifies each identity cannot write any installed binary/config or read the
  other's credential, then launches each actual binary using `setpriv` with no
  supplementary groups and `NoNewPrivileges`;
- checks actual process UID/GID, zero effective capabilities, migrations, health,
  all six LNURL discovery routes, gateway status and Celsius-to-Fahrenheit display;
- submits one UUID only to its in-process harmless canary owner, checks duplicate
  and post-restart replay return the original result with exactly one total mock
  command, and confirms daemon credit remains zero throughout;
- checks state-file ownership/mode, terminates its processes, and removes its
  temporary installation without altering the host's existing services/files.

The JSON result records full source SHA, installed binary hashes, identity and
permission checks, namespace scope, route checks and mock command count. The
Deployment workflow runs this against its release archive and retains
`INSTALL-REHEARSAL.json` with the outer package SHA256SUMS. On a PR, the archive's
source SHA is GitHub's tested merge SHA, not the PR branch head. Keep both in the
acceptance record.

This is an installed-release rehearsal with file-based synthetic credentials.
It does not exercise `LoadCredentialEncrypted`, the shipped systemd sandbox under
final dedicated identities, real credential scopes, public TLS/IPv6, authoritative
website/browser, signer/relay, remote network containment or physical completion.
Those must still be proven on the final reviewed installation with authorized
source/host access. Broad development sudo remains a separate pre-cutover gate.

An additional actual-systemd test is documented in `systemd-rehearsal.md`. It reuses
these process-level checks with transient units and encrypted synthetic credential
mounts. Its separate evidence narrows the systemd gap; it retains all final live
acceptance and production-privilege gates.
