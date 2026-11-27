const assert = require('node:assert/strict');
const { EventEmitter, once } = require('node:events');
const Module = require('node:module');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { pathToFileURL } = require('node:url');
const { runInNewContext } = require('node:vm');

test('keeps the activated frame interactive when a newer edit fails', { timeout: 5000 }, async () => {
    const events = new EventEmitter();
    const messages = [];
    const disposable = () => ({ dispose() {} });
    const uri = fsPath => ({ fsPath, scheme: 'file', toString: () => pathToFileURL(fsPath).href });
    const fileName = path.join(os.tmpdir(), 'probe.xr');
    let text = 'valid';
    const document = { fileName, uri: uri(fileName), languageId: 'xrack', getText: () => text };
    const on = name => listener => {
        events.on(name, listener);
        return { dispose: () => events.off(name, listener) };
    };
    const panel = {
        webview: {
            asWebviewUri: value => value,
            onDidReceiveMessage: on('incoming'),
            postMessage(message) {
                messages.push(message);
                events.emit(message.status, message);
            },
        },
        onDidDispose: on('dispose'),
    };
    let openPreview;
    const vscode = {
        Uri: { joinPath: (value, ...parts) => uri(path.join(value.fsPath, ...parts)) },
        ViewColumn: { Beside: 2 },
        commands: { registerCommand: (_, command) => { openPreview = command; } },
        window: { activeTextEditor: { document }, createWebviewPanel: () => panel },
        workspace: {
            textDocuments: [document],
            getWorkspaceFolder: () => undefined,
            getConfiguration: () => ({ get: (_, fallback) => fallback }),
            createFileSystemWatcher: () => ({
                ...disposable(), onDidCreate: disposable, onDidChange: disposable, onDidDelete: disposable,
            }),
            onDidChangeTextDocument: on('edit'),
            onDidSaveTextDocument: disposable,
            onDidCloseTextDocument: disposable,
        },
    };
    class PreviewProcess {
        alive = true;

        request(message) {
            switch (message.type) {
                case 'load':
                    if (message.text === 'invalid') return { error: 'syntax error' };
                    return { image: 'image', size: [120, 80], component: 'Probe', components: ['Probe'], inputs: [] };
                case 'activate':
                    return new Promise(resolve => events.emit('activating', resolve));
                case 'click':
                    return { window_closed: true };
                default:
                    assert.fail(`Unexpected request: ${message.type}`);
            }
        }
        stop() {}
    }
    const load = Module._load;
    let registerPreview;
    try {
        Module._load = (name, ...args) => {
            if (name === 'vscode') return vscode;
            if (name === './preview-process') return { PreviewProcess };
            return load(name, ...args);
        };
        ({ registerPreview } = require('./preview'));
    } finally {
        Module._load = load;
    }
    registerPreview({
        subscriptions: [],
        asAbsolutePath: item => item.startsWith('server') ? process.execPath : path.join(os.tmpdir(), item),
        extensionUri: uri(os.tmpdir()),
        globalStorageUri: uri(os.tmpdir()),
    });
    await openPreview();
    try {
        const activating = once(events, 'activating');
        const script = [...panel.webview.html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)]
            .find(([, body]) => body.includes('acquireVsCodeApi()'))?.[1];
        assert.ok(script, 'missing the inline preview webview script');
        runInNewContext(script, {
            acquireVsCodeApi: () => ({ postMessage: message => events.emit('incoming', message) }),
            document: { getElementById: () => ({ addEventListener() {} }) },
            navigator: { platform: 'MacIntel' },
            ResizeObserver: class { observe() {} },
            window: { addEventListener() {}, matchMedia: () => ({ addEventListener() {} }) },
        }, { filename: 'xrack-preview-webview.js', timeout: 1000 });
        const [activate] = await activating;
        assert.equal(messages.some(message => message.image), false);

        const failed = once(events, 'Preview unavailable');
        text = 'invalid';
        events.emit('edit', { document, contentChanges: [{}] });
        activate({});
        assert.equal((await failed)[0].error, 'syntax error');
        const frame = messages.find(message => message.image);
        assert.equal(frame?.status, 'Live preview');
        await new Promise(resolve => setImmediate(resolve));
        const closed = once(events, 'Preview window closed');
        events.emit('incoming', { type: 'click', frame: frame.frame, x: 12, y: 16 });
        assert.equal((await closed)[0].windowClosed, true);
    } finally {
        events.emit('dispose');
    }
});
