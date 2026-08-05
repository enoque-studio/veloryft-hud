# Veloryft documentation

English is the source language for the application, code, and documentation.

## I want to use Veloryft

| | |
| --- | --- |
| [Installation](getting_started/installation.md) | Requirements, installer or portable, verifying the download, uninstalling |
| [Game configuration](getting_started/game_configuration.md) | Turning on Data Out and pointing it at Veloryft |
| [First run](getting_started/first_run.md) | What the panels show, arranging them, why the HUD hides itself |
| [Troubleshooting](getting_started/troubleshooting.md) | The HUD does not appear, panels are off-screen, shortcuts do nothing |

## I want to look something up

| | |
| --- | --- |
| [Keyboard shortcuts](reference/keyboard_shortcuts.md) | Every shortcut and every tray action |
| [Environment variables](reference/environment_variables.md) | Every variable Veloryft reads |
| [Configuration files](reference/configuration_files.md) | What is saved, where, and in what format |
| [Telemetry formats](reference/telemetry_formats.md) | The three Forza Horizon 6 packet layouts and how they are told apart |
| [Vehicle catalog](reference/vehicle_catalog.md) | Car names, and how to override them |

## I want to understand how it works

| | |
| --- | --- |
| [System overview](architecture/system_overview.md) | The shape of the whole thing |
| [Repository structure](architecture/repository_structure.md) | What lives where, and why the workspace has two members |
| [Telemetry pipeline](architecture/telemetry_pipeline.md) | From datagram to drawable value |
| [Rendering pipeline](architecture/rendering_pipeline.md) | Layered windows, frame pacing, the performance contract |
| [Runtime data flow](architecture/runtime_data_flow.md) | Threads, shared state, shutdown, error paths |
| [Optional features](architecture/optional_features.md) | The one Cargo feature and why it is off |
| [Technology stack](architecture/technology_stack.md) | Dependencies, and what is deliberately absent |

## I want to work on it

| | |
| --- | --- |
| [Development setup](development/development_setup.md) | Prerequisites, building, running without the game |
| [Coding standards](development/coding_standards.md) | Naming, visibility, unsafe, the hot path |
| [Testing strategy](development/testing_strategy.md) | What is tested, how, and what is not |
| [Debugging](development/debugging.md) | The log, startup failures, attaching a debugger |
| [Dependency management](development/dependency_management.md) | Adding, updating, auditing |
| [Localization](development/localization.md) | English source text, the typed catalog, and adding a locale safely |

Start with [CONTRIBUTING.md](../CONTRIBUTING.md).

## I want to ship it

| | |
| --- | --- |
| [Release process](distribution/release_process.md) | Tag, what the workflow does, what to check afterwards |
| [Versioning](distribution/versioning.md) | One source of truth, and the Windows resource rule |
| [Windows installer](distribution/windows_installer.md) | Inno Setup, the AppId, upgrades |
| [Portable package](distribution/portable_package.md) | What is in the zip and why it needs nothing else |
| [Code signing](distribution/code_signing.md) | Why artifacts are unsigned, and what would change that |

## Security and privacy

| | |
| --- | --- |
| [Security model](security/security_model.md) | Trust boundaries, and the threats it does not defend against |
| [Privacy](security/privacy.md) | What is read, what is written, and how to verify nothing leaves |
| [Local HTTP server](security/local_http_server.md) | The optional loopback API and its threat model |
| [Vulnerability reporting](security/vulnerability_reporting.md) | How to report privately |

## Decisions

[Architecture decision records](decisions/README.md) — why things are the way
they are, including the alternatives that lost.

- [0001 Native Direct2D overlay](decisions/adr_0001_native_direct2d_overlay.md)
- [0002 Static CRT](decisions/adr_0002_static_crt.md)
- [0003 Public protocol boundary](decisions/adr_0003_public_protocol_boundary.md)
- [0004 Track-mapping feature boundary](decisions/adr_0004_track_mapping_feature.md)

## Legal

- [Licensing decision](legal/licensing_decision.md) — why Apache-2.0
- [LICENSE](../LICENSE), [NOTICE](../NOTICE)
- [Third-party notices](../THIRD_PARTY_NOTICES.md)
- [Trademarks](../TRADEMARKS.md)

## About these documents

Every claim here should be checkable against the code or a test. If a document
and the code disagree, the document is the bug — please report it.

Documentation is updated in the same pull request as the behaviour it describes.
A `docs/` tree that lags the code is worse than none, because it is trusted.
