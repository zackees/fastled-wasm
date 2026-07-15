import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main(): Promise<void> {
    const extensionDevelopmentPath = process.env.FASTLED_TEST_EXTENSION_PATH || path.resolve(__dirname, '../..');
    await runTests({
        version: '1.101.0',
        extensionDevelopmentPath,
        extensionTestsPath: path.resolve(__dirname, 'suite', 'index'),
        extensionTestsEnv: {
            FASTLED_EXPECT_BUNDLED_CLANGD: process.env.FASTLED_EXPECT_BUNDLED_CLANGD || 'universal',
            // The resolver must prove it is using the bundled absolute path.
            PATH: process.platform === 'win32' ? '' : ''
        },
        launchArgs: ['--disable-extensions']
    });
}

main().catch(error => { console.error(error); process.exitCode = 1; });
