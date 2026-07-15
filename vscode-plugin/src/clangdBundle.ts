import * as childProcess from 'child_process';
import * as crypto from 'crypto';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

export type BundledClangdResult =
    | { kind: 'ready'; path: string; target: string; version: string }
    | { kind: 'unavailable'; reason: 'universal-package' | 'unsupported-host' }
    | { kind: 'invalid'; reason: 'manifest-missing' | 'manifest-invalid' | 'target-mismatch' | 'binary-missing' | 'binary-hash-mismatch' | 'resource-dir-missing' | 'not-executable' | 'probe-timeout' | 'probe-failed' | 'version-mismatch'; message: string };

export interface FastLedExtensionApi {
    resolveBundledClangd(): Promise<BundledClangdResult>;
}

type Manifest = {
    schema_version: number;
    target: string;
    llvm_version: string;
    binary: { path: string; size: number; sha256: string };
    resource_include_path: string;
};

const HOST_TARGETS: Record<string, string> = {
    'win32-x64': 'win32-x64', 'win32-arm64': 'win32-arm64',
    'linux-x64': 'linux-x64', 'linux-arm64': 'linux-arm64',
    'darwin-x64': 'darwin-x64', 'darwin-arm64': 'darwin-arm64'
};

function invalid(reason: Extract<BundledClangdResult, { kind: 'invalid' }>['reason'], message: string): BundledClangdResult {
    return { kind: 'invalid', reason, message };
}

function relativePath(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) && !value.split(/[\\/]/).includes('..');
}

function inside(root: string, relative: string): string | undefined {
    if (!relativePath(relative)) { return undefined; }
    const candidate = path.resolve(root, relative);
    const prefix = root.endsWith(path.sep) ? root : root + path.sep;
    return candidate.startsWith(prefix) ? candidate : undefined;
}

function sha256(file: string): Promise<string> {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fs.createReadStream(file);
        stream.on('error', reject);
        stream.on('data', data => hash.update(data));
        stream.on('end', () => resolve(hash.digest('hex')));
    });
}

function probe(binary: string): Promise<{ stdout: string; stderr: string }> {
    return new Promise((resolve, reject) => {
        childProcess.execFile(binary, ['--version'], { timeout: 5000, windowsHide: true, maxBuffer: 64 * 1024, shell: false }, (error, stdout, stderr) => {
            if (error) { reject(error); return; }
            resolve({ stdout, stderr });
        });
    });
}

export async function resolveBundledClangd(context: vscode.ExtensionContext): Promise<BundledClangdResult> {
    const target = HOST_TARGETS[`${process.platform}-${process.arch}`];
    if (!target) { return { kind: 'unavailable', reason: 'unsupported-host' }; }
    const extensionRoot = context.extensionUri.fsPath;
    const packageFile = path.join(extensionRoot, 'package.json');
    let packageMetadata: { fastledBundledClangd?: { packageKind?: string } } = {};
    try { packageMetadata = JSON.parse(fs.readFileSync(packageFile, 'utf8')); } catch { return invalid('manifest-invalid', 'Cannot read extension package metadata'); }
    const root = path.join(extensionRoot, 'resources', 'clangd');
    const manifestPath = path.join(root, 'manifest.json');
    if (!fs.existsSync(manifestPath)) {
        return packageMetadata.fastledBundledClangd?.packageKind === 'universal'
            ? { kind: 'unavailable', reason: 'universal-package' }
            : invalid('manifest-missing', 'Bundled clangd manifest is missing');
    }
    let manifest: Manifest;
    try { manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')); } catch { return invalid('manifest-invalid', 'Bundled clangd manifest is invalid JSON'); }
    if (manifest.schema_version !== 1 || !relativePath(manifest.binary?.path) || !relativePath(manifest.resource_include_path) || typeof manifest.llvm_version !== 'string' || !/^[a-f0-9]{64}$/.test(manifest.binary?.sha256 || '') || !Number.isSafeInteger(manifest.binary?.size)) {
        return invalid('manifest-invalid', 'Bundled clangd manifest has an invalid schema');
    }
    if (manifest.target !== target) { return invalid('target-mismatch', `Bundle target ${manifest.target} does not match ${target}`); }
    const binary = inside(root, manifest.binary.path);
    const include = inside(root, manifest.resource_include_path);
    if (!binary || !include) { return invalid('manifest-invalid', 'Manifest path escapes its bundle'); }
    if (!fs.existsSync(binary) || !fs.statSync(binary).isFile()) { return invalid('binary-missing', 'Bundled clangd binary is missing'); }
    const stat = fs.statSync(binary);
    if (stat.size !== manifest.binary.size || await sha256(binary) !== manifest.binary.sha256) { return invalid('binary-hash-mismatch', 'Bundled clangd binary hash does not match its manifest'); }
    if (!fs.existsSync(path.join(include, 'stddef.h')) || !fs.existsSync(path.join(include, 'stdint.h'))) { return invalid('resource-dir-missing', 'Bundled clangd builtin headers are missing'); }
    if (process.platform !== 'win32' && (fs.statSync(binary).mode & 0o111) === 0) {
        try { fs.chmodSync(binary, 0o755); } catch { /* return deterministic failure below */ }
        if ((fs.statSync(binary).mode & 0o111) === 0) { return invalid('not-executable', 'Bundled clangd is not executable'); }
    }
    try {
        const output = await probe(binary);
        if (!output.stdout.includes(manifest.llvm_version)) { return invalid('version-mismatch', `clangd did not report LLVM ${manifest.llvm_version}`); }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return /timed out|ETIMEDOUT/i.test(message) ? invalid('probe-timeout', message) : invalid('probe-failed', message);
    }
    return { kind: 'ready', path: binary, target, version: manifest.llvm_version };
}
