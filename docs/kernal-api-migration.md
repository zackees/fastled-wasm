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
| Tree fingerprints | zccache-fingerprint | #233 / kernal-api#157: moved content hashing and generic tests upstream; removed the zccache dependency graph |
| Paths | dirs | #236 / kernal-api#163: filesystem user-home capability preserves account fallback; five callers migrated without changing layout |
| Process lifecycle and native containment | running-process, windows-sys | #234: command runner and viewer use kernel-owned contained children; removed direct backend dependencies and native handle code |
| Async runtime, channels, clocks | tokio, tokio-stream | Use async_engine types and operations; coordinate HTTP/SSE consumer types |
| HTTP download, archive extraction, hashes | reqwest, zip, tar, zstd, flate2, sha2 | #235 tracks SHA-256 facade migration; add shared streaming APIs and move generic archive tests upstream; retain toolchain URLs/configuration here |
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

### Tree fingerprint slice

The sibling kernel branch `feat/tree-fingerprint` (commit `87e937d`,
https://github.com/zackees/kernal-api/pull/158) adds the fs-gated
`hash::blake3_tree` capability without new dependencies. Generic same-size-edit
and deletion coverage moved upstream; FastLED retains its persistent
invalidation, source-selection, and cache matrix tests. The kernel encoding is
versioned and frames relative paths and fixed-size content hashes explicitly;
existing zccache-format cache keys receive a one-time cache miss.

The default-feature application suite passes: 253 library tests, two binary
tests, one integration test, and one doctest. Python: 24 passed, one skipped.
The upstream `fs,fs-watch` suite passes, including seven tree-hash tests; a
subsequent invalid-filename regression also passes (eight focused tests). On
this host it needs `RUSTFLAGS='-C link-arg=-Wl,--build-id=sha1'` because an
existing process identity test requires the linker to emit a GNU build ID.
Strict Clippy passes in both repositories. The one-agent pre-push review found
no blockers; its documentation precision suggestion was addressed and tested.

The lockfile loses 11 packages relative to `909740b`: `zccache-fingerprint`,
`zccache-core`, `zccache-hash`, `zccache-platform`, `crash-context`,
`crash-handler`, `doctest-file`, `fs2`, `interprocess`, `recvmsg`, and
`tracing-attributes`. This is graph reduction evidence, not a build-time
comparison. Publication and exact release consumption remain pending.

### Process containment slice

Tracking: https://github.com/zackees/fastled-wasm/issues/234

The test-command runner and native viewer use kernel `spawn_sync` and its
facade-owned child and stdio types. FastLED retains command construction,
discovery labels, timeout/cancellation policy, and output delivery. The kernel
owns process groups, Windows job handles, argument quoting, and child cleanup.
The existing runner cancellation and inherited-output-pipe regressions pass;
the viewer liveness regression now covers the same facade on every platform.
Direct `running-process` and `windows-sys` dependencies are removed, although
their private kernel implementations remain in the transitive graph.

The default-feature Rust suite passes (253 library tests, two binary tests,
one integration test, one doctest), as do Python tests (24 passed, one skipped).
Strict default-feature Clippy passes with warnings denied. The Windows MSVC
headless all-targets cross-check passes with `-j1`; the initial attempt was
terminated by the shared build scheduler, not a compiler diagnostic. Native
Windows execution remains pending. Review found a kernel startup-error cleanup
gap tracked in https://github.com/zackees/kernal-api/issues/159; preserving the
old viewer's failed-resume handling is a prerequisite for release consumption.

The fix is implemented on sibling branch `feat/windows-spawn-cleanup`, commit
`f23da2a`, in https://github.com/zackees/kernal-api/pull/160 (stacked on #158).
Job creation now precedes the suspended child, owned cleanup covers later
startup failures, and failed resume is reported. Linux kernel tests and strict
Windows cross-target Clippy pass. The source ownership regression was observed
RED then GREEN. Native Windows fault-injection tests for assignment, resume,
and duplication failures are added but await CI execution. One-agent pre-push
review found no actionable issues. Neither upstream PR is a published release.

### SHA-256 slice

Tracking: https://github.com/zackees/fastled-wasm/issues/235 and
https://github.com/zackees/kernal-api/issues/161.

The sibling `feat/sha256-facade` branch adds optional `hash-sha256`, reusing
the kernel's existing private SHA implementation dependency. Owned digests,
incremental state, and bounded reader/file operations preserve all SHA-256
encodings. The application removes direct `sha2` and uses the facade for cache
keys, preprocessing, frontend/build fingerprints, archive seals, and watchers.
Archive and watcher file hashing use an explicit 16 GiB application limit and
64 KiB kernel streaming buffer. Other existing incremental application hash
encodings remain unchanged. Two generic archive hash tests move upstream;
artifact verification and FastLED cache-policy coverage remain here.

The full application suite passes (251 library tests, two binary tests, one
integration test, one doctest), as do Python tests (24 passed, one skipped).
The kernel `fs,fs-watch,hash-sha256` suite passes, including known vectors,
chunking, changed file contents, interruptions, read failures, and byte limits.
Dependency-isolation checks prove SHA is absent by default and present with
its feature. The one-agent review found no actionable issues. App strict
Clippy passes with `-j1` in both repositories; initial concurrent lint attempts failed without
preserved compiler diagnostics and required sequential retries.

Tree PR #158 has now merged after all CI checks passed. Cleanup PR #160 targets
main. Exact published release consumption remains pending for all slices.

### User-home slice

Tracking: https://github.com/zackees/fastled-wasm/issues/236 and
https://github.com/zackees/kernal-api/issues/163.

Kernel `platform::fs::user_home_dir` uses the existing private directory backend
and preserves native account fallback; the environment-only host fact API is
unchanged. FastLED's five callers retain all overrides, product path components,
and caller fallback policies. The direct `dirs` dependency is removed. The
lockfile drops its version-5 directory stack and obsolete Windows bindings.
Kernel tests cover native backend parity and nonempty/empty/missing `HOME`,
isolated in child test processes. The application suite passes (251 library,
two binary, one integration, one doctest) and Python passes (24, one skipped).
The full kernel `fs,fs-watch,hash-sha256` suite and strict Clippy in both
repositories pass. One-agent pre-push review found no actionable issues.
Archive extraction migration is next, tracked in application issue #237.

### Archive slice (in progress; not adopted)

Tracking: application #237 and https://github.com/zackees/kernal-api/issues/165.
The sibling `feat/archive-facade` branch contains an unpublished ZIP foundation:
owned format/limit types, empty caller-exclusive destination contract, input,
metadata, entry, path and output ceilings, fixed-buffer copying, and executable
bit preservation. Four focused tests and strict archive-feature Clippy pass.
The focused test first failed because the archive feature did not exist.

ZIP links now use a deferred, bounded virtual graph: relative internal chains
are preserved, while escaping/cyclic/dangling links fail before link creation.
ZIP64 preflight validates metadata before backend allocation. Eight regressions
pass, plus read-only acceptance of a cached PlatformIO ZIP extracted into fresh
temporary staging. Strict archive-feature Clippy passes. Native Windows link
creation still needs execution coverage; post-creation resolution semantics and
additional malformed ZIP64 cases remain review targets.

The draft now also handles tar.zst, full/selected tgz extraction, tar symlinks
and hardlinks, and bounded GNU/PAX metadata. Fourteen focused regressions pass.
Review exposed a local/global PAX size-state mismatch and invalid link-prefix
normalization; both were reproduced with failing regressions and fixed,
including terminal directory suffixes. The default rejects dangling links; an
explicit `PreserveMissingLeaf` policy preserves missing final targets under
existing directories without allowing missing intermediate paths or hardlinks.

Read-only acceptance passed for cached esbuild tgz member extraction and the
catalog-pinned Emscripten 4.0.21 Linux x86-64 tar.zst. The latter's SHA-256 matched
`5cd3cbe0316d37c9b39bdc63691c014f136a5d82a9f08ed29bb7ad62f7a83655`; extracted
file hashes, executable modes, and literal symlink targets matched the archive.
Its npm symlinks include literal backslashes and are dangling on Linux, requiring
the explicit policy above to preserve the old extractor's behavior. No archive
contents were executed. Fixtures were extracted only into fresh temporary dirs.

No archive dependencies or tests have been removed from FastLED yet. Application
adoption, isolated-feature CI, native Windows coverage, and upstream publication
remain pending. All installer full-extraction call sites use empty staging
directories; esbuild removes an existing selected binary before extraction.

### Final migration audit

- Resolve every inventory row with code and dependency-graph evidence.
- Move generic mechanism tests upstream while retaining application integration
  and product-policy coverage here.
- Consume an exact published kernal-api release without a local path patch.
- Enforce the final dependency boundary in CI.
- Run lint, Rust workspace tests, Python smoke tests, and native compiler/viewer
  integration checks; verify supported-platform behavior and Safari invariants.
- Record comparable clean and incremental build measurements and explain any
  remaining implementation dependencies.
