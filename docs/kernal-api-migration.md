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
| Build-only support | indexmap | Direct unused entry removed; transitive Tauri copies remain until viewer migration |
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
FASTLED_PKG_CONFIG_PATH=/nix/store/gbzc2cpjmhylnmwljbqa1pwmli5a2n1v-xz-5.8.3-dev/lib/pkgconfig:/nix/store/9vxrnnhc400s1rgq801wqfdxhzpcl1z1-bzip2-1.0.8-dev/lib/pkgconfig:$(printf '%s:' /nix/store/*-dev/lib/pkgconfig /nix/store/*/share/pkgconfig)
FASTLED_NATIVE_LIB_PATH=$(PKG_CONFIG_PATH="$FASTLED_PKG_CONFIG_PATH" pkg-config --libs-only-L gtk+-3.0 webkit2gtk-4.1 openssl liblzma bzip2 | sed 's/-L//g; s/ /:/g')
PKG_CONFIG_PATH="$FASTLED_PKG_CONFIG_PATH" LD_LIBRARY_PATH="$FASTLED_NATIVE_LIB_PATH" soldr cargo test --workspace -j1
```

These are host-specific store paths. The explicit native xz/bzip2 prefixes avoid
32-bit libraries also present in this store; an unfiltered glob selected those
and failed the x86-64 link during archive adoption.

## Completion checks

### Obsolete manifest entries

The unused workspace `thiserror` declaration and direct build dependency on
`indexmap` are removed. `build.rs` only invokes the optional Tauri build helper;
there are no application references to either crate. The default viewer-enabled
build and all 258 Rust tests pass without the direct `indexmap/std` feature
request. The lockfile removes only the application-to-indexmap edge; neither
transitive package removal nor a build-speed gain is claimed. The new manifest
boundary was observed RED before removal and GREEN afterward; Python reports
28 passed and one skipped. Tauri build support remains to be migrated.

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

### Archive slice (locally adopted; publication pending)

Tracking: application #237 and https://github.com/zackees/kernal-api/issues/165.
Upstream implementation: https://github.com/zackees/kernal-api/pull/166,
stacked on home-resolution PR #164 and SHA-256 PR #162.
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

FastLED now delegates all three extraction wrappers to the facade and removes
direct zip/tar/zstd/flate2 dependencies. Two generic ZIP tests and their fixture
builder are removed; their extraction coverage lives upstream. Promotion,
catalog checksums, esbuild member selection, and Emscripten configuration remain
FastLED policy. The lockfile moves the four backends under kernal-api, updates
tar to the facade's exact version, and drops xattr.

The application boundary regression was RED before adoption and is now GREEN.
All 249 library tests, two binary tests, one CLI integration test and one doctest
pass; strict workspace Clippy and formatting pass. Python has 24 passed and one skipped. The boundary test passes Ruff,
Black, isort and Pyright. One-agent application review found no actionable
issues. Archive-only CI and ZIP/tar/zstd dependency-isolation checks are now
included in PR #166; the local isolation checks pass. Native Windows coverage, upstream publication and
replacement of the temporary path override remain pending. All full-extraction
call sites use empty staging directories; esbuild removes an existing selected
binary before extraction.

The SHA/home/archive stack was rebased onto upstream main `1181fdf`, preserving
the newer Linux WebKit changes. `git range-diff` confirms all five migration
patches are identical after rebase. New heads are SHA `40fcb18`, home `4efac1c`,
and archive `6c32591` (including CI checks). The migrated-feature kernel suite
passes after rebase; the first attempt ended in a Soldr relay error, followed
by a successful unchanged retry. SHA-256 PR #162 and home-resolution PR #164
have now merged after their complete CI checks passed. Archive PR #166 has
been retargeted to main; its remaining release gates still apply. No release
has been published.

Archive run `34736578924` ended with a Windows native test failure in
`strict_worker_assigns_before_resume_and_job_close_reaps_tree`: the readiness
marker lacked the expected `ready:` prefix. Inspection found the helper writes
the marker non-atomically while `wait_for_text` accepts any successful read,
including empty/partial content. That race is a concrete candidate requiring
a regression and fix, not grounds to declare Windows validation complete.
Tracked at https://github.com/zackees/kernal-api/issues/168. A deterministic
test reproduces partial visibility during the original create/write sequence.
The fix stages each test marker with the existing tempfile dev dependency,
then atomically publishes without overwriting an existing marker. Two local
regressions pass, including failed-write cleanup and no-clobber behavior;
production process containment and shutdown assertions are unchanged.

Archive and HTTP branches have been rebased on merged SHA/home main `63ce8ae`;
the four archive patches compare identically before/after rebase. Positive
ZIP link-chain and tar symlink/hardlink tests now run on Windows as well as
Unix. All 14 archive regressions, both marker regressions, and the combined
archive/fs/fs-watch/hash-sha256 kernel suite pass locally. Strict Windows-target
archive Clippy (all targets) also passes. Updated archive head `8c27706` is
pushed to PR #166; native Windows execution still requires fresh CI. The HTTP
draft is rebased onto this fix. No failed job has been blindly restarted.

Run `34737499816` passed all 539 native Windows library tests, including the
previously failing containment test, then exposed failures in both newly
enabled archive symlink tests: extraction succeeded, but following the links
returned Windows error 123 (`InvalidFilename`). The raw archive targets retain
forward slashes; Windows relative reparse targets require native separators
(matching https://github.com/dotnet/runtime/issues/79031). A Windows-only
separator conversion is under test/review, preserving Unix literals and the
existing link-graph validation. Native Windows GREEN remains a release gate.
The run is now terminal: every other CI job passed. The scoped separator helper
and all 14 Linux archive regressions pass, and focused source review is clean.
Archive head `9c176b9` is pushed with this fix and portable negative link-graph
tests enabled on Windows. Strict Windows all-target archive Clippy and the
additional Windows test compilation both pass. Fresh native CI is required;
the HTTP draft is rebased onto the fix.

Run `34737890123` completed successfully on archive head `9c176b9`, including
native Windows positive/negative archive link tests. PR #166 is merged as
`299735e`. The optional native macOS ARM execution job remains skipped by the
repository's runner policy; macOS cross-compilation/archive checks passed.
This is not yet a published release, and the application path patch remains.

### HTTP client slice (draft; partial local adoption)

Tracking: https://github.com/zackees/fastled-wasm/issues/238 and
https://github.com/zackees/kernal-api/issues/167. Investigation found blocking
downloads, GitHub metadata and HEAD probes, async DWARF debug POSTs, and server
HTTP/SSE integration tests. Removing reqwest requires all of these callers,
not just downloads. Existing private kernel ureq callers do not provide a
public capability. Product URLs, schemas, fallback policy and protocol tests
remain FastLED-owned.

The sibling `feat/http-client-facade` draft adds an optional async GET transport
using private reqwest 0.12.28/native TLS without constructing a runtime. It has
owned client/response/limit types, bounded small-body collection, post-parse
header acceptance limits, finite connection/read/total timeouts, and rejects
embedded URL credentials. Redirects are returned to callers, not followed.
Five loopback tests pass after the initial missing-feature RED test, covering
status/body preservation, declared/streamed overflow, header ceilings, invalid
URLs, unfollowed redirects, and stalled headers/body. Local compilation requires
the host OpenSSL pkg-config and shared-library paths.
Strict HTTP-feature Clippy and the broader HTTP-enabled kernel test suite pass
(166 library tests plus enabled integration tests); no HTTP adoption or
cross-platform execution claim is implied by those checks.

The draft now adds facade-owned GET/HEAD/POST request types, borrowed application
headers/body, response header access, and request URL/body/header byte ceilings.
Header counts are independently bounded before request construction. Eight
focused tests pass, including HEAD entity-length handling, wire-level POST
payload/header checks, malformed header and framing-header rejection, and an
excessive-header regression observed RED then GREEN. Strict Clippy and the
broader HTTP-enabled kernel suite still pass.

Draft commit `c08555e` adds caller-buffered async body reads with a private
zero-copy pending transport chunk. Reads obey the cumulative body ceiling,
return before body completion, and stay failed after an overflow; collection
after partial reads returns only unread bytes. Eleven focused HTTP tests and
the broader HTTP-enabled suite pass, as does strict all-target host Clippy.
An impossible-deadline regression was observed RED then GREEN; client
construction now rejects zero or greater-than-365-day timeouts. These generic
mechanism tests live upstream, not in FastLED. HTTP is still not adopted.

The draft now includes blocking client/response adapters borrowing a caller-owned
kernel runtime. They reuse the async transport, implement streaming `Read`, and
create neither a runtime nor a worker. Calls from an entered runtime fail with
`WouldBlock`; tests verify rejected nested reads leave the stream usable later.
Thirteen HTTP regressions, the broader HTTP-enabled kernel suite, and strict
all-target host Clippy pass. Default-versus-enabled dependency graph checks prove
reqwest isolation, and HTTP is added to the individual-feature CI matrix.

The HTTP draft now implements opt-in redirects (default zero, maximum 32),
validating each intermediate response before following it. Cross-origin hops
drop application headers, HTTPS downgrades fail, and POST redirects use explicit
301/302/303-to-GET versus 307/308 replay semantics. Seventeen focused HTTP tests
and the broader HTTP-enabled kernel suite pass. A buffered-body deadline bypass
was reproduced RED then fixed GREEN by retaining the absolute deadline in the
response, so caller pauses cannot bypass the cumulative request deadline.
Additional redirect method/relative-location and TLS/cancellation coverage,
source review, and client adoption remain pending.
Strict all-target host HTTP Clippy also passes. The complete HTTP draft is
rebased onto merged archive main `299735e`; no HTTP PR or release is published.

Nineteen focused HTTP tests now pass. New socket-observed cancellation tests
verify that dropping a pending request or a response closes the connection;
the runtime remains running while closure is observed. A relative-redirect
matrix verifies HEAD preservation, same-origin application-header retention,
POST-to-GET for 301/302/303, and POST body replay for 307/308. Strict host Clippy
also passes. These are upstream mechanism tests; FastLED adoption is still pending.

Initial parser-allocation inspection found the locked private Hyper 1.11.0
HTTP/1 backend defaults to 100 headers and a 417792-byte read-buffer ceiling.
The facade's configurable header ceiling is still checked after parsing, and
these transitive implementation defaults are not a published facade guarantee.
That distinction must be resolved or explicitly contracted before release.

The opt-in HTTPS acceptance test passed against FastLED's public GitHub release
metadata endpoint using verified TLS, a user-agent, bounded body collection,
and the facade redirect policy. This proves trusted HTTPS on this host, not
invalid-certificate rejection or all supported platforms.

Local adoption now routes `archive::download` through the kernel's blocking
streaming client and a caller-owned kernel runtime. FastLED keeps its 120-second
total timeout, ten-redirect policy, destination creation, flush/error reporting,
and a 16-GiB artifact ceiling. A product test proves successful writes and
preservation of an existing destination on HTTP error. The boundary test failed
on the old reqwest import then passed after adoption. The existing 249-library,
2-binary, 1-integration, 1-doctest workspace suite passed, as did the new product
test and seven Python smoke/boundary tests. Enabling the feature required only
updating locked bytes 1.11.1 to the kernel's exact 1.12.1, not a broad refresh.
Other reqwest callers remain, so the direct dependency is not yet removed.
Strict workspace all-target Clippy passes, and the full Python unit suite has
25 passes/1 skip. HTTP remains a locally adopted draft pending upstream review
and release; this patch must not be released with the local path override.

The remaining blocking HTTP callers in install/project now use kernel clients:
release metadata stays bounded and is parsed as FastLED JSON, HEAD probes keep
their boolean fallback, and extension downloads reuse the streaming artifact
downloader. Project metadata now sends the same user-agent as install metadata.
There are no remaining `reqwest::blocking` imports; the app no longer enables
reqwest's blocking feature. Source and manifest boundary tests were observed
RED then GREEN. Python unit tests pass (26/1 skipped). The direct reqwest
dependency remains for async debug requests and server integration tests.
After feature removal, strict workspace all-target Clippy and the full Rust
suite pass (250 library, 2 binary, 1 integration, 1 doctest).

DWARF source-smoke POSTs now use kernel HTTP, runtime construction, and sleep;
FastLED retains the JSON payload and source-path/status policy. Reqwest has been
removed from production dependencies and remains temporarily as a dev-dependency
for server integration tests. The boundary regression failed on the old DWARF
import then passed. The full existing workspace suite and strict Clippy passed;
an additional real-local-server source-smoke test also passes for both a mapped
source and its missing-file error. This is HTTP/source-resolution validation,
not a WASM execution or Safari test. Server-test migration is still required
before removing reqwest entirely.

Server integration tests now use the kernel HTTP client, including POST bodies,
authorization headers, response headers, and caller-buffered SSE reads. Reqwest
is removed from both direct dependency lists; the boundary test was observed RED
then GREEN. All 21 endpoint tests and the full workspace suite pass (251 library,
2 binary, 1 integration, 1 doctest), as do Python unit tests (26/1 skipped)
and strict workspace all-target Clippy.
The lockfile drops now-unused HTTP/2, charset, and secondary TLS dependencies;
reqwest remains a private transitive dependency, not an application import.
No build-speed improvement has yet been measured.

HTTP remains a locally adopted draft. Upstream review, TLS rejection/downgrade
fixtures, the pre-allocation metadata contract, and supported-platform CI still
need resolution. No HTTP PR or release is published, and the temporary local
path override must be removed before release.

Pre-publication HTTP review found that 307/308 could replay a POST body across
origins after stripping its headers. A socket-observed regression reproduced
the second connection, then passed after rejecting cross-origin POST replay;
same-origin replay and 301/302/303 conversion remain covered. Twenty local HTTP
tests pass (one external trusted-HTTPS test is ignored by default). Review also
identified automatic decompression drift under downstream Reqwest feature
unification. A gzip-enabled regression failed when the backend removed the
encoding header, then passed after disabling all four automatic decoders.
Twenty-one HTTP tests and strict all-target Clippy pass with gzip enabled; a
native CI step now exercises this feature-unification regression after locked
checks, resolving its deliberately additional backend feature independently.
No production dependency change is needed. The broader HTTP-enabled kernel
suite and FastLED's full 255-test Rust workspace suite also pass. Local TLS
rejection/hostname/downgrade coverage remains outstanding. These are upstream
mechanism tests, not FastLED protocol tests.

Four deterministic loopback TLS tests now cover trusted HTTPS, default rejection
of an untrusted certificate, rejection of a trusted certificate with the wrong
hostname, and downgrade rejection before any plaintext connection. A public
test-only PKCS#12 identity and certificate are checked in upstream; trust is
scoped to the test client, never installed in the OS. Disabling verification
and the downgrade guard produced three expected failures; restoring them makes
all four tests pass. Strict HTTP-feature Clippy and the broader kernel suite
also pass. These tests run in the existing native all-features CI lanes, but
supported-platform results and the metadata-allocation contract remain pending.

The metadata contract is now documented upstream with a distinction between
application acceptance limits, private parser thresholds, and allocator/RSS
usage. Hyper 1.11.0 is an exact optional private dependency so published
consumers use the audited parser. Regressions reject excessive response heads,
field counts, trailers and chunk extensions even with relaxed application
limits; the version-pin guard was observed RED then GREEN. Twenty-two HTTP
integration tests, 170 kernel library tests, the other enabled integration
suites, strict Clippy and dependency-isolation checks pass. Local review is clean.

Kernel [PR #169](https://github.com/zackees/kernal-api/pull/169) is open;
[CI run 34740069309](https://github.com/zackees/kernal-api/actions/runs/34740069309)
is in progress. FastLED explicitly updates its locked Hyper 1.9.0 to 1.11.0;
Cargo also reselects already-locked Windows dependency edges without adding
package versions. The full FastLED Rust suite (255 tests), strict all-target
Clippy and three boundary tests pass. Publication and removal of the temporary
local path override remain pending, as does the rest of the dependency inventory.

### Async runtime and coordination slice

Issue #239 tracks remaining Tokio ownership. All application runtime construction
now uses kernel `RuntimeBuilder`/`Runtime::run`; basic sleeps and relative
timeouts in commands, the test runner and server use `async_engine` operations.
The source boundary failed before migration and passes afterward. FastLED keeps
its existing timeout values, exit behavior and test-runner policy. The full
255-test Rust workspace suite, strict all-target Clippy and Python suite
(27 passes/1 skip) pass.

At that initial stage, Tokio and tokio-stream remained direct dependencies. Work included
channel types (notably blocking sends from output-reader threads), broadcast/SSE,
signals, semaphore try-acquisition, absolute timers, tasks and server I/O.
Kernel async_engine explicitly leaves select/attribute macro calls as a separate
macro-boundary decision; this slice does not add a generic race combinator or
claim those dependencies have been removed.

Task launch/handles and unbounded test-event channels now use kernel-owned
types. The application-specific command-task Drop guard is removed because
kernel Task already cancels on drop; the HTTP server explicitly detaches its
task to retain its existing runtime-owned lifetime. Test-runner command and
output-drain deadlines use kernel Deadline. The cancellation product regression
now waits for a READY event, covers explicit cancellation and handle drop, and
waits beyond normal command completion before checking for escaped-shell output.
The async source boundary was observed RED then GREEN. Bounded channels then
needed a kernel blocking-send operation for the synchronous reader threads.
All 255 Rust tests, strict all-target Clippy and 27 Python tests pass (one
Python test skipped) after this task/channel migration.

Bounded channels now use the kernel blocking-send operation (upstream #170,
PR #171). Shared deadline waits, periodic viewer monitoring and non-waiting
semaphore admission use the kernel operations from #172 / PR #173. The app
retains its 100 ms cadence, timeout priorities and four-slot admission policy;
its endpoint test verifies HTTP 429 on saturation and admission after release.

Broadcast delivery and SSE stream adaptation now use the kernel API from #174.
The direct `tokio-stream` dependency is removed. The optional kernel
`event-stream` feature owns stream interoperability, without a pump task or
additional queue. Generic tests cover fanout, exact entry capacity, explicit
lag counts, ordered recovery, closure and cancellation. FastLED retains event
JSON and its existing explicit filtering of SSE lag notifications. Entry
capacity is not a payload-byte bound. The source/dependency boundary was
observed RED before adoption and GREEN afterward.

The lockfile updates only `futures-core` 0.3.32 -> 0.3.34 and `tokio-stream`
0.1.18 -> 0.1.19 to meet exact upstream requirements. Cargo also reselects
already-locked Windows dependency edges; no other package versions change.
Tokio remains direct for signals, server I/O and macro call sites. Upstream
merge/release, exact registry adoption and the other inventory rows remain
outstanding; the local path patch is still migration-only.

### Screenshot persistence adoption

Screenshot persistence now uses `platform::fs::AsyncFileIo` from the local
kernel HTTP-server development branch (upstream #176). One shared budget per
server admits four native writes, each accepting at most 64 MiB with a 30-second
deadline. Saturation is reported as a screenshot failure; there is no additional
waiting-task queue. Native work still running after cancellation retains its
permit until it ends. Writes remain non-atomic and can leave partial output on
failure or timeout. FastLED retains PNG validation, authorization, preconfigured
destination selection and saved/failure events. Direct Tokio write and directory
creation calls are removed and banned by the boundary test. The HTTP server
itself still uses Axum/Tower/Tokio pending the complete upstream contract and
route-parity work. This remains migration-only path-patch adoption, not a
published release.

Validation: the screenshot endpoint covers successful persistence and native
write failure reporting. All 255 Rust workspace tests and strict all-target
Clippy pass; Python reports 27 passed and one skipped. The boundary test was
observed RED before removing the Tokio calls and GREEN afterward. One local
review covered the Rust, Python and documentation changes with no findings.

### HTTP transport adoption draft

The current local draft replaces Axum/Tower routing with kernel HTTP serving.
FastLED retains the route table, JSON payloads, authorization, screenshot names,
sleep scheduling, MIME mapping, source-root policy and browser headers. Files
and worker-prefix injection now stream through kernel response bodies; build
events use kernel SSE encoding. The source boundary bans direct Axum, Tower HTTP,
Tokio filesystem reads and Tokio listeners. Axum and its private routing helpers
leave the lockfile; Tower HTTP remains transitive through other facilities.
The only updated package version is `http-body-util` 0.1.3 -> 0.1.5, matching the
kernel's exact requirement.

This is not release-ready adoption: native path selection/canonicalization still
run synchronously as in the baseline; native CI and exact published kernel
adoption remain required. Opening and response metadata preparation use one
shared kernel `FileResponses` pool with four slots and a 30-second deadline.
Workers retain admission after cancellation until native work really ends.
Kernel transport limits
now bound requests, responses, streams, connections and native reads. The app
retains a 64 MiB request limit. Handler and absolute connection deadlines both
cover the largest accepted test wait/interval plus 60 seconds, with a one-hour
minimum; they no longer cut off an accepted long sleep after one hour.
Parser-generated errors remain outside application header policy.

Validation: all 257 Rust workspace tests and strict all-target Clippy pass;
Python reports 27 passed and one skipped. The raw-wire Axum baseline remains
green with kernel serving. Review identified blocking file preparation and
deadlines shorter than accepted test schedules; both are fixed and re-reviewed.
Generic file-preparation and cancellation-admission regressions live upstream;
the application retains a regression covering its maximum accepted sleep.

### Final migration audit

The Axum baseline now has a raw-wire parity test for missing-file 404,
unsupported-method 405, HEAD content length with no body, browser-isolation and
cache headers, wildcard CORS, and OPTIONS preflight methods/headers. This test
passes before transport replacement and must remain green after adoption.

- Resolve every inventory row with code and dependency-graph evidence.
- Move generic mechanism tests upstream while retaining application integration
  and product-policy coverage here.
- Consume an exact published kernal-api release without a local path patch.
- Enforce the final dependency boundary in CI.
- Run lint, Rust workspace tests, Python smoke tests, and native compiler/viewer
  integration checks; verify supported-platform behavior and Safari invariants.
- Record comparable clean and incremental build measurements and explain any
  remaining implementation dependencies.
