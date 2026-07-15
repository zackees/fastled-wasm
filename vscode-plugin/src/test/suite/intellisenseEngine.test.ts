import * as assert from 'assert';
import {
    EngineController,
    EngineMode,
    EngineRuntime,
} from '../../intellisenseEngine';

type Bundle = Awaited<ReturnType<EngineRuntime['resolveBundledClangd']>>;

class FakeRuntime implements EngineRuntime {
    public readonly events: string[] = [];
    public mode: EngineMode = 'auto';
    public bundle: Bundle = { kind: 'ready', path: 'C:/FastLED/clangd.exe', target: 'win32-x64', version: '21.1.5' };
    public clangdExtension = true;
    public cpptoolsExtension = true;

    workspaceFolders(): readonly string[] { return ['C:/sketch-a', 'C:/sketch-b']; }
    isSketchDirectory(folder: string): boolean { return folder !== 'C:/sketch-b' || true; }
    async writeConfiguration(folder: string): Promise<void> { this.events.push(`write:${folder}`); }
    requestedEngine(): EngineMode { return this.mode; }
    async resolveBundledClangd(): Promise<Bundle> { return this.bundle; }
    isExtensionAvailable(id: string): boolean {
        return id === 'llvm-vs-code-extensions.vscode-clangd' ? this.clangdExtension : this.cpptoolsExtension;
    }
    async setClangdPath(value: string): Promise<void> { this.events.push(`clangd.path:${value}`); }
    async setClangdEnabled(value: boolean): Promise<void> { this.events.push(`clangd.enable:${value}`); }
    async setCpptoolsEnabled(folder: string, value: boolean | undefined): Promise<void> { this.events.push(`cpptools:${folder}:${value === undefined ? 'restore' : value ? 'default' : 'disabled'}`); }
    async restoreManagedSettings(folders: readonly string[]): Promise<void> { this.events.push(`restore:${folders.join(',')}`); }
    async runCommand(command: string): Promise<void> { this.events.push(`command:${command}`); }
    log(message: string): void { this.events.push(`log:${message}`); }
    warn(message: string): void { this.events.push(`warn:${message}`); }
}

suite('FastLED IntelliSense engine selection', () => {
    test('auto uses the bundled clangd and disables cpptools first', async () => {
        const runtime = new FakeRuntime();
        const result = await new EngineController(runtime).refresh('test');

        assert.strictEqual(result.engine, 'clangd');
        assert.deepStrictEqual(runtime.events.slice(0, 7), [
            'write:C:/sketch-a',
            'write:C:/sketch-b',
            'cpptools:C:/sketch-a:disabled',
            'cpptools:C:/sketch-b:disabled',
            'clangd.path:C:/FastLED/clangd.exe',
            'clangd.enable:true',
            'command:clangd.restart',
        ]);
    });

    test('auto falls back to cpptools when no native clangd is available', async () => {
        const runtime = new FakeRuntime();
        runtime.bundle = { kind: 'unavailable', reason: 'universal-package' };

        const result = await new EngineController(runtime).refresh('test');

        assert.strictEqual(result.engine, 'cpptools');
        assert.ok(runtime.events.includes('clangd.enable:false'));
        assert.ok(runtime.events.includes('command:clangd.shutdown'));
        assert.ok(runtime.events.includes('cpptools:C:/sketch-a:default'));
        assert.ok(runtime.events.includes('cpptools:C:/sketch-b:default'));
    });

    test('explicit clangd fails visibly rather than silently selecting a different engine', async () => {
        const runtime = new FakeRuntime();
        runtime.mode = 'clangd';
        runtime.clangdExtension = false;

        await assert.rejects(
            new EngineController(runtime).refresh('test'),
            /clangd extension is not installed/,
        );
    });

    test('off stops clangd and restores only FastLED-managed settings', async () => {
        const runtime = new FakeRuntime();
        runtime.mode = 'off';

        const result = await new EngineController(runtime).refresh('test');

        assert.strictEqual(result.engine, 'off');
        assert.ok(runtime.events.includes('clangd.enable:false'));
        assert.ok(runtime.events.includes('command:clangd.shutdown'));
        assert.ok(runtime.events.includes('restore:C:/sketch-a,C:/sketch-b'));
    });

    test('serializes overlapping refresh requests', async () => {
        const runtime = new FakeRuntime();
        const controller = new EngineController(runtime);
        await Promise.all([controller.refresh('first'), controller.refresh('second')]);

        const firstRestart = runtime.events.indexOf('command:clangd.restart');
        const secondWrite = runtime.events.lastIndexOf('write:C:/sketch-a');
        assert.ok(firstRestart >= 0);
        assert.ok(secondWrite > firstRestart, `refreshes interleaved: ${runtime.events.join(', ')}`);
    });
});
