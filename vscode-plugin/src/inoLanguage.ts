import * as path from 'path';
import * as vscode from 'vscode';

/** The small document shape needed to bind an Arduino sketch to C++. */
export type SketchDocument = {
    uri: { scheme: string; fsPath: string };
    languageId: string;
};

export interface InoLanguageRuntime<T extends SketchDocument> {
    workspaceFolder(document: T): string | undefined;
    setCppLanguage(document: T): Promise<void>;
}

/**
 * clangd activates for C++ language identifiers, while this extension owns
 * `.ino` as Arduino.  Bind only the sketch's top-level Arduino tabs; library
 * and nested files retain the language chosen by their owner.
 */
export class InoLanguageBinder<T extends SketchDocument> {
    public constructor(private readonly runtime: InoLanguageRuntime<T>) {}

    public async bind(document: T): Promise<boolean> {
        if (!isTopLevelSketchIno(document, this.runtime.workspaceFolder(document)) || document.languageId === 'cpp') {
            return false;
        }
        await this.runtime.setCppLanguage(document);
        return true;
    }
}

export function isTopLevelSketchIno(document: SketchDocument, sketchDir: string | undefined): boolean {
    if (document.uri.scheme !== 'file' || path.extname(document.uri.fsPath).toLowerCase() !== '.ino' || !sketchDir) {
        return false;
    }
    return comparablePath(path.dirname(document.uri.fsPath)) === comparablePath(sketchDir);
}

/** Binds both documents already open during activation and later sketch tabs. */
export class InoLanguageController implements vscode.Disposable {
    private readonly subscriptions: vscode.Disposable[] = [];
    private readonly activeBindings = new Map<string, Promise<void>>();
    private readonly binder = new InoLanguageBinder<vscode.TextDocument>({
        workspaceFolder: document => vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath,
        setCppLanguage: async document => { await vscode.languages.setTextDocumentLanguage(document, 'cpp'); },
    });

    public constructor(private readonly output: vscode.OutputChannel) {}

    public start(context: vscode.ExtensionContext): Promise<void> {
        this.subscriptions.push(vscode.workspace.onDidOpenTextDocument(document => {
            void this.bindDocument(document);
        }));
        this.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders(() => {
            for (const document of vscode.workspace.textDocuments) {
                void this.bindDocument(document);
            }
        }));
        context.subscriptions.push(this);
        return Promise.all(vscode.workspace.textDocuments.map(document => this.bindDocument(document))).then(() => undefined);
    }

    public dispose(): void {
        for (const subscription of this.subscriptions) { subscription.dispose(); }
        this.subscriptions.length = 0;
    }

    private bindDocument(document: vscode.TextDocument): Promise<void> {
        const key = document.uri.toString();
        const active = this.activeBindings.get(key);
        if (active) { return active; }
        const binding = this.bindDocumentInner(document).finally(() => this.activeBindings.delete(key));
        this.activeBindings.set(key, binding);
        return binding;
    }

    private async bindDocumentInner(document: vscode.TextDocument): Promise<void> {
        try {
            if (await this.binder.bind(document)) {
                this.output.appendLine(`IntelliSense bound ${document.uri.fsPath} to C++ for clangd navigation.`);
            }
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            this.output.appendLine(`IntelliSense could not bind ${document.uri.fsPath} to C++: ${message}`);
        }
    }
}

function comparablePath(value: string): string {
    const normalized = path.normalize(value).replace(/\\/g, '/').replace(/\/+$/, '');
    return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}
