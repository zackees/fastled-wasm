import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main(): Promise<void> {
    const extensionDevelopmentPath = process.env.FASTLED_TEST_EXTENSION_PATH || path.resolve(__dirname, '../..');
    const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'fastled-vscode-test-'));
    fs.writeFileSync(path.join(workspace, 'Blink.ino'), '#include <FastLED.h>\nCRGB leds[1];\nvoid setup() {}\nvoid loop() {}\n');
    try {
        await runTests({
            version: '1.101.0',
            extensionDevelopmentPath,
            extensionTestsPath: path.resolve(__dirname, 'suite', 'index'),
            extensionTestsEnv: {
                FASTLED_EXPECT_BUNDLED_CLANGD: process.env.FASTLED_EXPECT_BUNDLED_CLANGD || 'universal',
                FASTLED_TEST_WORKSPACE: workspace,
                // The resolver must prove it is using the bundled absolute path.
                PATH: process.platform === 'win32' ? '' : ''
            },
            launchArgs: [workspace, '--disable-extensions']
        });
    } finally {
        fs.rmSync(workspace, { recursive: true, force: true });
    }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
