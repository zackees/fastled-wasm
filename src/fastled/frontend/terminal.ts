/** Interactive PTY client. Sketch logging deliberately uses its own surface. */
import { Terminal } from './vendor/xterm/build/xterm.js';
import { FitAddon } from './vendor/xterm/build/addon-fit.js';

export function installTerminal() {
    const opener = document.getElementById('terminal-open') as HTMLButtonElement;
    const dialog = document.getElementById('terminal-dialog') as HTMLDialogElement;
    const container = document.getElementById('terminal-container') as HTMLDivElement;
    const status = document.getElementById('terminal-status') as HTMLSpanElement;
    const restart = document.getElementById('terminal-restart') as HTMLButtonElement;
    let terminal: Terminal | undefined;
    let fit: FitAddon | undefined;
    let socket: WebSocket | undefined;
    let inputQueue: object[] = [];
    let inputTimer: ReturnType<typeof setTimeout> | undefined;

    // A server-minted, single-use reattach token (#254). The server issues one
    // only for a configured agent session; a plain shell sends none and the
    // terminal behaves exactly as before. sessionStorage keeps it to this tab,
    // and a failed read must never stop the terminal from opening.
    const TOKEN_KEY = 'fastled.terminal.reattach';
    const readToken = (): string | null => {
        try {
            return sessionStorage.getItem(TOKEN_KEY);
        } catch {
            return null;
        }
    };
    const storeToken = (token: string | null) => {
        try {
            if (token === null) sessionStorage.removeItem(TOKEN_KEY);
            else sessionStorage.setItem(TOKEN_KEY, token);
        } catch {
            // A tab without storage simply loses reattach, not the terminal.
        }
    };

    const send = (message: object) => {
        if (socket?.readyState === WebSocket.OPEN) {
            if (socket.bufferedAmount > 256 * 1024) {
                status.textContent = 'Input queue full; reconnect to continue.';
                socket.close();
                return;
            }
            socket.send(JSON.stringify(message));
        }
    };
    const drainInput = () => {
        inputTimer = undefined;
        if (!socket || socket.readyState > WebSocket.OPEN) {
            inputQueue = [];
            return;
        }
        if (socket.readyState === WebSocket.OPEN && inputQueue.length) {
            send(inputQueue.shift()!);
        }
        if (inputQueue.length) inputTimer = setTimeout(drainInput, 4);
    };
    const enqueueInput = (messages: object[]) => {
        if (inputQueue.length + messages.length > 128) {
            status.textContent = 'Paste exceeds the input queue limit; reconnect to continue.';
            socket?.close();
            return;
        }
        inputQueue.push(...messages);
        if (inputTimer === undefined) drainInput();
    };
    const resize = () => {
        if (!dialog.open || !fit || !terminal) return;
        fit.fit();
        send({ type: 'resize', cols: terminal.cols, rows: terminal.rows });
    };
    const connect = (reattach: string | null = readToken()) => {
        socket?.close();
        clearTimeout(inputTimer);
        inputTimer = undefined;
        inputQueue = [];
        terminal?.dispose();
        terminal = new Terminal({ scrollback: 5000, fontSize: 14, cursorBlink: true });
        fit = new FitAddon();
        terminal.loadAddon(fit);
        terminal.open(container);
        resize();
        terminal.focus();
        const activeTerminal = terminal;
        const url = new URL('/terminal/ws', location.href);
        url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        if (reattach) url.searchParams.set('reattach', reattach);
        const connection = new WebSocket(url);
        socket = connection;
        connection.binaryType = 'arraybuffer';
        status.textContent = 'Connecting…';
        restart.disabled = true;
        connection.onopen = () => {
            if (socket !== connection) return;
            status.textContent = 'Connected — interactive shell';
            resize();
        };
        connection.onmessage = (event) => {
            if (socket !== connection) return;
            if (event.data instanceof ArrayBuffer) {
                activeTerminal.write(new Uint8Array(event.data), () => {
                    if (socket === connection) send({ type: 'ack' });
                });
            } else {
                const message = JSON.parse(event.data);
                if (typeof message.reattach === 'string') {
                    // Each attach mints a fresh token; the previous one is spent.
                    storeToken(message.reattach);
                    return;
                }
                if (typeof message.exit !== 'undefined') storeToken(null);
                status.textContent = message.error || `Shell exited (${message.exit}).`;
            }
        };
        connection.onerror = () => {
            if (socket === connection) status.textContent = 'Terminal connection failed. Use the FastLED server URL.';
        };
        connection.onclose = () => {
            if (socket !== connection) return;
            restart.disabled = false;
            if (status.textContent?.startsWith('Connected') || status.textContent === 'Connecting…') {
                status.textContent = 'Disconnected. Restart opens a new shell in the sketch directory.';
            }
        };
        activeTerminal.onData((data: string) => {
            // Keep JSON/UTF-8 frames below 64 KiB without splitting surrogates.
            // Pace large pastes so bounded backend queues can apply pressure.
            const points = Array.from(data);
            const messages: object[] = [];
            for (let i = 0; i < points.length; i += 4096) {
                messages.push({ type: 'input', data: points.slice(i, i + 4096).join('') });
            }
            enqueueInput(messages);
        });
        activeTerminal.onBinary((data: string) => {
            const bytes = Array.from(data, (char) => char.charCodeAt(0) & 255);
            const messages: object[] = [];
            for (let i = 0; i < bytes.length; i += 4096) {
                messages.push({ type: 'binary', data: bytes.slice(i, i + 4096) });
            }
            enqueueInput(messages);
        });
    };
    opener.addEventListener('click', () => {
        dialog.showModal();
        if (!terminal) connect();

        resize();
        terminal?.focus();
    });
    document.getElementById('terminal-close')!.addEventListener('click', () => dialog.close());
    dialog.addEventListener('close', () => opener.focus());
    // Escape belongs to shell applications (vim/clud), not dialog dismissal.
    dialog.addEventListener('cancel', (event) => event.preventDefault());
    // Restart is the deliberate "give me a new session" control, so it drops
    // the reattach token rather than rejoining the old session.
    restart.addEventListener('click', () => {
        storeToken(null);
        connect(null);
    });
    document.getElementById('terminal-copy')!.addEventListener('click', async () => {
        if (!terminal) return;
        const buffer = terminal.buffer.active;
        const lines: string[] = [];
        for (let i = 0; i < buffer.length; i++) lines.push(buffer.getLine(i)?.translateToString(true) || '');
        try {
            await navigator.clipboard.writeText(terminal.getSelection() || lines.join('\n'));
        } catch {
            status.textContent = 'Clipboard unavailable; select text and use your system copy shortcut.';
        }
    });
    new ResizeObserver(resize).observe(container);
    window.addEventListener('pagehide', () => socket?.close());
}
