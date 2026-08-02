# Architecture decisions

Records of decisions that were expensive to make and would be expensive to
reverse — the ones where someone six months from now would otherwise look at the
code and reasonably ask "why on earth is it like this?"

An ADR is not documentation of how something works; that lives in
[architecture](../architecture/). An ADR records **why**, including the options
that were rejected and what would have to change for the decision to be revisited.

## Records

| | Decision | Status |
| --- | -------- | ------ |
| [0001](adr_0001_native_direct2d_overlay.md) | Native Direct2D overlay instead of WebView2 | Accepted |
| [0002](adr_0002_static_crt.md) | Static C runtime | Accepted |
| [0003](adr_0003_public_protocol_boundary.md) | An in-repository protocol crate instead of a private path dependency | Accepted |
| [0004](adr_0004_track_mapping_feature.md) | Track mapping behind a Cargo feature, off by default | Accepted |

## Not yet written

A record on source licensing will be added once the owner has confirmed the
choice in practice. The analysis behind it is in
[the licensing decision](../legal/licensing_decision.md), and Apache-2.0 is what
the repository currently applies.

## Writing one

Add a record when a decision:

- Constrains future work.
- Looks wrong without its context.
- Was chosen over a reasonable alternative that someone will suggest again.

Do not add one for a decision that is obvious from the code, or that can be
reversed in an afternoon.

Use the next number, and keep the shape: context, the decision, why the
alternatives lost, consequences including the bad ones, and what would make you
change your mind. A record that lists only benefits is marketing, not a
decision.

Records are not edited when a decision changes. Write a new one that supersedes
the old, and mark the old one superseded — the point is the history.
