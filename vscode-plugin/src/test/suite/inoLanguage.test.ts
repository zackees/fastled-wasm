import * as assert from 'assert';
import * as path from 'path';
import * as vscode from 'vscode';
import { InoLanguageBinder, SketchDocument } from '../../inoLanguage';

type FakeDocument = SketchDocument & { languageId: string };

suite('FastLED .ino language binding', () => {
    test('rebinds an already-open top-level sketch from Arduino to C++', async () => {
        const changed: string[] = [];
        const binder = new InoLanguageBinder<FakeDocument>({
            workspaceFolder: () => 'C:/sketch',
            setCppLanguage: async document => {
                changed.push(document.uri.fsPath);
                document.languageId = 'cpp';
            },
        });
        const document: FakeDocument = {
            uri: { scheme: 'file', fsPath: 'C:/sketch/Blink.ino' },
            languageId: 'arduino',
        };

        assert.strictEqual(await binder.bind(document), true);
        assert.deepStrictEqual(changed, ['C:/sketch/Blink.ino']);
        assert.strictEqual(document.languageId, 'cpp');
    });

    test('leaves nested, non-sketch, and already-C++ documents alone', async () => {
        const changed: string[] = [];
        const binder = new InoLanguageBinder<FakeDocument>({
            workspaceFolder: () => 'C:/sketch',
            setCppLanguage: async document => { changed.push(document.uri.fsPath); },
        });
        const documents: FakeDocument[] = [
            { uri: { scheme: 'file', fsPath: 'C:/sketch/tabs/Other.ino' }, languageId: 'arduino' },
            { uri: { scheme: 'file', fsPath: 'C:/sketch/FastLED.h' }, languageId: 'cpp' },
            { uri: { scheme: 'file', fsPath: 'C:/sketch/Blink.ino' }, languageId: 'cpp' },
            { uri: { scheme: 'untitled', fsPath: 'C:/sketch/Blink.ino' }, languageId: 'arduino' },
        ];

        for (const document of documents) {
            assert.strictEqual(await binder.bind(document), false);
        }
        assert.deepStrictEqual(changed, []);
    });

    test('routes a bound top-level sketch to C++ definition providers', async () => {
        const workspace = process.env.FASTLED_TEST_WORKSPACE;
        assert.ok(workspace, 'test workspace was supplied by the Extension Development Host');
        const extension = vscode.extensions.getExtension('fastled.fastled-wasm');
        assert.ok(extension, 'FastLED extension is installed');
        await extension.activate();
        const document = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(workspace, 'Blink.ino')));
        let bound: vscode.TextDocument | undefined;
        for (let attempt = 0; attempt < 50; attempt += 1) {
            const bound = vscode.workspace.textDocuments.find(candidate => candidate.uri.toString() === document.uri.toString());
            if (bound?.languageId === 'cpp') { break; }
            await new Promise(resolve => setTimeout(resolve, 20));
        }
        bound = vscode.workspace.textDocuments.find(candidate => candidate.uri.toString() === document.uri.toString());
        assert.strictEqual(bound?.languageId, 'cpp', 'FastLED did not bind the opened Blink.ino document to cpp');

        const expected = new vscode.Location(vscode.Uri.file(path.join(workspace, 'FastLED.h')), new vscode.Position(0, 0));
        const provider = vscode.languages.registerDefinitionProvider({ language: 'cpp', scheme: 'file' }, {
            provideDefinition(candidate) {
                assert.strictEqual(candidate.uri.toString(), document.uri.toString());
                assert.strictEqual(candidate.languageId, 'cpp');
                return expected;
            },
        });
        try {
            const definitions = await vscode.commands.executeCommand<vscode.Location[]>(
                'vscode.executeDefinitionProvider', document.uri, new vscode.Position(1, 1),
            );
            assert.ok(definitions?.some(location => location.uri.toString() === expected.uri.toString()));
        } finally {
            provider.dispose();
        }
    });
});
