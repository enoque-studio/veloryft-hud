# Trademark policy

The Apache License 2.0 covers the **code** in this repository. It explicitly does
not grant trademark rights (section 6). This file states what that means in
practice.

## What is a Veloryft trademark

- The name **Veloryft**.
- The Veloryft logo and wordmark, in every variant, including everything under
  `assets/brand/` and the installer artwork generated from it in
  `installer/assets/`.

These are not licensed under Apache-2.0. They are included in the repository so
that official builds can be produced, not so that they can be reused.

## What you may do without asking

- Use the software, for any purpose, including commercially.
- Fork, modify, and redistribute the source under Apache-2.0.
- State factually that your project is based on, derived from, or compatible
  with Veloryft — for example, "a fork of Veloryft" or "works with Veloryft".
- Use the name in documentation, articles, reviews, and comparisons.

## What needs permission

- Distributing a modified build **under the Veloryft name** or with the Veloryft
  logo. Rename your fork and replace the artwork, so that users can tell whose
  build they are running and know who to report problems to.
- Using the name or logo in a way that suggests affiliation, sponsorship, or
  endorsement by this project.
- Registering domains, package names, or accounts that could be mistaken for
  official Veloryft channels.

If you fork this repository and distribute binaries, replace the contents of
`assets/brand/`, regenerate the installer artwork with
`scripts/generate_installer_assets.py`, and change the product name in
`installer/veloryft.iss` and `build.rs`. Also change the installer `AppId`: it
is the identity Windows uses for upgrades, and reusing it would make your build
overwrite an official install.

## Third-party trademarks

"Forza", "Forza Horizon", "Forza Motorsport", "Xbox", and "Microsoft" are
trademarks of Microsoft Corporation. "Turn 10 Studios" and "Playground Games"
are names of their respective owners. Vehicle manufacturer and model names are
trademarks of their respective owners.

**Veloryft is not affiliated with, sponsored by, or endorsed by any of them.**
Every use of those names in this project is descriptive: it identifies which
game the software reads telemetry from, or which vehicle a telemetry packet
refers to.

## Questions

Open a discussion on the repository. Permission requests are handled
case by case; a clear description of what you want to distribute and under what
name is enough to start.
