const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { once } = require('node:events');
const fs = require('node:fs/promises');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');
const readline = require('node:readline');
const { test } = require('node:test');

test('MCP queries, waits for and interacts with the live session', {
    timeout: 6000, skip: process.platform === 'win32',
}, async t => {
    const project = await fs.mkdtemp(path.join(os.tmpdir(), 'xrm-'));
    t.after(() => fs.rm(project, { recursive: true, force: true }));
    const id = process.pid + '-face';
    const socketDir = path.join(project, 'xra-' + id);
    const socket = path.join(socketDir, 's');
    const querySocket = path.join(socketDir, 'q');
    const source = path.join(project, 'search.xr');
    const sourceVersions = { [source]: 42 };
    await fs.mkdir(socketDir, { mode: 0o700 });
    await fs.mkdir(path.join(project, '.xrack-live'));
    await fs.writeFile(path.join(project, '.xrack-live', id + '.json'), JSON.stringify({
        version: 1, pid: process.pid, component: 'Search', source, socket, querySocket,
    }));
    const inspection = net.createServer(client => client.end(JSON.stringify({
        version: 1, sourceVersions,
        recording: { version: 1, steps: [{ type: 'click', id: 'submit' }] },
        updatePlan: { pipelineApplied: true, entries: [{ element: 'submit', decisions: { updateEffect: 'paint' } }] },
        updates: [{ id: 1, layoutRuns: 0, layoutReuses: 1 }],
        properties: [{ name: 'query', kind: 'String', value: 'xr' }],
        tree: { nodes: [{ key: 'root', parent: null, source: { file: source, line: 1 } }] },
        trace: { generation: 3, captureValues: true, frames: [
            { id: 2, parentId: 1, name: 'search', parameters: [{ name: 'query', value: 'xr' }] },
            { id: 1, parentId: null, name: 'clicked' },
        ] },
    })));
    const queries = net.createServer(client => {
        client.once('data', chunk => {
            const query = JSON.parse(chunk);
            if (query.input) {
                assert.deepEqual(query, { generation: 3, input: { type: 'fill', id: 'query', text: 'cloud' } });
                client.end(JSON.stringify({ generation: 3, applied: true }));
                return;
            }
            assert.deepEqual(query, { nodeKey: 'root', property: 'results', offset: 12, limit: 1, generation: 3 });
            client.end(JSON.stringify({ sourceVersions, count: 13, items: ['result 12'] }));
        });
    });
    inspection.listen(socket);
    queries.listen(querySocket);
    t.after(() => { inspection.close(); queries.close(); });
    await Promise.all([once(inspection, 'listening'), once(queries, 'listening')]);

    const child = spawn(process.execPath, [path.join(__dirname, 'xrack-mcp.js'), '--project', project]);
    t.after(() => child.kill());
    const responses = readline.createInterface({ input: child.stdout })[Symbol.asyncIterator]();
    let sequence = 0;
    async function request(method, params) {
        const id = ++sequence;
        child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
        const response = JSON.parse((await responses.next()).value);
        assert.equal(response.id, id);
        assert.equal(response.error, undefined);
        return response.result;
    }
    const call = (name, args) => request('tools/call', { name, arguments: args });
    assert.equal((await request('initialize', { protocolVersion: '2025-11-25' })).protocolVersion, '2025-11-25');
    assert.equal((await request('tools/list')).tools.length, 12);
    assert.equal((await call('list_sessions')).structuredContent.sessions[0].id, id);
    const recording = await call('get_recording', { session: id });
    assert.deepEqual(recording.structuredContent.steps, [{ type: 'click', id: 'submit' }]);
    const profile = await call('get_update_profile', { session: id, element: 'submit' });
    assert.equal(profile.structuredContent.plan.entries[0].decisions.updateEffect, 'paint');
    assert.equal(profile.structuredContent.updates[0].layoutReuses, 1);
    const wait = await call('wait_for_property', { session: id, property: 'query', equals: 'xr' });
    assert.equal(wait.structuredContent.matched, true);
    const unsatisfied = await call('wait_for_property', { session: id, property: 'query', equals: false, timeoutMs: 1 });
    assert.equal(unsatisfied.structuredContent.matched, false);
    const node = await call('get_node', { session: id, nodeKey: 'root' });
    assert.equal(node.structuredContent.componentProperties[0].value, 'xr');
    assert.deepEqual(node.structuredContent.sourceVersions, sourceVersions);
    const stack = await call('get_call_stack', { session: id, frame: 2 });
    assert.deepEqual(stack.structuredContent.stack.map(frame => frame.name), ['clicked', 'search']);
    const filled = await call('fill', { session: id, id: 'query', text: 'cloud', generation: 3 });
    assert.equal(filled.structuredContent.applied, true);
    const page = await call('read_model', { session: id, nodeKey: 'root', property: 'results', offset: 12, limit: 1, generation: 3 });
    assert.deepEqual(page.structuredContent.items, ['result 12']);
    const invalid = await call('read_model', { session: id, nodeKey: 'root', property: 'results', limit: 25 });
    assert.equal(invalid.isError, true);
    assert.equal((await call('click', { session: id, nodeKey: 'root' })).isError, true);
    child.stdin.end();
    assert.equal((await once(child, 'exit'))[0], 0);
});
