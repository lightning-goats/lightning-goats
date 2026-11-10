# Read-only native recipient and DNS qualification

`examples/check_cyberherd_recipient.rs` uses the same `Recipient::fetch` and
`public_network` code as the payout runtime. It never opens an application journal,
loads a credential, follows an invoice callback, creates an invoice or uses a
provider/signing API. It refuses a systemd credential environment. The deployment
job builds it into the separately checksummed test-only artifact as
`READ-ONLY-check-recipient`; it is not a production daemon or activation command.
This source is prepared; installed network results remain unqualified.

After source/build/artifact verification, install the helper into a fresh protected
prefix and use temporary one-shot systemd units under the actual payout identity.
Retain its reviewed private-network denial, capability/filesystem protections,
128 MiB memory limit and zero privileges. Hide all production configuration,
journals, IPC and credential-store paths. Load no systemd credential. Keep exact
temporary unit bytes/hashes and stop/remove only those copies after checks.

The positive command is:

```sh
/ROOT_OWNED_HELPER/READ-ONLY-check-recipient public herd@lightning-goats.com
```

It uses actual public DNS, approved-address pinning, normal certificate validation,
HTTPS port 443, no proxy/redirects, bounded bodies/time and typed recipient metadata
validation. It prints only acceptance, public amount bounds and advertised Nostr
support; no callback URL/token, raw metadata, request ID or credential is printed.
An ordinary LNURL recipient without Nostr support remains valid. No callback is
requested and no invoice is created.

For negative tests, create a root-owned read-only **fixture hosts file** with:

```text
127.0.0.1 localhost
::1 localhost
127.0.0.1 private-v4.cyberherd.example.net
fd00:abcd::1 private-v6.cyberherd.example.net
1.1.1.1 mixed.cyberherd.example.net
127.0.0.1 mixed.cyberherd.example.net
```

Bind that file over `/etc/hosts` **inside only the temporary unit's mount namespace**
using `BindReadOnlyPaths=PROTECTED_FIXTURE_HOSTS:/etc/hosts`. Do not edit the host's
DNS, hosts file, routing or firewall. The negative command is:

```sh
/ROOT_OWNED_HELPER/READ-ONLY-check-recipient private-dns-fixture
```

The three syntactically valid public DNS names must resolve through actual native
lookup to private IPv4, private IPv6 and mixed public/private sets. Only the exact
native DNS-admission refusal passes. NXDOMAIN, timeout, connection/TLS errors or
a successful fetch cannot count as private-address enforcement. The refusal
occurs before HTTP-client creation or connection. An example unit test covers
that distinction; its execution is explicit in deployment CI.

Private success evidence should retain source, artifact/helper/unit hashes,
selected public recipient and sanitized reports. Production journals, units and
receive process must remain unchanged. A positive GET establishes only discovery
for that recipient and observed DNS/TLS path. It does not qualify invoice/payment
execution, all recipients, IPv6 public availability, global firewall/forwarding,
household signing, browser presentation or recovery adoption. Those remain
separate requirements; no result authorizes spending or publication.
