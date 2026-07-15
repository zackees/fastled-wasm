import * as cp from 'child_process';
import * as path from 'path';
import * as vscode from 'vscode';

export type SnapshotDocument = { path: string; version: number; text: string };
export type SnapshotRequest = { sketch_dir: string; generation: number; documents: SnapshotDocument[] };

type SnapshotRunner = (request: SnapshotRequest) => Promise<void>;

/**
 * A tiny generation gate used by the controller and its Extension Development
 * Host tests. An obsolete CLI process may finish after a newer buffer; its
 * result must never become the current editor state.
 */
export class VersionedSnapshotScheduler<T> implements vscode.Disposable {
    private generation = 0;
    private timer: NodeJS.Timeout | undefined;

    public constructor(private readonly delayMs: number, private readonly publish: (generation: number) => Promise<T>) {}

    public schedule(): void {
        const generation = ++this.generation;
        if (this.timer) { clearTimeout(this.timer); }
        this.timer = setTimeout(() => {
            this.timer = undefined;
            void this.publish(generation).catch(() => undefined);
        }, this.delayMs);
    }

    public isCurrent(generation: number): boolean { return generation === this.generation; }

    public dispose(): void {
        if (this.timer) { clearTimeout(this.timer); this.timer = undefined; }
    }
}

export class IntelliSenseSnapshotController implements vscode.Disposable {
    private readonly scheduler: VersionedSnapshotScheduler<void>;
    private readonly subscriptions: vscode.Disposable[] = [];
    private configurationReady: Promise<void> | undefined;

    public constructor(
        private readonly output: vscode.OutputChannel,
        private readonly runSnapshot: SnapshotRunner = runSnapshotCommand,
    ) {
        this.scheduler = new VersionedSnapshotScheduler(350, generation => this.publish(generation));
    }

    public start(context: vscode.ExtensionContext): void {
        const schedule = () => this.scheduler.schedule();
        this.subscriptions.push(
            vscode.workspace.onDidOpenTextDocument(document => { if (isTopLevelIno(document)) { schedule(); } }),
            vscode.workspace.onDidChangeTextDocument(event => { if (isTopLevelIno(event.document)) { schedule(); } }),
            vscode.workspace.onDidSaveTextDocument(document => { if (isTopLevelIno(document)) { schedule(); } }),
            vscode.workspace.onDidCloseTextDocument(document => { if (isTopLevelIno(document)) { schedule(); } }),
            vscode.workspace.onDidCreateFiles(event => this.refreshForTopology(event.files)),
            vscode.workspace.onDidDeleteFiles(event => this.refreshForTopology(event.files)),
            vscode.workspace.onDidRenameFiles(event => this.refreshForTopology(event.files.flatMap(file => [file.oldUri, file.newUri]))),
        );
        context.subscriptions.push(this, ...this.subscriptions);
        void this.ensureConfiguration().then(schedule);
    }

    public dispose(): void {
        this.scheduler.dispose();
        for (const subscription of this.subscriptions) { subscription.dispose(); }
    }

    private refreshForTopology(uris: readonly vscode.Uri[]): void {
        if (!uris.some(uri => path.extname(uri.fsPath).toLowerCase() === '.ino')) { return; }
        this.configurationReady = undefined;
        void this.ensureConfiguration().then(() => this.scheduler.schedule());
    }

    private ensureConfiguration(): Promise<void> {
        if (!this.configurationReady) {
            const sketchDir = currentSketchDirectory();
            this.configurationReady = sketchDir
                ? runFastled(['--write-clangd', sketchDir]).catch(error => {
                    this.output.appendLine(`IntelliSense configuration was not refreshed: ${error.message}`);
                })
                : Promise.resolve();
        }
        return this.configurationReady;
    }

    private async publish(generation: number): Promise<void> {
        try {
            const sketchDir = currentSketchDirectory();
            if (!sketchDir) { return; }
            const request: SnapshotRequest = {
                sketch_dir: sketchDir,
                generation,
                documents: vscode.workspace.textDocuments
                    .filter(document => isTopLevelIno(document, sketchDir))
                    .map(document => ({ path: document.uri.fsPath, version: document.version, text: document.getText() })),
            };
            await this.runSnapshot(request);
            if (this.scheduler.isCurrent(generation)) {
                this.output.appendLine(`IntelliSense snapshot ${generation} refreshed (${request.documents.length} open .ino buffer(s)).`);
            }
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            this.output.appendLine(`IntelliSense snapshot failed: ${message}`);
            vscode.window.setStatusBarMessage('FastLED IntelliSense kept its last valid snapshot; fix the current .ino syntax for updated prototypes.', 8000);
        }
    }
}

function currentSketchDirectory(): string | undefined {
    const active = vscode.window.activeTextEditor?.document.uri;
    const activeFolder = active && vscode.workspace.getWorkspaceFolder(active);
    return activeFolder?.uri.fsPath ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

function isTopLevelIno(document: vscode.TextDocument, expectedSketchDir?: string): boolean {
    if (document.uri.scheme !== 'file' || path.extname(document.uri.fsPath).toLowerCase() !== '.ino') { return false; }
    const sketchDir = expectedSketchDir ?? currentSketchDirectory();
    return Boolean(sketchDir && path.dirname(document.uri.fsPath) === sketchDir);
}

function runSnapshotCommand(request: SnapshotRequest): Promise<void> {
    return runFastled(['--write-intellisense-snapshot'], JSON.stringify(request));
}

function runFastled(args: string[], input?: string): Promise<void> {
    return new Promise((resolve, reject) => {
        const child = cp.spawn('fastled', args, { stdio: ['pipe', 'pipe', 'pipe'] });
        let stderr = '';
        child.stderr?.on('data', data => { stderr += data.toString(); });
        child.once('error', reject);
        child.once('close', code => code === 0 ? resolve() : reject(new Error(stderr.trim() || `fastled exited with code ${code}`)));
        child.stdin?.end(input);
    });
}
