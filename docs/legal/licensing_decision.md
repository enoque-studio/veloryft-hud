# Source licensing decision

**Decision: Apache-2.0, with trademarks reserved separately. Applied.**

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

## No separate binary EULA

The old installer required acceptance of a project-specific EULA. Even after
its contradictory restrictions were removed, it remained an unreviewed second
contract with no clear benefit for an Apache-2.0 application.

It has been retired. Source and official binaries are distributed under
Apache-2.0, with `NOTICE`, third-party notices, the warranty disclaimer already
present in the license, and a separate trademark policy. The installer displays
no click-through license page and installs those notices beside the executable.

## Files in place

| File | Purpose |
| ---- | ------- |
| [LICENSE](../../LICENSE) | Apache-2.0, full text |
| [NOTICE](../../NOTICE) | Attribution, as Apache-2.0 section 4 requires |
| [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) | Dependencies, external data attribution, and their license status |
| [TRADEMARKS.md](../../TRADEMARKS.md) | What the name and logo do and do not permit |
| `installer/veloryft.iss` | Ships the same license and notices with the binary |

## Closed provenance review

The vehicle catalog was traced to Martin Refseth's public FH6 ordinal gist. The
author states that the data was extracted from game files, but the gist declares
no reuse license. Attribution alone cannot supply that missing permission.

Veloryft therefore no longer embeds or redistributes that catalog. Users can
provide their own local mapping through the documented extension point. This
closes the repository's data-provenance blocker without making a claim about
the external dataset's license.

Trademark registration remains optional. The policy asserts common-law rights
in the name and logo; registration would strengthen them and is a separate
business decision.
