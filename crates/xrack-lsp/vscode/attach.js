const fs = require('node:fs/promises');
const { randomBytes } = require('node:crypto');
const net = require('node:net');
const path = require('node:path');
const vscode = require('vscode');

const { readReply: readSnapshot } = require('./live-client');

async function runningApps() {
    const results = [];
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
        if (folder.uri.scheme !== 'file') continue;
        const directory = path.join(folder.uri.fsPath, '.xrack-live');
        let entries;
        try {
            entries = await fs.readdir(directory, { withFileTypes: true });
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
            continue;
        }
        for (const entry of entries) {
            if (!entry.isFile() || !entry.name.endsWith('.json')) continue;
            try {
                const app = JSON.parse(await fs.readFile(path.join(directory, entry.name), 'utf8'));
                if (app.version !== 1 || typeof app.socket !== 'string' || !path.isAbsolute(app.socket)
                    || typeof app.component !== 'string' || !Number.isSafeInteger(app.pid) || app.pid < 1) continue;
                process.kill(app.pid, 0);
                await fs.access(app.socket);
                results.push({
                    label: app.component,
                    description: 'PID ' + app.pid + ' · ' + folder.name,
                    socket: app.socket,
                    editorSocket: typeof app.editorSocket === 'string' && path.isAbsolute(app.editorSocket)
                        ? app.editorSocket : undefined,
                    folder: folder.uri.fsPath,
                });
            } catch {
                // The application may have exited during discovery.
            }
        }
    }
    return results;
}

function page(webview, media) {
    const nonce = randomBytes(16).toString('hex');
    const script = webview.asWebviewUri(vscode.Uri.joinPath(media, 'attached-app.js'));
    const stylesheet = webview.asWebviewUri(vscode.Uri.joinPath(media, 'attached-app.css'));
    return `<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}'; style-src ${webview.cspSource};">
<link rel="stylesheet" href="${stylesheet}">
</head><body>
<header><h1 id="app"></h1><span id="status"></span><button id="copy-recording" hidden>Copy Recording</button></header>
<p id="error" role="status">Waiting for the application…</p>
<main>
<section id="tree-section" hidden aria-labelledby="tree-title">
  <div id="tree-header"><h2 id="tree-title">Component tree</h2>
    <button id="pick" hidden aria-pressed="false">Pick in Window</button></div>
  <ul id="tree" class="tree"></ul><p id="truncated" hidden>Showing the first part of the tree.</p>
</section>
<section aria-labelledby="property-title">
  <h2 id="property-title">Properties</h2>
  <div id="sources"><button id="source" hidden>Go to Definition</button>
    <button id="component-source" hidden>Open Component Type</button></div>
  <div id="properties"></div>
</section>
<section id="calls-section" hidden aria-labelledby="calls-title">
  <h2 id="calls-title">Recent calls</h2>
  <p id="calls-status"></p>
  <label hidden id="calls-picker">Call <select id="calls"></select></label>
  <div id="call-stack"></div>
</section>
<section id="update-section" hidden aria-labelledby="update-title">
  <h2 id="update-title">UI updates</h2>
  <div id="update-details"></div>
</section>
<section id="diagnostics-section" hidden aria-labelledby="diagnostics-title">
  <h2 id="diagnostics-title">Request traces</h2>
  <label>Request <select id="diagnostic-request"></select></label>
  <ol id="diagnostic-events"></ol>
</section>
</main>
<script nonce="${nonce}" src="${script}"></script>
</body></html>`;
}

async function openSource(connection, message) {
    const snapshot = connection.snapshot;
    const node = snapshot?.tree?.nodes.find(node => node.key === message.key);
    let source;
    if (Number.isSafeInteger(message.planIndex)) {
        if (message.generation !== snapshot?.trace?.generation) return;
        source = snapshot?.updatePlan?.entries[message.planIndex]?.source;
    } else if (Number.isSafeInteger(message.frame)) {
        const frame = snapshot?.trace?.frames.find(frame => frame.id === message.frame);
        if (!frame || frame.generation !== snapshot.trace.generation) return;
        source = message.callSite ? frame.callSite : frame.source;
    } else if (typeof message.property === 'string') {
        const properties = node?.parent == null && message.key === 'root'
            ? snapshot?.properties : node?.componentProperties;
        source = properties?.find(property => property.name === message.property)?.source;
    } else {
        source = message.component ? node?.componentSource : node?.source;
    }
    if (!source || typeof source.file !== 'string' || !path.isAbsolute(source.file)
        || path.extname(source.file) !== '.xr' || !Number.isSafeInteger(source.column)
        || !Number.isSafeInteger(source.line) || source.line < 1 || source.column < 1) return;
    const document = await vscode.workspace.openTextDocument(vscode.Uri.file(source.file));
    const version = snapshot.sourceVersions?.[source.file];
    let actual = 0x811c9dc5;
    for (const byte of Buffer.from(document.getText(), 'utf8')) actual = Math.imul(actual ^ byte, 0x01000193);
    if ((version !== undefined && (actual >>> 0) !== version)
        || (version === undefined && document.isDirty) || source.line > document.lineCount) {
        void vscode.window.showInformationMessage('The running app is using an earlier source version. Apply a valid layout before opening this definition.');
        return;
    }
    const column = Array.from(document.lineAt(source.line - 1).text)
        .slice(0, source.column - 1).join('').length;
    const position = new vscode.Position(source.line - 1, column);
    const selection = new vscode.Range(position, position);
    const editor = vscode.window.visibleTextEditors.find(editor => editor.document === document);
    await vscode.window.showTextDocument(document, { viewColumn: editor?.viewColumn ?? vscode.ViewColumn.One, selection });
}

function registerAttach(context) {
    let active;
    let timer;
    let selection = 0;

    function detach() {
        ++selection;
        const connection = active;
        active = undefined;
        clearTimeout(timer);
        if (connection) {
            stopEditor(connection);
            connection.panel.dispose();
        }
    }

    async function refresh(connection) {
        try {
            const snapshot = await readSnapshot(connection.socket);
            if (connection !== active) return;
            connection.snapshot = snapshot;
            connection.error = undefined;
            show(connection);
        } catch (error) {
            if (connection !== active) return;
            connection.error = error.message;
            show(connection);
            if (error.code === 'ENOENT' || error.code === 'ECONNREFUSED') {
                connection.closed = true;
                stopEditor(connection);
            }
        } finally {
            if (connection === active && !connection.closed) timer = setTimeout(() => refresh(connection), 100);
        }
    }

    function show(connection) {
        if (!connection.ready) return;
        const snapshot = connection.snapshot;
        const display = {
            app: connection.label,
            status: connection.description + (connection.editorReady ? ' · Editor sync' : ''),
            tree: snapshot?.tree,
            inspection: connection.inspecting ? snapshot?.inspection : undefined,
            properties: snapshot?.properties ?? [],
            trace: snapshot?.trace,
            updatePlan: snapshot?.updatePlan,
            updates: snapshot?.updates,
            diagnostics: snapshot?.diagnostics,
            recording: snapshot?.recording && {
                count: snapshot.recording.steps.length, truncated: snapshot.recording.truncated,
            },
            error: connection.error ?? connection.syncError ?? snapshot?.reloadDiagnostic ?? snapshot?.treeError,
        };
        const encoded = JSON.stringify(display);
        if (encoded !== connection.display) {
            connection.display = encoded;
            void connection.panel.webview.postMessage({ type: 'snapshot', ...display });
        }
    }

    function stopEditor(connection) {
        clearTimeout(connection.editorTimer);
        connection.editor?.destroy();
    }

    function inspectWindow(connection, inspection) {
        if (connection.inspecting && !connection.editor.destroyed) {
            connection.editor.write(JSON.stringify({ version: 1, inspection }) + '\n');
        }
    }

    function scheduleEditor(connection) {
        const revision = ++connection.revision;
        clearTimeout(connection.editorTimer);
        connection.editorTimer = setTimeout(async () => {
            const documents = vscode.workspace.textDocuments.filter(document => document.isDirty
                && document.languageId === 'xrack'
                && document.uri.scheme === 'file'
                && vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath === connection.folder);
            const buffers = Object.fromEntries(await Promise.all(documents.map(async document => {
                const text = document.getText();
                const file = await fs.realpath(document.uri.fsPath).catch(() => document.uri.fsPath);
                return [file, text];
            })));
            if (connection !== active || connection.editor.destroyed || revision !== connection.revision) return;
            const message = JSON.stringify({ version: 1, buffers }) + '\n';
            if (Buffer.byteLength(message) > 4 * 1024 * 1024) {
                connection.syncError = 'XRack editor update exceeds 4 MiB';
                return;
            }
            connection.syncError = undefined;
            connection.editor.write(message);
        }, 60);
    }

    function startEditor(connection) {
        const editor = connection.editor = net.createConnection(connection.editorSocket);
        connection.revision = 0;
        let pending = '';
        editor.setEncoding('utf8');
        editor.on('data', text => {
            pending += text;
            if (pending.length > 8192) {
                connection.syncError = 'Invalid XRack editor response';
                editor.destroy();
                return;
            }
            if (!pending.includes('\n')) return;
            const lines = pending.split('\n');
            pending = lines.pop();
            for (const line of lines) {
                try {
                    const reply = JSON.parse(line);
                    if (reply.error) connection.syncError = reply.error;
                    else if (reply.version === 1) {
                        connection.editorReady = true;
                        connection.inspecting = reply.inspection === true;
                        inspectWindow(connection, { type: 'attach' });
                        scheduleEditor(connection);
                    }
                } catch (error) {
                    connection.syncError = error.message;
                    editor.destroy();
                }
            }
        });
        editor.on('error', error => { connection.syncError = error.message; });
        editor.on('close', () => {
            connection.editorReady = false;
            connection.inspecting = false;
            clearTimeout(connection.editorTimer);
        });
    }

    function changed(document) {
        const connection = active;
        if (connection?.editorReady && document.languageId === 'xrack'
            && document.uri.scheme === 'file'
            && vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath === connection.folder) {
            scheduleEditor(connection);
        }
    }

    async function attach(syncEditor) {
        const request = ++selection;
        try {
            const discovered = await runningApps();
            const apps = syncEditor ? discovered.filter(app => app.editorSocket) : discovered;
            if (request !== selection) return;
            if (!apps.length) {
                const message = syncEditor
                    ? 'Start an XRack app built with editor sync in this workspace.'
                    : 'Start an XRack live-reload application in this workspace.';
                void vscode.window.showInformationMessage(message);
                return;
            }
            const app = apps.length === 1 ? apps[0] : await vscode.window.showQuickPick(apps, {
                placeHolder: 'Choose an XRack application',
            });
            if (!app || request !== selection) return;
            if (active?.socket === app.socket && !active.closed
                && (syncEditor ? active.editor && !active.editor.destroyed : !active.editor)) {
                active.panel.reveal();
                return;
            }
            detach();
            const panel = vscode.window.createWebviewPanel(
                'xrackRunningApp', 'XRack Running App', vscode.ViewColumn.Beside, {
                    enableScripts: true,
                    localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, 'media')],
                },
            );
            const connection = active = { ...app, panel, error: 'Waiting for the application…' };
            const messages = panel.webview.onDidReceiveMessage(message => {
                if (active !== connection) return;
                if (message?.type === 'ready') {
                    connection.ready = true;
                    connection.display = undefined;
                    show(connection);
                } else if (message?.type === 'copyRecording' && connection.snapshot?.recording) {
                    const recording = JSON.stringify(connection.snapshot.recording, null, 2);
                    void vscode.env.clipboard.writeText(recording).catch(error => {
                        void vscode.window.showErrorMessage('XRack recording: ' + error.message);
                    });
                } else if (message?.type === 'source') {
                    void openSource(connection, message).catch(error => {
                        void vscode.window.showErrorMessage('XRack definition: ' + error.message);
                    });
                } else if (message?.type === 'select'
                    && connection.snapshot?.tree?.nodes.some(node => node.key === message.key)) {
                    inspectWindow(connection, { type: 'select', key: message.key });
                } else if (message?.type === 'pick' && typeof message.active === 'boolean') {
                    inspectWindow(connection, { type: 'pick', active: message.active });
                }
            });
            panel.onDidDispose(() => {
                messages.dispose();
                if (connection === active) {
                    active = undefined;
                    clearTimeout(timer);
                    stopEditor(connection);
                }
            });
            panel.webview.html = page(panel.webview, vscode.Uri.joinPath(context.extensionUri, 'media'));
            if (syncEditor) startEditor(connection);
            void refresh(connection);
        } catch (error) {
            void vscode.window.showErrorMessage('XRack attach failed: ' + error.message);
        }
    }

    context.subscriptions.push(
        { dispose: detach },
        vscode.workspace.onDidChangeTextDocument(event => {
            if (event.contentChanges.length) changed(event.document);
        }),
        vscode.workspace.onDidSaveTextDocument(changed),
        vscode.workspace.onDidCloseTextDocument(changed),
        vscode.commands.registerCommand('xrack.detachRunningApp', detach),
        vscode.commands.registerCommand('xrack.attachRunningApp', () => attach(false)),
        vscode.commands.registerCommand('xrack.syncRunningAppEditor', () => attach(true)),
    );
}

module.exports = { registerAttach, readSnapshot };
