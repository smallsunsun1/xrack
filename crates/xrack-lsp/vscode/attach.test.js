const assert = require('node:assert/strict');
const { EventEmitter, once } = require('node:events');
const fs = require('node:fs/promises');
const Module = require('node:module');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');

test('separates read-only attachment from opt-in editor sync', { timeout: 5000, skip: process.platform === 'win32' }, async () => {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'xra-test-'));
    const socket = path.join(directory, 's');
    const editorSocket = path.join(directory, 'e');
    const file = path.join(directory, 'counter.xr');
    const changes = new EventEmitter();
    let editorConnections = 0;
    const sourceServer = net.createServer(client => {
        editorConnections++;
        client.setEncoding('utf8');
        client.write('{"version":1,"inspection":true}\n');
        let pending = '';
        client.on('data', chunk => {
            pending += chunk;
            const lines = pending.split('\n');
            pending = lines.pop();
            for (const line of lines) {
                const payload = JSON.parse(line);
                changes.emit(payload.inspection ? 'inspection' : 'update', payload);
            }
        });
        client.on('close', () => changes.emit('disconnected'));
    });
    let loading = true;
    let received = 0;
    const server = net.createServer(client => {
        client.on('data', chunk => { received += chunk.length; });
        client.end(JSON.stringify({
            version: 1,
            properties: [
                { name: 'query', kind: 'String', value: 'XRack' },
                { name: 'loading', kind: 'Bool', value: loading },
                { name: 'results', kind: 'StringList', count: loading ? 0 : 1, items: loading ? [] : ['XRack · 结果 1'] },
            ],
            tree: {
                nodes: [
                    { key: 'root', parent: null, kind: 'Window', properties: [] },
                    {
                        key: 'root/#query-field', parent: 0, kind: 'SearchField', name: 'query-field',
                        source: { file, line: 1, column: 3 },
                        properties: [{ name: 'text', kind: 'String', value: 'XRack' }],
                    },
                    {
                        key: 'root/#results-view', parent: 0, kind: 'ListView', name: 'results-view',
                        properties: [{ name: 'row-count', kind: 'Int', value: loading ? 0 : 1 }],
                    },
                ],
            },
        }) + '\n');
    });
    const subscriptions = [];
    try {
        const listening = once(server, 'listening');
        server.listen(socket);
        await listening;
        const editorListening = once(sourceServer, 'listening');
        sourceServer.listen(editorSocket);
        await editorListening;
        await fs.writeFile(file, '// saved');
        await fs.mkdir(path.join(directory, '.xrack-live'));
        await fs.writeFile(path.join(directory, '.xrack-live', 'fixture.json'), JSON.stringify({
            version: 1, socket, editorSocket, component: 'AsyncDemo', pid: process.pid,
        }));
        const commands = {};
        const changed = new EventEmitter();
        const disposable = { dispose() {} };
        let snapshot;
        let disposed = false;
        const folder = { uri: { scheme: 'file', fsPath: directory }, name: 'Test project' };
        const document = {
            languageId: 'xrack',
            uri: { scheme: 'file', fsPath: file },
            isDirty: true,
            getText: () => '// unsaved',
            lineCount: 1,
            lineAt: () => ({ text: '😀aquery-field := SearchField {}' }),
        };
        const listen = event => handler => {
            changes.on(event, handler);
            return { dispose: () => changes.off(event, handler) };
        };
        const vscode = {
            ViewColumn: { One: 1, Beside: 2 },
            Uri: { file: fsPath => ({ fsPath }), joinPath: (uri, ...parts) => path.join(uri, ...parts) },
            Position: class { constructor(line, character) { Object.assign(this, { line, character }); } },
            Range: class { constructor(start, end) { Object.assign(this, { start, end }); } },
            window: {
                visibleTextEditors: [],
                showInformationMessage: message => changed.emit('notice', message),
                showTextDocument: (document, options) => changed.emit('opened', { document, options }),
                createWebviewPanel() {
                    return {
                        webview: {
                            asWebviewUri: uri => uri,
                            onDidReceiveMessage: listen('message'),
                            postMessage(value) { snapshot = value; changed.emit('properties'); },
                            set html(value) { queueMicrotask(() => changes.emit('message', { type: 'ready' })); },
                        },
                        onDidDispose: () => disposable,
                        dispose() { disposed = true; },
                    };
                },
            },
            workspace: {
                workspaceFolders: [folder],
                textDocuments: [document],
                openTextDocument: () => document,
                getWorkspaceFolder: () => folder,
                onDidChangeTextDocument: listen('edit'),
                onDidSaveTextDocument: listen('save'),
                onDidCloseTextDocument: listen('close'),
            },
            commands: { registerCommand(name, callback) { commands[name] = callback; return disposable; } },
        };
        const load = Module._load;
        let attach;
        try {
            Module._load = (name, ...args) => name === 'vscode' ? vscode : load(name, ...args);
            attach = require('./attach');
        } finally {
            Module._load = load;
        }
        attach.registerAttach({ subscriptions, extensionUri: directory });
        function updatesTo(predicate) {
            return new Promise(resolve => {
                const onChange = () => {
                    if (!predicate(snapshot)) return;
                    changed.off('properties', onChange);
                    resolve();
                };
                changed.on('properties', onChange);
            });
        }
        const loadingValue = state => state.properties.find(property => property.name === 'loading')?.value;
        const initial = updatesTo(state => loadingValue(state) === true);
        await commands['xrack.attachRunningApp']();
        await initial;
        assert.equal(snapshot.properties[0].value, 'XRack');
        assert.equal(snapshot.tree.nodes[1].properties[0].value, 'XRack');
        assert.equal(snapshot.tree.nodes[2].properties[0].value, 0);

        const next = updatesTo(state => loadingValue(state) === false);
        loading = false;
        await next;
        assert.deepEqual(snapshot.properties[2].items, ['XRack · 结果 1']);
        assert.equal(snapshot.tree.nodes[2].properties[0].value, 1);
        assert.equal(received, 0);
        assert.equal(editorConnections, 0);

        const linked = once(changes, 'inspection');
        const draft = once(changes, 'update');
        await commands['xrack.syncRunningAppEditor']();
        assert.deepEqual((await linked)[0], { version: 1, inspection: { type: 'attach' } });
        assert.deepEqual((await draft)[0], { version: 1, buffers: { [await fs.realpath(file)]: '// unsaved' } });
        assert.equal(await fs.readFile(file, 'utf8'), '// saved');
        const highlighted = once(changes, 'inspection');
        changes.emit('message', { type: 'select', key: 'root/#query-field' });
        assert.deepEqual((await highlighted)[0], { version: 1, inspection: { type: 'select', key: 'root/#query-field' } });
        const picking = once(changes, 'inspection');
        changes.emit('message', { type: 'pick', active: true });
        assert.deepEqual((await picking)[0], { version: 1, inspection: { type: 'pick', active: true } });

        const stale = once(changed, 'notice');
        changes.emit('message', { type: 'source', key: 'root/#query-field' });
        assert.match((await stale)[0], /earlier source version/);

        const cleared = once(changes, 'update');
        document.isDirty = false;
        changes.emit('save', document);
        assert.deepEqual((await cleared)[0], { version: 1, buffers: {} });

        const opened = once(changed, 'opened');
        changes.emit('message', { type: 'source', key: 'root/#query-field' });
        const jump = (await opened)[0];
        assert.equal(jump.document.uri.fsPath, file);
        assert.equal(jump.options.selection.start.line, 0);
        assert.equal(jump.options.selection.start.character, 3);

        const stopped = once(changes, 'disconnected');
        await commands['xrack.detachRunningApp']();
        await stopped;
        assert.equal(disposed, true);
    } finally {
        for (const subscription of subscriptions.reverse()) subscription.dispose();
        await new Promise(resolve => server.close(resolve));
        await new Promise(resolve => sourceServer.close(resolve));
        await fs.rm(directory, { recursive: true, force: true });
    }
});
