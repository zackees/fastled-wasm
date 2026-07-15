import * as assert from 'assert';
import * as vscode from 'vscode';
import { FastLedExtensionApi } from '../../clangdBundle';

suite('bundled clangd', () => {
    test('exposes the documented API without finding a system clangd', async () => {
        const extension = vscode.extensions.getExtension<FastLedExtensionApi>('fastled.fastled-wasm');
        assert.ok(extension, 'FastLED extension is installed');
        const api = await extension.activate();
        const result = await api.resolveBundledClangd();
        if (process.env.FASTLED_EXPECT_BUNDLED_CLANGD === 'universal') {
            assert.deepStrictEqual(result, { kind: 'unavailable', reason: 'universal-package' });
        } else {
            assert.strictEqual(result.kind, 'ready');
            if (result.kind === 'ready') {
                assert.ok(result.path.startsWith(extension.extensionPath), 'resolved clangd is inside installed VSIX');
            }
        }
    });
});
