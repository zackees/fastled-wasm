# Interactive terminal

Open **Terminal** to use your shell, including `clud ...`. Its initial directory
is the directory you named on the command line: the sketch directory for
`fastled <sketch>`, and the served directory for `fastled --serve-dir <dir>`.
Running `fastled` from inside the sketch therefore changes nothing, and running
it from elsewhere no longer starts the shell somewhere unrelated. A relative
directory argument is resolved against the launch directory, which is what a
relative path means on the command line. The CLI loopback server owns the PTY.
The shipped Tauri
viewer and browsers use the same server and transport. Static output hosted
elsewhere cannot open a shell. Use the exact `http://127.0.0.1:PORT` URL printed
by FastLED; terminal access rejects hostname aliases.

**Close** hides the terminal while its session continues. Reopening preserves
the session and bounded 5,000-line scrollback. **Copy** copies selected text or
the buffer. Escape and Ctrl+C belong to shell applications. After shell exit or
disconnection, **Restart** starts a new shell in that same sketch directory.
Reloading or closing the page disconnects its shell; there is no reattachment
across reloads. The dialog fits desktop/mobile viewports and forwards resizing.

Sketch stdout and console log/warn stay in the existing canvas-click output
popup. They never enter the shell stream: mixing logs with a full-screen agent
application would corrupt its display. Console errors remain native.

## Shell environment

Unix launches `$SHELL -l -i`, falling back to `/bin/sh` when SHELL is unset or
empty. An invalid configured shell fails visibly. It inherits the launch
environment and reads login/interactive startup files. FastLED sets
`TERM=xterm-256color` and supplements PATH with `~/.local/bin` (uv tool's normal
entrypoint directory) and standard system directories. Existing
`~/.nix-profile/bin` and `/run/current-system/sw/bin` are also included: NixOS
does not put its tools in `/usr/bin`, and inherited profile guards can prevent
login files from restoring PATH after a desktop launcher strips it. Windows
uses COMSPEC (or cmd.exe), native ConPTY, and inherited PATH plus `~/.local/bin`.

The server constructs this environment independently of the viewer's desktop
launch. User startup files retain control and can override PATH or change cwd;
FastLED does not rewrite them. Custom PATH configuration should retain
`~/.local/bin`. Missing `clud` remains a normal shell error; no tool is installed
automatically. Production code never embeds the motivating user's home path.

## Transport and lifecycle

`GET /terminal/ws` requires Origin and Host to match the exact bound loopback
HTTP address. Missing/null/foreign origins and hostname aliases are rejected,
even though static server routes permit CORS. Each connection owns a PTY;
four simultaneous sessions are allowed. No command, cwd or environment is
accepted in the handshake.

Client JSON messages:

- `{"type":"input","data":"pwd\r"}`: UTF-8 stdin.
- `{"type":"binary","data":[255]}`: xterm legacy binary input.
- `{"type":"resize","cols":100,"rows":30}`: resize, columns 2–500, rows 1–300.
- `{"type":"ack"}`: acknowledge an output chunk after xterm renders it.

Server binary frames preserve raw bytes, including split UTF-8. Text frames
report `{"exit":0}` or `{"error":"..."}`. Maximum message size is 64 KiB;
pastes are split at Unicode codepoint boundaries and paced through a bounded
client queue. Backend queues hold at most 32 items each, and at most 16 chunks await rendering
acknowledgements. Protocol errors or full input queues disconnect instead of
dropping keystrokes. PTY reads and writes use dedicated threads. A separate
supervisor observes disconnect even if a foreground program stops reading stdin,
then kills/reaps the shell. Unix cleanup terminates the PTY foreground job and
hangs up the shell so it can notify its background jobs. Deliberately daemonized
or disowned programs are outside the terminal lifecycle, as in a normal shell.

## Verification without WASM compilation

```sh
soldr cargo test --workspace terminal_240
bash lint
bash test
FASTLED_TERMINAL_BINARY=/absolute/path/to/target/debug/fastled \
FASTLED_ESBUILD=/absolute/path/to/esbuild \
uv run --with playwright pytest tests/frontend/test_terminal.py -v -s
```

The fixture bundles the real vendored terminal and CSS with esbuild, then starts
the real server/PTY from a temporary directory different from its served path.
It tests Chromium and WebKit. Install matching Playwright browsers separately;
NixOS needs its Nix-provided browser bundle and matching Playwright version.
Set `FASTLED_TEST_REAL_CLUD=1` to additionally verify the host's
`~/.local/bin/clud` resolution and `clud --help` under a stripped parent PATH.
This invokes help only, not an agent task. Native macOS Safari remains a separate
platform check. This feature changes no Emscripten/linker defaults or JSPI flags.

xterm 5.5.0 and fit-addon 0.10.0 are downloaded directly from published tarballs
into `src/fastled/frontend/vendor/xterm/`. Upstream MIT licenses, source maps,
URLs and SHA-256 checksums are included. Relative imports and esbuild bundle
these assets; no npm, node_modules, CDN or runtime package resolution is used.

## Validation record (2026-09-12)

Work ran in `/home/niteris/dev/fastled-wasm-wt-terminal-240`, branched from
`origin/main` at `3ef4a57`. The original unpushed migration branch was preserved.
The Nix shell supplies the Linux native build dependencies; its local definition
at `/tmp/fastled-terminal-shell.nix` uses `mkShell`, `pkg-config` and an
`LD_LIBRARY_PATH` built from openssl, gtk3, webkitgtk_4_1, libsoup_3, bzip2, zlib,
xz, stdenv.cc.cc.lib, glib, gdk-pixbuf, pango, cairo, atk and harfbuzz.

Exact local gate commands:

```sh
nix-shell /tmp/fastled-terminal-shell.nix --run 'soldr cargo test --workspace terminal_240'
nix-shell /tmp/fastled-terminal-shell.nix --run 'PATH=/home/niteris/.soldr/cargo/bin:$PATH bash lint'
nix-shell /tmp/fastled-terminal-shell.nix --run 'bash test'
nix-shell /tmp/fastled-terminal-shell.nix --run 'FASTLED_TERMINAL_BINARY=/home/niteris/dev/fastled-wasm-wt-terminal-240/target/debug/fastled FASTLED_ESBUILD=/home/niteris/.fastled/toolchains/esbuild/linux/x64/0.28.0/esbuild FASTLED_TEST_REAL_CLUD=1 PLAYWRIGHT_BROWSERS_PATH=/nix/store/f0rap655j6wmqbfvqdw445kwcxkxwf7n-playwright-browsers uv run --with playwright==1.59.0 pytest tests/frontend/test_terminal.py -x -v -s'
```

Results: focused Rust tests 3 passed; full Rust suite 255 library tests, 2 binary
tests, 1 integration test and 1 doctest passed; Python 23 passed, 1 skipped (existing
Windows-only test); lint passed including clippy, dylint and Python checks.
Browser suite: 3 passed in 8.63 seconds, using Chromium and WebKit. Both passed
cwd (including a space in its name), ANSI/UTF-8 rendering, resizing, log isolation,
copy, hide/reopen state, fresh restart, actual `clud --help` with exit status 0,
and a byte-exact 120,400-byte Unicode paste. The third test verifies disconnect
cleanup when stdin blocks, including reuse after four disconnected sessions.

RED -> GREEN evidence: before the endpoint existed the foreign-origin test
failed with 404 instead of 403. Review reproduced four blocked writers leaking
all four slots (fifth connection got 429); separating writes from supervision
made that exact browser regression pass. A stripped-PATH browser check exposed
missing NixOS system paths, which are now included. Initial build attempts
failed on missing native library search paths; all gates passed after supplying
the Nix environment. No unrelated repository code was changed for those setup
failures. The pre-push review ended clean with one reviewer.

The full production app JS and CSS also bundled successfully with the existing
esbuild 0.28.0, using browser/ES2021/ESM flags and the local Three alias; CSS
bundling preserves external `./assets/*` URLs for the existing asset copier.
Native macOS Safari and Windows ConPTY were not run locally. No WASM compile was
needed, and no JSPI flags or WebAssembly JSPI APIs were introduced.
