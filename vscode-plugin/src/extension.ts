import * as vscode from 'vscode';
import * as cp from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import { FastLedExtensionApi, resolveBundledClangd } from './clangdBundle';
import { IntelliSenseSnapshotController } from './intellisense';
import { EngineController, registerEngineLifecycle, VscodeEngineRuntime } from './intellisenseEngine';
import { InoLanguageController } from './inoLanguage';

let serverProcess: cp.ChildProcess | undefined;
let outputChannel: vscode.OutputChannel;
let intelliSenseEngines: EngineController | undefined;

export function activate(context: vscode.ExtensionContext): FastLedExtensionApi {
    outputChannel = vscode.window.createOutputChannel('FastLED WASM');
    intelliSenseEngines = new EngineController(new VscodeEngineRuntime(context, outputChannel));
    registerEngineLifecycle(context, intelliSenseEngines, outputChannel);
    // The clangd client only registers navigation providers for C++ document
    // language IDs. Bind already-open Arduino tabs before configuring it.
    const inoLanguageReady = new InoLanguageController(outputChannel).start(context);
    
    // Register all commands
    const commands = [
        vscode.commands.registerCommand('fastled.compile', () => compile()),
        vscode.commands.registerCommand('fastled.compileQuick', () => compile(['--quick'])),
        vscode.commands.registerCommand('fastled.compileWeb', () => compile(['--web'])),
        vscode.commands.registerCommand('fastled.justCompile', () => compile(['--just-compile'])),
        vscode.commands.registerCommand('fastled.initProject', () => initProject()),
        vscode.commands.registerCommand('fastled.startServer', () => startServer()),
        vscode.commands.registerCommand('fastled.stopServer', () => stopServer()),
        vscode.commands.registerCommand('fastled.update', () => updateCompiler()),
        vscode.commands.registerCommand('fastled.purge', () => purgeContainers()),
        vscode.commands.registerCommand('fastled.openBrowser', () => openBrowser()),
        vscode.commands.registerCommand('fastled.refreshIntelliSense', () => refreshIntelliSense('manual refresh')),
        vscode.commands.registerCommand('fastled.showBundledClangdDiagnostics', async () => {
            const result = await resolveBundledClangd(context);
            outputChannel.show(true);
            outputChannel.appendLine(`Bundled clangd: ${JSON.stringify(result)}`);
            if (result.kind === 'ready') {
                vscode.window.showInformationMessage(`Bundled clangd ${result.version}: ${result.target}`);
            } else {
                vscode.window.showWarningMessage(`Bundled clangd unavailable: ${result.kind === 'invalid' ? result.reason : result.reason}`);
            }
        })
    ];

    commands.forEach(cmd => context.subscriptions.push(cmd));

    // Keep the generated Arduino prototype prelude in sync with live VS Code
    // buffers. This never saves or rewrites a user sketch.
    new IntelliSenseSnapshotController(
        outputChannel,
        undefined,
        async () => {
            await inoLanguageReady;
            await refreshIntelliSense('workspace activation');
        },
    ).start(context);

    // Register status bar item
    const statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.text = "$(zap) FastLED";
    statusBarItem.tooltip = "FastLED WASM Compiler";
    statusBarItem.command = 'fastled.compile';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    // Auto-detect FastLED projects and show welcome message
    if (vscode.workspace.workspaceFolders) {
        const workspaceFolder = vscode.workspace.workspaceFolders[0];
        if (isSketchDirectory(workspaceFolder.uri.fsPath)) {
            vscode.window.showInformationMessage(
                'FastLED sketch detected! Use the FastLED commands to compile and run.',
                'Compile & Run'
            ).then(selection => {
                if (selection === 'Compile & Run') {
                    compile();
                }
            });
        }
    }
    return { resolveBundledClangd: () => resolveBundledClangd(context) };
}

export function deactivate() {
    if (serverProcess) {
        serverProcess.kill();
    }
}

async function compile(args: string[] = []): Promise<void> {
    const workspaceFolder = getCurrentWorkspaceFolder();
    if (!workspaceFolder) {
        vscode.window.showErrorMessage('No workspace folder found. Please open a FastLED sketch directory.');
        return;
    }

    const config = vscode.workspace.getConfiguration('fastled');
    const useWebCompiler = config.get<boolean>('useWebCompiler', false);
    const defaultMode = config.get<string>('defaultCompileMode', 'quick');

    // Build command arguments
    const commandArgs = [workspaceFolder];
    
    if (useWebCompiler && !args.includes('--web')) {
        commandArgs.push('--web');
    }
    
    if (!args.some(arg => ['--quick', '--debug', '--release'].includes(arg))) {
        commandArgs.push(`--${defaultMode}`);
    }
    
    commandArgs.push(...args);

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine(`Compiling FastLED sketch: ${workspaceFolder}`);
    outputChannel.appendLine(`Command: fastled ${commandArgs.join(' ')}`);

    try {
        await runFastLEDCommand(commandArgs);
        await refreshIntelliSense('successful compile');
        vscode.window.showInformationMessage('FastLED compilation completed successfully!');
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`FastLED compilation failed: ${errorMessage}`);
        outputChannel.appendLine(`Error: ${errorMessage}`);
    }
}

async function refreshIntelliSense(reason: string): Promise<void> {
    if (!intelliSenseEngines) { return; }
    try {
        await intelliSenseEngines.refresh(reason);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        outputChannel.appendLine(`IntelliSense refresh failed: ${message}`);
        vscode.window.showErrorMessage(`FastLED IntelliSense: ${message}`);
        throw error;
    }
}

async function initProject(): Promise<void> {
    const examples = [
        'Blink',
        'FxWave2d',
        'Pride2015',
        'Noise',
        'Fire2012',
        'DemoReel100',
        'Pacifica',
        'Pride2015Palette',
        'Video'
    ];

    const selectedExample = await vscode.window.showQuickPick(examples, {
        placeHolder: 'Select a FastLED example to initialize'
    });

    if (!selectedExample) {
        return;
    }

    const workspaceFolder = getCurrentWorkspaceFolder();
    if (!workspaceFolder) {
        vscode.window.showErrorMessage('No workspace folder found. Please open a directory first.');
        return;
    }

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine(`Initializing FastLED project with example: ${selectedExample}`);

    try {
        await runFastLEDCommand(['--init', selectedExample]);
        vscode.window.showInformationMessage(`FastLED project initialized with ${selectedExample} example!`);
        
        // Refresh the workspace
        await vscode.commands.executeCommand('workbench.action.reloadWindow');
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`Failed to initialize project: ${errorMessage}`);
        outputChannel.appendLine(`Error: ${errorMessage}`);
    }
}

async function startServer(): Promise<void> {
    if (serverProcess) {
        vscode.window.showWarningMessage('FastLED server is already running.');
        return;
    }

    const workspaceFolder = getCurrentWorkspaceFolder();
    const args = ['--server'];
    
    if (workspaceFolder) {
        args.push(workspaceFolder);
    }

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine('Starting FastLED compiler server...');

    try {
        serverProcess = cp.spawn('fastled', args, {
            cwd: workspaceFolder || process.cwd()
        });

        serverProcess.stdout?.on('data', (data) => {
            outputChannel.append(data.toString());
        });

        serverProcess.stderr?.on('data', (data) => {
            outputChannel.append(data.toString());
        });

        serverProcess.on('close', (code) => {
            outputChannel.appendLine(`FastLED server process exited with code ${code}`);
            serverProcess = undefined;
        });

        vscode.window.showInformationMessage('FastLED compiler server started!');
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`Failed to start server: ${errorMessage}`);
        outputChannel.appendLine(`Error: ${errorMessage}`);
    }
}

async function stopServer(): Promise<void> {
    if (!serverProcess) {
        vscode.window.showWarningMessage('No FastLED server is currently running.');
        return;
    }

    serverProcess.kill();
    serverProcess = undefined;
    outputChannel.appendLine('FastLED server stopped.');
    vscode.window.showInformationMessage('FastLED server stopped.');
}

async function updateCompiler(): Promise<void> {
    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine('Updating FastLED compiler...');

    try {
        await runFastLEDCommand(['--update']);
        vscode.window.showInformationMessage('FastLED compiler updated successfully!');
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`Failed to update compiler: ${errorMessage}`);
        outputChannel.appendLine(`Error: ${errorMessage}`);
    }
}

async function purgeContainers(): Promise<void> {
    const confirmation = await vscode.window.showWarningMessage(
        'This will remove all FastLED Docker containers and images. Continue?',
        'Yes',
        'No'
    );

    if (confirmation !== 'Yes') {
        return;
    }

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine('Purging FastLED Docker containers...');

    try {
        await runFastLEDCommand(['--purge']);
        vscode.window.showInformationMessage('FastLED containers purged successfully!');
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`Failed to purge containers: ${errorMessage}`);
        outputChannel.appendLine(`Error: ${errorMessage}`);
    }
}

async function openBrowser(): Promise<void> {
    const workspaceFolder = getCurrentWorkspaceFolder();
    if (!workspaceFolder) {
        vscode.window.showErrorMessage('No workspace folder found.');
        return;
    }

    const outputPath = path.join(workspaceFolder, 'fastled_js', 'index.html');
    
    if (!fs.existsSync(outputPath)) {
        vscode.window.showErrorMessage('FastLED output not found. Please compile the sketch first.');
        return;
    }

    try {
        await vscode.env.openExternal(vscode.Uri.file(outputPath));
    } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(`Failed to open browser: ${errorMessage}`);
    }
}

function runFastLEDCommand(args: string[]): Promise<void> {
    return new Promise((resolve, reject) => {
        const childProcess = cp.spawn('fastled', args, {
            cwd: getCurrentWorkspaceFolder() || process.cwd()
        });

        childProcess.stdout?.on('data', (data: Buffer) => {
            outputChannel.append(data.toString());
        });

        childProcess.stderr?.on('data', (data: Buffer) => {
            outputChannel.append(data.toString());
        });

        childProcess.on('close', (code: number | null) => {
            if (code === 0) {
                resolve();
            } else {
                reject(new Error(`Process exited with code ${code}`));
            }
        });

        childProcess.on('error', (error: Error) => {
            reject(error);
        });
    });
}

function getCurrentWorkspaceFolder(): string | undefined {
    const activeEditor = vscode.window.activeTextEditor;
    
    if (activeEditor) {
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(activeEditor.document.uri);
        if (workspaceFolder) {
            return workspaceFolder.uri.fsPath;
        }
    }

    if (vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0) {
        return vscode.workspace.workspaceFolders[0].uri.fsPath;
    }

    return undefined;
}

function isSketchDirectory(dirPath: string): boolean {
    try {
        const files = fs.readdirSync(dirPath);
        return files.some(file => file.endsWith('.ino'));
    } catch {
        return false;
    }
}
