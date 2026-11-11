const { strict: assert } = require('node:assert');
const { test } = require('node:test');
const { PreviewQueue } = require('./preview-queue');

test('coalesces edits and suppresses stale frames while a render is running', async t => {
    t.mock.timers.enable({ apis: ['setTimeout'] });
    const renders = [];
    const queue = new PreviewQueue(isCurrent => new Promise(resolve => {
        renders.push({ isCurrent, resolve });
    }), 400);

    queue.refresh();
    t.mock.timers.tick(300);
    queue.refresh();
    t.mock.timers.tick(399);
    assert.equal(renders.length, 0);
    t.mock.timers.tick(1);
    assert.equal(renders.length, 1);
    assert.equal(renders[0].isCurrent(), true);

    queue.refresh();
    queue.refresh();
    t.mock.timers.tick(400);
    assert.equal(renders.length, 1);
    assert.equal(renders[0].isCurrent(), false);
    const first = queue.running;
    renders[0].resolve();
    await first;
    assert.equal(renders.length, 2);
    assert.equal(renders[1].isCurrent(), true);

    queue.refresh();
    const closed = queue.dispose();
    renders[1].resolve();
    await closed;
    t.mock.timers.tick(400);
    assert.equal(renders.length, 2);
    assert.equal(renders[1].isCurrent(), false);
});
