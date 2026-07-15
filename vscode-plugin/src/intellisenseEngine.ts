import * as fs from 'fs';
import * as vscode from 'vscode';

import { BundledClangdResult, resolveBundledClangd } from './clangdBundle';
import { runFastled } from './intellisense';

const CLANGD_EXTENSION = 'llvm-vs-code-extensions.vscode-clangd';
const CPPTOOLS_EXTENSION = 'ms-vscode.cpptools';

export type EngineMode = 'auto' | 'clangd' | 'cpptools' | 'off';
export type SelectedEngine = Exclude<EngineMode, 'auto'>;

export type EngineResult = {
    engine: SelectedEngine;
    configuredFolders: readonly string[];
};

/**
 * Thin runtime boundary so engine choice and ordering are testable without a
 * desktop VS Code process or a native clangd binary.
 */
export interface EngineRuntime {
    workspaceFolders(): readonly string[];
    isSketchDirectory(folder: string): boolean;
    writeConfiguration(folder: string): Promise<void>;
    requestedEngine(): EngineMode;
    resolveBundledClangd(): Promise<BundledClangdResult>;
    isExtensionAvailable(id: string): boolean;
    setClangdPath(value: string): Promise<void>;
    setClangdEnabled(value: boolean): Promise<void>;
    setCpptoolsEnabled(folder: string, value: boolean | undefined): Promise<void>;
    restoreManagedSettings(folders: readonly string[]): Promise<void>;
    runCommand(command: string): Promise<void>;
    log(message: string): void;
    warn(message: string): void;
}

/**
 * Chooses exactly one language engine for this VS Code window. Metadata is
 * still emitted once for every FastLED workspace folder because both engines
 * consume the same generated Arduino translation-unit model.
 */
export class EngineController {
    private tail: Promise<void> = Promise.resolve();

    public constructor(private readonly runtime: EngineRuntime) {}

    public refresh(reason: string): Promise<EngineResult> {
        let result: EngineResult | undefined;
        const run = this.tail.then(async () => { result = await this.refreshInner(reason); });
        // A failed explicit selection must be reported to the caller, but must
        // not poison later refreshes caused by a configuration change.
        this.tail = run.catch(() => undefined);
        return run.then(() => result!);
    }

    private async refreshInner(reason: string): Promise<EngineResult> {
        const folders = this.runtime.workspaceFolders().filter(folder => this.runtime.isSketchDirectory(folder));
        for (const folder of folders) {
            await this.runtime.writeConfiguration(folder);
        }

        const engine = await this.selectEngine();
        if (engine === 'clangd') {
            const bundle = await this.runtime.resolveBundledClangd();
            if (bundle.kind !== 'ready') {
                throw new Error(`bundled clangd is unavailable: ${bundle.reason}`);
            }
            // Stop the competing provider before starting clangd. cpptools
            // remains installed for its non-language-service features.
            for (const folder of folders) {
                await this.runtime.setCpptoolsEnabled(folder, false);
            }
            await this.runtime.setClangdPath(bundle.path);
            await this.runtime.setClangdEnabled(true);
            await this.runtime.runCommand('clangd.restart');
        } else if (engine === 'cpptools') {
            // clangd.enable alone does not stop an already-running server.
            if (this.runtime.isExtensionAvailable(CLANGD_EXTENSION)) {
                await this.runtime.setClangdEnabled(false);
                await this.runtime.runCommand('clangd.shutdown');
            }
            for (const folder of folders) {
                await this.runtime.setCpptoolsEnabled(folder, true);
            }
            if (!this.runtime.isExtensionAvailable(CPPTOOLS_EXTENSION)) {
                this.runtime.warn('Microsoft C/C++ is not installed; FastLED generated IntelliSense metadata but no fallback language service is available.');
            }
        } else {
            if (this.runtime.isExtensionAvailable(CLANGD_EXTENSION)) {
                await this.runtime.setClangdEnabled(false);
                await this.runtime.runCommand('clangd.shutdown');
            }
            await this.runtime.restoreManagedSettings(folders);
        }

        this.runtime.log(`IntelliSense engine ${engine} refreshed for ${folders.length} sketch folder(s): ${reason}.`);
        return { engine, configuredFolders: folders };
    }

    private async selectEngine(): Promise<SelectedEngine> {
        const requested = this.runtime.requestedEngine();
        if (requested === 'off' || requested === 'cpptools') { return requested; }

        const hasClangdExtension = this.runtime.isExtensionAvailable(CLANGD_EXTENSION);
        const bundle = await this.runtime.resolveBundledClangd();
        const bundledServerReady = bundle.kind === 'ready';

        if (requested === 'clangd') {
            if (!hasClangdExtension) { throw new Error('clangd extension is not installed; install llvm-vs-code-extensions.vscode-clangd or select Microsoft C/C++.'); }
            if (!bundledServerReady) { throw new Error(`bundled clangd is unavailable: ${bundle.reason}; select Microsoft C/C++ or install a native FastLED VSIX.`); }
            return 'clangd';
        }

        return hasClangdExtension && bundledServerReady ? 'clangd' : 'cpptools';
    }
}

/** VS Code implementation of the engine boundary. */
export class VscodeEngineRuntime implements EngineRuntime {
    public constructor(
        private readonly context: vscode.ExtensionContext,
        private readonly output: vscode.OutputChannel,
    ) {}

    workspaceFolders(): readonly string[] {
        return (vscode.workspace.workspaceFolders ?? []).map(folder => folder.uri.fsPath);
    }

    isSketchDirectory(folder: string): boolean {
        try { return fs.readdirSync(folder).some(file => file.toLowerCase().endsWith('.ino')); }
        catch { return false; }
    }

    writeConfiguration(folder: string): Promise<void> {
        return runFastled(['--write-clangd', folder]);
    }

    requestedEngine(): EngineMode {
        return vscode.workspace.getConfiguration('fastled').get<EngineMode>('intelliSenseEngine', 'auto');
    }

    resolveBundledClangd(): Promise<BundledClangdResult> {
        return resolveBundledClangd(this.context);
    }

    isExtensionAvailable(id: string): boolean {
        return vscode.extensions.getExtension(id) !== undefined;
    }

    async setClangdPath(value: string): Promise<void> {
        await this.rememberWorkspaceValue('clangd.path', vscode.workspace.getConfiguration('clangd').inspect<string>('path')?.workspaceValue);
        await vscode.workspace.getConfiguration('clangd').update('path', value, vscode.ConfigurationTarget.Workspace);
    }

    async setClangdEnabled(value: boolean): Promise<void> {
        await this.rememberWorkspaceValue('clangd.enable', vscode.workspace.getConfiguration('clangd').inspect<boolean>('enable')?.workspaceValue);
        await vscode.workspace.getConfiguration('clangd').update('enable', value, vscode.ConfigurationTarget.Workspace);
    }

    async setCpptoolsEnabled(folder: string, value: boolean | undefined): Promise<void> {
        const config = vscode.workspace.getConfiguration('C_Cpp', vscode.Uri.file(folder));
        if (value !== undefined) {
            await this.rememberWorkspaceValue(`C_Cpp.intelliSenseEngine:${folder}`, config.inspect<string>('intelliSenseEngine')?.workspaceFolderValue);
        }
        await config.update(
            'intelliSenseEngine',
            value === undefined ? undefined : value ? 'default' : 'disabled',
            vscode.ConfigurationTarget.WorkspaceFolder,
        );
    }

    async restoreManagedSettings(folders: readonly string[]): Promise<void> {
        await this.restoreWorkspaceValue('clangd.path', 'clangd', 'path');
        await this.restoreWorkspaceValue('clangd.enable', 'clangd', 'enable');
        for (const folder of folders) {
            const key = `C_Cpp.intelliSenseEngine:${folder}`;
            const saved = this.context.workspaceState.get<ManagedValue>(key);
            if (!saved?.managed) { continue; }
            await vscode.workspace.getConfiguration('C_Cpp', vscode.Uri.file(folder)).update(
                'intelliSenseEngine', saved.value,
                vscode.ConfigurationTarget.WorkspaceFolder,
            );
            await this.context.workspaceState.update(key, undefined);
        }
    }

    async runCommand(command: string): Promise<void> {
        await vscode.commands.executeCommand(command);
    }

    log(message: string): void { this.output.appendLine(message); }

    warn(message: string): void {
        this.output.appendLine(`IntelliSense warning: ${message}`);
        void vscode.window.showWarningMessage(message);
    }

    private async rememberWorkspaceValue(key: string, value: unknown): Promise<void> {
        if (this.context.workspaceState.get<ManagedValue>(key)?.managed) { return; }
        await this.context.workspaceState.update(key, { managed: true, value } satisfies ManagedValue);
    }

    private async restoreWorkspaceValue(key: string, section: string, setting: string): Promise<void> {
        const saved = this.context.workspaceState.get<ManagedValue>(key);
        if (!saved?.managed) { return; }
        await vscode.workspace.getConfiguration(section).update(setting, saved.value, vscode.ConfigurationTarget.Workspace);
        await this.context.workspaceState.update(key, undefined);
    }
}

type ManagedValue = { managed: true; value: unknown };

/** Register lifecycle refreshes without starting a second engine on activation. */
export function registerEngineLifecycle(
    context: vscode.ExtensionContext,
    controller: EngineController,
    output: vscode.OutputChannel,
): void {
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(event => {
        if (!event.affectsConfiguration('fastled.intelliSenseEngine')) { return; }
        void controller.refresh('engine setting changed').catch(error => {
            const message = error instanceof Error ? error.message : String(error);
            output.appendLine(`IntelliSense engine selection failed: ${message}`);
            void vscode.window.showErrorMessage(`FastLED IntelliSense: ${message}`);
        });
    }));
}
