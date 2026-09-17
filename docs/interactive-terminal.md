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
client queue. Backend queues hold at most 32 items each, and at most 16 chunks
await rendering acknowledgements. Protocol errors or full input queues disconnect
instead of dropping keystrokes. PTY reads and writes use dedicated threads.

Client input reaches the PTY through bounded writes: each attempt waits at most
50 ms for room in the terminal's input queue, and the worker checks for a
disconnected client between attempts. A foreground program that stops reading
stdin therefore holds its session only while its client is connected; on
disconnect the worker stops, the shell and its foreground job are killed and
reaped, and the terminal slot is released promptly; the regression tests
require all four slots back within 5 s while the blocked program still runs. This
matters because a plain blocking write on a full queue cannot be released by
anything on the server side — not by cancelling the thread, and not by killing
the process tree, which leaves the writer parked (#256). Unix cleanup also
hangs up the shell so it can notify its background jobs. Deliberately daemonized
or disowned programs are outside the terminal lifecycle, as in a normal shell.

### Input design decisions (#259)

- **No per-message input cap beyond the 64 KiB WebSocket limit.** A message
  larger than the free space in the terminal's input queue is written in the
  pieces the queue accepts, with the disconnect check between them, so a single
  message can wait on a program that is not reading but can never pin a slot
  past its client. A smaller cap would only split pastes that the client
  already paces.
- **Only the bounded write is non-blocking.** `kernal-api` switches the PTY
  input to non-blocking mode for the duration of `write_available` and restores
  it before returning (`O_NONBLOCK` on Unix, `PIPE_NOWAIT` on the Windows ConPTY
  input pipe). The reader keeps blocking reads on its own thread, which already
  ends when the session is dropped; a non-blocking reader would need its own
  wait loop and gain nothing.

## Verification

CI runs the browser suite on every pull request:

- `linux-x86-terminal-test.yml`: `tests/frontend/test_terminal.py` in Chromium
  and WebKit, all tests, none skipped.
- `windows-x86-terminal-test.yml`: the slot-release regression against ConPTY.
- `macos-arm-live-test.yml`: `ci/safari_terminal_smoke.py` in real Safari
  through safaridriver — the slot-release regression from Safari's own
  WebSocket, then a typed command rendered in xterm, with a screenshot artifact.

Under `CI`, the suite fails instead of skipping when its binary, esbuild,
Playwright or psutil is missing. No WASM compilation is involved.

Local run:

```sh
soldr cargo build --bin fastled
FASTLED_TERMINAL_BINARY="$PWD/target/debug/fastled" \
FASTLED_ESBUILD=/absolute/path/to/esbuild \
uv run --with playwright==1.62.0 --with psutil pytest tests/frontend/test_terminal.py -v
```

Install the browsers with
`uv run --with playwright==1.62.0 playwright install chromium webkit`. esbuild
0.28.0 is the version `fastled` installs under
`~/.fastled/toolchains/esbuild/`. Set `FASTLED_TEST_REAL_CLUD=1` to additionally
verify the host's `~/.local/bin/clud` resolution and `clud --help` under a
stripped parent PATH; this invokes help only, not an agent task.

Hosts that cannot install Playwright's WebKit (NixOS fails its host dependency
check) can run WebKit in the Playwright container and attach to it. Client and
server Playwright versions must match:

```sh
docker run -d --rm --name playwright-webkit --network host --init \
  mcr.microsoft.com/playwright:v1.62.0-noble \
  /bin/sh -c "cd /tmp && npx -y playwright@1.62.0 run-server --port 39123 --host 127.0.0.1"
FASTLED_PLAYWRIGHT_WEBKIT_ENDPOINT=ws://127.0.0.1:39123/ \
FASTLED_TERMINAL_BINARY=... FASTLED_ESBUILD=... \
uv run --with playwright==1.62.0 --with psutil pytest tests/frontend/test_terminal.py -v
docker stop playwright-webkit
```

Host networking keeps the page on the server's exact loopback origin, which the
terminal's Origin/Host check requires. On NixOS, Chromium may also need
`libgbm` on `LD_LIBRARY_PATH`.

The fixture bundles the real vendored terminal and CSS with esbuild, then starts
the real server and PTY from a temporary directory different from its served
path.

xterm 5.5.0 and fit-addon 0.10.0 are downloaded directly from published tarballs
into `src/fastled/frontend/vendor/xterm/`. Upstream MIT licenses, source maps,
URLs and SHA-256 checksums are included. Relative imports and esbuild bundle
these assets; no npm, node_modules, CDN or runtime package resolution is used.
This feature changes no Emscripten/linker defaults or JSPI flags.
