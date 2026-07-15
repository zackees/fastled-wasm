/* CI executes this in an Extension Development Host after installing a VSIX. */
import * as assert from 'assert';
import * as path from 'path';
import * as vscode from 'vscode';
import { FastLedExtensionApi } from '../clangdBundle';

export async function verifyInstalledBundledClangd(expected: 'native' | 'universal'): Promise<void> {
    const extension = vscode.extensions.getExtension<FastLedExtensionApi>('fastled.fastled-wasm');
    assert.ok(extension, 'FastLED extension is installed');
    const api = await extension.activate();
    const result = await api.resolveBundledClangd();
    if (expected === 'universal') {
        assert.deepStrictEqual(result, { kind: 'unavailable', reason: 'universal-package' });
    } else {
        assert.strictEqual(result.kind, 'ready');
        if (result.kind === 'ready') {
            const relative = path.relative(extension.extensionPath, result.path);
            assert.ok(relative && !relative.startsWith('..') && !path.isAbsolute(relative), 'clangd is inside the installed extension');
        }
    }
}
