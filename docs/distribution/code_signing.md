# Code signing

## Current state

**Veloryft releases are not code-signed.**

No Authenticode certificate exists for this project, so nothing in the release
pipeline signs anything. The release workflow says so explicitly in its output
rather than leaving it ambiguous.

Nothing in the documentation, the release notes, or the installer claims
otherwise. If you find something that does, it is a bug — report it.

## What that means for users

Windows treats an unsigned executable from an unknown publisher with suspicion:

- **SmartScreen** shows "Windows protected your PC" on first run. Getting past
  it takes **More info** → **Run anyway**.
- **Browsers** may warn during download, or refuse without an explicit override.
- **The UAC prompt**, if an all-users install is chosen, shows "Unknown
  publisher" rather than a name.
- **Some antivirus products** flag unsigned binaries from small publishers on
  reputation alone, not on behaviour.

None of these is evidence about the file's contents. They are what an unsigned
binary looks like.

## Checksums instead

Every release publishes `SHA256SUMS`. This is what lets you confirm you have the
file that was built:

```powershell
(Get-FileHash .\Veloryft-Setup-0.2.0-beta.1.exe).Hash.ToLower()
```

Compare with the matching line. If they differ, do not run it.

### What a checksum does and does not prove

**Proves:** the file was not corrupted or altered between the release page and
your disk.

**Does not prove:** who built it. Anyone who could replace the artifact could
also replace `SHA256SUMS`. The checksum is a transport integrity check, not an
identity check. That is exactly the gap a signature fills.

The release history helps: an artifact is built by a public workflow from a
tagged commit, and the workflow logs are public. That is provenance you can
inspect, not provenance Windows can verify.

## If a certificate is obtained

Sign both artifacts. The place is already marked in
`.github/workflows/release.yml`, between building the packages and generating
the checksums.

What that would need:

1. **An OV or EV Authenticode certificate.** OV builds SmartScreen reputation
   over time; EV starts with reputation immediately and requires hardware token
   storage.
2. **The signing key stored securely.** A cloud signing service, or a hardware
   token attached to a self-hosted runner. Never a PFX file in the repository or
   in a plain repository secret.
3. **Signing added to the release job**, with `signtool` and a timestamp server
   — without a timestamp, signatures stop validating when the certificate
   expires.
4. **The secrets scoped to the release workflow only.** Never available to
   anything a pull request can trigger, because a pull request from a fork runs
   attacker-supplied code.
5. **Every document that says "unsigned" updated**, including this page,
   [installation](../getting_started/installation.md), and the release notes.

Do not add a signing step that silently does nothing when the secret is absent.
An artifact that is sometimes signed and sometimes not, with no way to tell, is
worse than one that is honestly never signed.

## Other options

**Signing the checksums with a personal key** (GPG or minisign) proves who
produced them and closes the gap above for anyone willing to import the key. It
does nothing for SmartScreen, but it is cheap and needs no certificate
authority.

**Build provenance attestation** — GitHub's `attest-build-provenance` — produces
a signed statement that the artifact came from a specific workflow run on a
specific commit. Verifiable with `gh attestation verify`. Again nothing for
SmartScreen, but strong provenance for anyone who checks.

Both are worth doing regardless of whether a certificate is ever bought. Neither
is implemented yet.

## Owner decision

Buying a certificate is a cost decision with a real user-experience benefit. The
options:

| Option | Cost | Effect |
| ------ | ---- | ------ |
| Stay unsigned | None | SmartScreen warning on every release; documented |
| Sign the checksums | None | Provenance for the technically inclined; no SmartScreen change |
| Build attestation | None | Same, verifiable through GitHub tooling |
| OV certificate | Low hundreds per year | Warning fades as reputation builds |
| EV certificate | Higher, plus a token | Immediate reputation; needs a self-hosted runner or a cloud signing service |

Until one is chosen, the honest position is the current one: say plainly that
the artifacts are unsigned, and publish checksums.
