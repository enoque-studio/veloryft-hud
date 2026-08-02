# Source licensing decision

**Decision: Apache-2.0, with trademarks reserved separately.**
**Applied. The installer EULA has been revised to match, and still needs legal
review.**

This document records the analysis behind the choice, so that it can be
re-examined rather than re-argued.

## The problem it had to solve

The repository had no source license, while the installer shipped an EULA that
forbade redistribution, sale, sublicensing, and reverse engineering.

Those two facts together meant:

- The project could not honestly be called open source.
- Outside contributions could not be accepted, because there were no terms to
  accept them under.
- Anyone reading the source had no idea what they were permitted to do with it.
- A future paid product would have inherited the ambiguity.

## Options considered

### MIT

Shortest permissive license. Anyone may do anything, with attribution.

Against it: no patent grant, and no trademark language. For a project shipping a
consumer binary under a brand name, the trademark silence is the weaker point —
it can be handled in a separate policy, but the license does not help.

### Apache-2.0 — chosen

Permissive, with two things MIT lacks:

- **An express patent grant** from contributors, plus termination of that grant
  for anyone who starts patent litigation over the work. For a project taking
  outside contributions, this removes a real ambiguity about what a contributor
  granted.
- **An explicit statement that trademark rights are not granted** (section 6),
  which is exactly the separation this project needs: the code is open, the name
  and the logo are not.

Against it: longer, and requires preserving `NOTICE`. Both are minor.

Compatibility is good — Apache-2.0 code can be used in GPL-3.0 projects, and it
is the de facto default in the Rust ecosystem, so contributors already know it.

### GPL-3.0 or AGPL-3.0

Copyleft. Derivatives must stay open, which is a legitimate goal.

Rejected because it conflicts with how this project is distributed. The owner
intends a possible paid product built on the same code; under GPL that binary's
source would have to be offered to every recipient. And the existing EULA would
have had to be rewritten far more drastically, since GPL forbids adding
restrictions of the kind an EULA typically contains.

AGPL adds a network-use clause, which is aimed at hosted services. Veloryft is a
desktop application. The clause would do nothing here except deter adoption.

### Source-available, no OSI license

Publish under "all rights reserved" with reading permitted.

Rejected, but it was the honest fallback if no decision had been made: the
project would have had to be described as source available and not open source,
outside contributions refused, and the missing license recorded as a release
blocker.

## Consequences

**Commercial use.** Anyone may use, modify and sell software based on this
source, including a competitor. That is the deliberate cost of a permissive
license, and it is offset by the trademark reservation: they cannot call it
Veloryft.

**A future paid product.** Unaffected. Apache-2.0 does not require derivatives
to be open, so a proprietary build on top of this code is permitted — including
the owner's.

**Contributions.** Accepted under Apache-2.0 by the standard inbound-equals-
outbound rule, stated in [CONTRIBUTING.md](../../CONTRIBUTING.md). No separate
CLA. A CLA would allow relicensing later but adds friction to every contribution
and is hard to justify at this size.

**Dependencies.** All permissive and compatible; `deny.toml` enforces the
allowed set. A copyleft dependency would undermine the choice, which is why the
list is checked on every pull request.

## The EULA

The installer EULA covered the prebuilt binary and contained clauses that
directly contradicted Apache-2.0: no redistribution, no sale, no reverse
engineering, personal non-commercial use only.

It has been revised so that:

- Apache-2.0 governs the source code and **prevails on any conflict**.
- The EULA governs the official prebuilt binary only.
- Its remaining substance is warranty disclaimers, beta status, privacy
  behaviour, trademark notices, and third-party trademark disclaimers — none of
  which Apache-2.0 addresses and all of which are compatible with it.
- The restriction that remains is the trademark one, which Apache-2.0
  explicitly does not grant anyway.

**It has not been reviewed by a lawyer.** It is marked as a draft, and must not
be described as final until that review happens. The revision removed a known
contradiction; it did not turn an untested document into a tested one.

## Files in place

| File | Purpose |
| ---- | ------- |
| [LICENSE](../../LICENSE) | Apache-2.0, full text |
| [NOTICE](../../NOTICE) | Attribution, as Apache-2.0 section 4 requires |
| [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) | Dependencies, their licenses, and the unresolved data provenance |
| [TRADEMARKS.md](../../TRADEMARKS.md) | What the name and logo do and do not permit |
| `installer/eula.txt` | Binary distribution terms, revised, draft |

## Still open

**`data/car_ordinals.json` has no established provenance.** It arrived in the
initial commit with no source, author, or license. This is a release blocker,
independent of the source license, and is documented in
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).

**Legal review of the EULA.** Required before it can be called final.

**Trademark registration.** The policy asserts common-law rights in the name and
logo. Registration would strengthen it, and is a separate decision with its own
cost.
