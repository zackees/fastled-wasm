import * as assert from 'assert';
import { VersionedSnapshotScheduler } from '../../intellisense';

suite('FastLED live IntelliSense snapshots', () => {
    test('rapid edits only allow the latest generation to be current', async () => {
        const published: number[] = [];
        const scheduler = new VersionedSnapshotScheduler<number>(1, async generation => {
            published.push(generation);
            return generation;
        });
        scheduler.schedule();
        scheduler.schedule();
        scheduler.schedule();
        await new Promise(resolve => setTimeout(resolve, 20));
        assert.deepStrictEqual(published, [3]);
        assert.strictEqual(scheduler.isCurrent(3), true);
        assert.strictEqual(scheduler.isCurrent(2), false);
        scheduler.dispose();
    });
});
