import * as Mocha from 'mocha';
import * as path from 'path';

export function run(): Promise<void> {
    const mocha = new Mocha({ ui: 'tdd', color: true });
    mocha.addFile(path.resolve(__dirname, 'bundledClangd.test.js'));
    mocha.addFile(path.resolve(__dirname, 'intellisense.test.js'));
    return new Promise((resolve, reject) => mocha.run(failures => failures ? reject(new Error(`${failures} tests failed`)) : resolve()));
}
