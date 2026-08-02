# Security policy

## Reporting a vulnerability

**Do not open a public issue.**

Report privately through GitHub Security Advisories:

1. Go to the [Security tab](https://github.com/ESousa97/veloryft-hud/security/advisories/new)
   of this repository.
2. Choose **Report a vulnerability**.
3. Describe the issue, the affected version, and how to reproduce it.

The report stays private until a fix is published. If you do not have a GitHub
account, open a public issue titled "security contact request" containing no
details of the problem, and you will be given a private channel.

## What to include

- The version affected (Help → About, or the installer file name).
- Your Windows version.
- What an attacker can do, and what they need first — local access, a running
  browser, a program on the same machine.
- Steps to reproduce, or a short proof of concept.

## What not to include

Do not attach telemetry captures, log files, or screenshots without checking
them first. Veloryft's own logs avoid personal data by design, but a capture of
your screen or your `%APPDATA%` folder may contain your name, your machine name,
or other software's data. Redact before sending.

## Response

This is a small project maintained by one person, so no guaranteed response
times are promised — a policy that cannot be met is worse than none.

What you can expect:

- An acknowledgement that the report was received and read.
- An assessment of whether it is a vulnerability, and why.
- If it is: a fix in a release, and credit in the advisory unless you prefer
  otherwise.
- If it is not: an explanation of the reasoning, so you can disagree.

Please allow 90 days before public disclosure, or less if a fix ships sooner.

## Supported versions

| Version | Supported |
| ------- | --------- |
| Latest release | Yes |
| Anything older | No |

Veloryft is in open beta. Only the most recent release receives fixes; there are
no maintenance branches. Update before reporting.

## Scope

The following are in scope:

- The overlay binary and anything it does on a user's machine.
- The build and release pipeline in this repository.
- The optional loopback HTTP server, when the `track_mapping` feature is
  compiled in.

The following are **not** vulnerabilities in Veloryft:

- **Unsigned binaries triggering SmartScreen.** Releases are not code-signed.
  This is documented, and checksums are published with every release.
- **The loopback API being reachable by other local programs.** It has no
  authentication by design, binds `127.0.0.1` only, and is not compiled into the
  default build. See
  [`docs/security/local_http_server.md`](docs/security/local_http_server.md).
- **Settings files being readable and editable.** They are plain text in the
  user's own profile, deliberately, so that the overlay can be repaired by hand.
- **The overlay being visible to the game or to anti-cheat.** Veloryft reads
  telemetry the game broadcasts and draws a window. It does not hide itself.

## Security model

Veloryft's threat model, its network behaviour, and what it does and does not
send anywhere are documented in
[`docs/security/security_model.md`](docs/security/security_model.md) and
[`docs/security/privacy.md`](docs/security/privacy.md).
