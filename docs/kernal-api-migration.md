# kernal-api migration

Tracking: https://github.com/zackees/fastled-wasm/issues/229
Upstream: https://github.com/zackees/kernal-api/issues/155

The target is for fastled-wasm to own FastLED business logic while kernal-api
owns reusable capabilities and their implementation dependencies and tests.
Backend crate re-exports do not satisfy this boundary. Dependency count alone
does not prove faster builds; measure clean and incremental builds after the
capability migration, with the same toolchain, features, and cache conditions.

## Capability inventory

| Capability | Current direct dependencies | Migration work |
| --- | --- | --- |
| Advisory locks | fs2 | #230: all six call sites migrated to kernal-api guards; headless Rust suite passes |
| Glob matching and watchers | globset, notify | #231/#232: migrated to filesystem patterns and fs-watch; lost watches trigger recovery and rescan |
| Paths and tree fingerprints | dirs, zccache-fingerprint | Adapt existing filesystem/hash facade; preserve content-authoritative invalidation |
| Process lifecycle and native containment | running-process, windows-sys | Adapt semantic process API; preserve pipe draining, cancellation, and descendant cleanup |
| Async runtime, channels, clocks | tokio, tokio-stream | Use async_engine types and operations; coordinate HTTP/SSE consumer types |
| HTTP download, archive extraction, hashes | reqwest, zip, tar, zstd, flate2, sha2 | Add shared streaming APIs and move generic archive tests upstream; retain toolchain URLs/configuration here |
| HTTP server and SSE | axum, tower-http | Add shared server capability; retain routes, payloads, and FastLED policy here |
| Native viewer and terminal | tauri, tauri-build, gtk, webkit2gtk, crossterm | Extend semantic hosting as needed for test capture, microphone, graphics fixes, keyboard handling |
| Compiler provisioning and C++ parsing | ctcb-core, tree-sitter, tree-sitter-cpp | Move reusable tool/parse mechanisms; retain FastLED build and preprocessing policy |
| CLI, configuration, and utility support | clap, serde, serde_json, toml, anyhow, strsim, shell-words, tempfile, getrandom | Establish facade operations appropriate to actual consumers; remove obsolete dependencies |
| Build-only support | indexmap | Determine whether still required by build graph |
| Python launcher/tool provisioning | meson, ninja, uv, typeguard, zcmds_win32 | Audit runtime necessity and move shared provisioning into the base without breaking wheel installation |

## Development state

Work is on `feat/kernal-api-migration`. The migration uses an exact `=0.1.0`
declaration plus a temporary sibling path patch to
`../fastled-wasm-extern/kernal-api`, initially checked out at
`76172bf3ee41ec601d3fa834291dfbe6bfc11789`. Remove the path patch and verify
an exact published release before landing on a release branch. The application
MSRV now matches its existing 1.95 toolchain and kernal-api's requirement.

The boundary test failed before each dependency migration and passed afterward.
All Python unit tests pass (24 passed, one skipped). The default-feature Rust
suite passes (255 library tests, two binary tests, one CLI integration test,
one doctest), including lost-watch recovery, watcher teardown, and existing
cache contention coverage. The headless suite also passed before the final
teardown test was added. Headless and default-feature Clippy pass with warnings denied. Rust
builds on this NixOS host need the installed OpenSSL/GTK/WebKit/liblzma
pkg-config and shared-library search paths, plus the bzip2 library path.
The new Python boundary check passes Ruff, Black, isort, and Pyright.
No build-speed improvement has been established yet. Cross-platform execution,
the complete repository lint script, and native viewer/browser smoke checks
remain part of the final migration audit.

To reproduce the default-feature Rust test run on this NixOS host:

```bash
FASTLED_PKG_CONFIG_PATH=$(printf '%s:' /nix/store/*-dev/lib/pkgconfig /nix/store/*/share/pkgconfig)
FASTLED_NATIVE_LIB_PATH=$(PKG_CONFIG_PATH="$FASTLED_PKG_CONFIG_PATH" pkg-config --libs-only-L gtk+-3.0 webkit2gtk-4.1 openssl liblzma | sed 's/-L//g; s/ /:/g')
FASTLED_BZIP_LIB_PATH=$(printf '%s:' /nix/store/*bzip2*/lib)
PKG_CONFIG_PATH="$FASTLED_PKG_CONFIG_PATH" LD_LIBRARY_PATH="$FASTLED_NATIVE_LIB_PATH:$FASTLED_BZIP_LIB_PATH" soldr cargo test --workspace
```

## Completion checks

- Resolve every inventory row with code and dependency-graph evidence.
- Move generic mechanism tests upstream while retaining application integration
  and product-policy coverage here.
- Consume an exact published kernal-api release without a local path patch.
- Enforce the final dependency boundary in CI.
- Run lint, Rust workspace tests, Python smoke tests, and native compiler/viewer
  integration checks; verify supported-platform behavior and Safari invariants.
- Record comparable clean and incremental build measurements and explain any
  remaining implementation dependencies.
