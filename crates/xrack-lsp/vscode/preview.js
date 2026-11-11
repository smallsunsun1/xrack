const { execFile } = require('node:child_process');
const { randomBytes } = require('node:crypto');
const fs = require('node:fs');
const files = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const vscode = require('vscode');
const { PreviewQueue } = require('./preview-queue');
const { PreviewProcess } = require('./preview-process');

function run(command, args, cwd, input) {
    return new Promise((resolve, reject) => {
        const child = execFile(command, args, {
            cwd, windowsHide: true, maxBuffer: 64 * 1024 * 1024, timeout: 10 * 60 * 1000,
        }, (error, stdout, stderr) => {
            if (error) reject(new Error(stderr.trim() || error.message));
            else resolve(stdout);
        });
        child.stdin.on('error', () => {});
        child.stdin.end(input);
    });
}

function findCheckout(directory) {
    while (directory) {
        if (fs.existsSync(path.join(directory, 'crates/xrack-runtime/Cargo.toml'))
            && fs.existsSync(path.join(directory, 'crates/xrack-renderer-femtovg/Cargo.toml'))) {
            return directory;
        }
        const parent = path.dirname(directory);
        if (parent === directory) break;
        directory = parent;
    }
}

function compilerFor(context, document, checkout) {
    const root = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath
        ?? path.dirname(document.uri.fsPath);
    const configured = vscode.workspace.getConfiguration('xrack', document.uri)
        .get('preview.compilerPath', '').trim();
    if (configured) return path.resolve(root, configured);
    const executable = process.platform === 'win32' ? 'xrackc.exe' : 'xrackc';
    const bundled = context.asAbsolutePath(path.join('server', executable));
    const built = checkout && path.join(checkout, 'bazel-bin/crates/xrack-compiler', executable);
    if (built && fs.existsSync(built)) return built;
    return fs.existsSync(bundled) ? bundled : executable;
}

function canonical(file) {
    try {
        return fs.realpathSync(file);
    } catch {
        return path.normalize(file);
    }
}

async function prepareProject(context) {
    const directory = await files.mkdtemp(path.join(os.tmpdir(), 'xrack-preview-'));
    try {
        await files.mkdir(path.join(directory, 'src'));
        await files.copyFile(context.asAbsolutePath('preview-host.rs'), path.join(directory, 'src/main.rs'));
        return directory;
    } catch (error) {
        await files.rm(directory, { recursive: true, force: true });
        throw error;
    }
}

function manifest(name, version, checkout) {
    if (!/^\d+\.\d+\.\d+(?:[-+][\w.+-]+)?$/.test(version)) {
        throw new Error('xrackc returned an invalid preview version');
    }
    const dependency = name => checkout
        ? `{ path = ${JSON.stringify(path.join(checkout, 'crates', name))} }`
        : `"=${version}"`;
    return `[package]
name = "${name}"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[profile.dev.package.${name}]
debug = 0

[dependencies]
xrack-runtime = ${dependency('xrack-runtime')}
xrack-layout = ${dependency('xrack-layout')}
xrack-renderer-femtovg = ${dependency('xrack-renderer-femtovg')}
pollster = "1.0"
wgpu = { version = "=30.0.0", default-features = false, features = ["std", "wgsl", "metal", "vulkan", "dx12"] }
`;
}

async function writeChanged(file, content) {
    const old = await files.readFile(file, 'utf8').catch(error => {
        if (error.code !== 'ENOENT') throw error;
    });
    if (old !== content) await files.writeFile(file, content);
}

async function buildPreview(name, directory, target, cwd) {
    const args = ['build', '--quiet', '--message-format=json-render-diagnostics',
        '--manifest-path', path.join(directory, 'Cargo.toml'), '--target-dir', target];
    let output;
    try {
        // A populated local cache should not have to wait for the registry.
        output = await run('cargo', [...args, '--offline'], cwd);
    } catch (error) {
        if (!/offline mode|--offline was specified/.test(error.message)) throw error;
        output = await run('cargo', args, cwd);
    }
    const artifact = output.split('\n').filter(line => line.startsWith('{'))
        .map(line => JSON.parse(line))
        .find(message => message.reason === 'compiler-artifact' && message.target.name === name
            && message.executable);
    if (!artifact) throw new Error('Cargo did not produce a preview executable.');
    const program = path.join(directory, `live-${randomBytes(6).toString('hex')}${process.platform === 'win32' ? '.exe' : ''}`);
    await files.copyFile(artifact.executable, program, fs.constants.COPYFILE_FICLONE);
    return program;
}

function page(webview, media, filename) {
    const nonce = randomBytes(16).toString('hex');
    const resource = name => webview.asWebviewUri(vscode.Uri.joinPath(media, name));
    const title = filename.replace(/[&<>"']/g, char => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[char]);
    return `<!doctype html>
<html lang="en">
<head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${resource('preview.css')}">
<title>XRack Preview</title></head>
<body>
<header class="toolbar">
    <div class="brand">
        <img src="${resource('xrack.png')}" alt="" width="34" height="34">
        <div class="brand-copy">
            <h1>XRack Preview</h1>
            <div class="filename" title="${title}">${title}</div>
        </div>
    </div>
    <div class="controls">
        <label class="component" for="component">Component <select id="component" disabled></select></label>
        <span class="status" id="status" data-phase="busy" role="status">Starting preview…</span>
    </div>
</header>
<details id="diagnostics" class="diagnostics" hidden open>
    <summary>Preview diagnostics</summary>
    <pre id="error"></pre>
</details>
<main class="canvas" aria-label="Component preview">
    <div class="waiting" id="waiting">
        <strong>Waiting for the first frame</strong>
        <p>The rendered component appears here once the build completes.</p>
    </div>
    <img id="snapshot" alt="XRack component preview" draggable="false" hidden>
</main>
<footer class="statusbar">
    <span>Refreshes as you edit</span>
    <span id="dimensions">No frame yet</span>
</footer>
<script nonce="${nonce}">
const vscode = acquireVsCodeApi();
const select = document.getElementById('component');
const status = document.getElementById('status');
const diagnostics = document.getElementById('diagnostics');
const error = document.getElementById('error');
const waiting = document.getElementById('waiting');
const dimensions = document.getElementById('dimensions');
const image = document.getElementById('snapshot');
let frame;
select.onchange = () => vscode.postMessage({ type: 'select', component: select.value });
image.addEventListener('click', event => {
    if (frame === undefined || event.button !== 0) return;
    const bounds = image.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    vscode.postMessage({
        type: 'click', frame,
        x: (event.clientX - bounds.left) * image.naturalWidth / bounds.width,
        y: (event.clientY - bounds.top) * image.naturalHeight / bounds.height,
    });
});
window.addEventListener('message', ({data}) => {
    if ('status' in data) {
        status.textContent = data.status;
        status.dataset.phase = data.phase;
        diagnostics.hidden = !data.error;
        if (data.error) diagnostics.open = true;
        error.textContent = data.error || '';
    }
    if (data.components) {
        select.replaceChildren(...data.components.map(name => new Option(name, name)));
        select.value = data.component;
        select.disabled = false;
    }
    if (data.image) {
        const source = 'data:image/png;base64,' + data.image;
        const show = () => {
            frame = data.frame;
            image.hidden = false;
            waiting.hidden = true;
            dimensions.textContent = image.naturalWidth + ' × ' + image.naturalHeight + ' px';
        };
        if (image.src === source && image.complete && image.naturalWidth) {
            show();
        } else {
            frame = undefined;
            image.onload = show;
            image.src = source;
        }
    }
});
vscode.postMessage({ type: 'ready' });
</script></body></html>`;
}

function registerPreview(context) {
    let current;
    context.subscriptions.push(vscode.commands.registerCommand('xrack.openPreview', async () => {
        const document = vscode.window.activeTextEditor?.document;
        if (document?.languageId !== 'xrack' || document.uri.scheme !== 'file') {
            void vscode.window.showInformationMessage('Open a saved .xr file to start its preview.');
            return;
        }
        if (current?.uri === document.uri.toString()) {
            current.panel.reveal(vscode.ViewColumn.Beside, true);
            return;
        }
        current?.panel.dispose();
        const media = vscode.Uri.joinPath(context.extensionUri, 'media');
        const panel = vscode.window.createWebviewPanel(
            'xrackPreview', `XRack Preview · ${path.basename(document.fileName)}`,
            { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
            { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [media] },
        );
        panel.iconPath = {
            light: vscode.Uri.joinPath(media, 'preview-light.svg'),
            dark: vscode.Uri.joinPath(media, 'preview-dark.svg'),
        };
        current = { panel, uri: document.uri.toString() };
        let project;
        let buildCwd;
        let dependencies;
        const target = path.join(context.globalStorageUri.fsPath, 'preview-target');
        const name = `xrack-editor-preview-${randomBytes(6).toString('hex')}`;
        let component;
        let session;
        let starting;
        let clicking = false;
        let disposed = false;
        let frame = 0;
        let ready = false;
        const showFrame = (image, metadata = {}) => {
            void panel.webview.postMessage({ ...metadata, image, frame: ++frame });
        };
        const queue = new PreviewQueue(async isCurrent => {
            const post = payload => {
                if (isCurrent()) void panel.webview.postMessage(payload);
            };
            try {
                post({ status: 'Compiling preview…', phase: 'busy' });
                const checkout = findCheckout(path.dirname(document.uri.fsPath))
                    ?? findCheckout(context.asAbsolutePath('preview-sdk'));
                const compiler = compilerFor(context, document, checkout);
                const sdk = checkout ?? (path.isAbsolute(compiler) && findCheckout(path.dirname(compiler)));
                const cwd = sdk || path.dirname(document.uri.fsPath);
                const buffers = Object.fromEntries(vscode.workspace.textDocuments
                    .filter(doc => doc.languageId === 'xrack' && doc.uri.scheme === 'file'
                        && doc !== document && doc.isDirty)
                    .map(doc => [doc.uri.fsPath, doc.getText()]));
                const output = await run(compiler, [document.uri.fsPath, '--emit', 'preview'], cwd,
                    JSON.stringify({ text: document.getText(), buffers }));
                if (!isCurrent()) return;
                const compiled = JSON.parse(output);
                if (compiled.protocol !== 2) {
                    throw new Error('Live preview needs the current xrackc. Rebuild it or use the bundled compiler.');
                }
                const { components, rust, version, inputs } = compiled;
                dependencies = new Set(inputs.map(canonical));
                if (!components.length) throw new Error('This file has no component to preview.');
                if (!components.includes(component)) [component] = components;
                const selected = component;
                const config = manifest(name, version, sdk);
                if (session?.rust === rust && session.config === config && session.renderer.alive) {
                    const previous = session;
                    if (previous.component !== selected) {
                        previous.image = await previous.renderer.frame('select\t' + selected);
                        previous.component = selected;
                    }
                    if (isCurrent()) showFrame(previous.image, {
                        status: 'Live preview', phase: 'live', components, component: selected,
                    });
                    return;
                }
                const directory = await (project ??= prepareProject(context));
                await Promise.all([
                    writeChanged(path.join(directory, 'src/ui.rs'), rust),
                    writeChanged(path.join(directory, 'Cargo.toml'), config),
                ]);
                if (!isCurrent()) return;
                post({ status: 'Building preview…', phase: 'busy' });
                buildCwd = cwd;
                const program = await buildPreview(name, directory, target, cwd);
                if (!isCurrent()) {
                    await files.rm(program, { force: true });
                    return;
                }
                post({ status: 'Rendering preview…', phase: 'busy' });
                const renderer = new PreviewProcess(program, selected, cwd);
                starting = renderer;
                try {
                    const image = await renderer.frame();
                    if (!isCurrent()) return;
                    void session?.renderer.stop();
                    session = { renderer, image, rust, config, component: selected };
                    showFrame(image, { status: 'Live preview', phase: 'live', components, component: selected });
                } finally {
                    starting = undefined;
                    if (session?.renderer !== renderer) void renderer.stop();
                }
            } catch (error) {
                if (isCurrent()) dependencies = undefined;
                post({ status: 'Preview unavailable', phase: 'error', error: error.message });
            }
        });
        async function click(message) {
            if (clicking || queue.busy || message.frame !== frame || !session
                || !Number.isFinite(message.x) || !Number.isFinite(message.y)
                || message.x < 0 || message.y < 0 || message.x >= 2048 || message.y >= 2048) return;
            const active = session;
            clicking = true;
            try {
                active.image = await active.renderer.frame(`click\t${message.x}\t${message.y}`);
                if (!disposed && session === active && message.frame === frame) showFrame(active.image);
            } catch (error) {
                if (!disposed && session === active) {
                    session = undefined;
                    void panel.webview.postMessage({ status: 'Preview unavailable', phase: 'error', error: error.message });
                }
            } finally {
                clicking = false;
            }
        }
        const affected = uri => ready && uri.scheme === 'file' && (
            uri.toString() === document.uri.toString()
            || (dependencies ? dependencies.has(canonical(uri.fsPath)) : path.extname(uri.fsPath) === '.xr')
        );
        const watcher = vscode.workspace.createFileSystemWatcher('**/*.xr');
        const onDiskChange = uri => {
            if (affected(uri)) queue.refresh();
        };
        const events = [
            watcher,
            watcher.onDidCreate(onDiskChange),
            watcher.onDidChange(onDiskChange),
            watcher.onDidDelete(onDiskChange),
            panel.webview.onDidReceiveMessage(message => {
                if (message.type === 'ready') {
                    ready = true;
                    queue.refresh(0);
                } else if (message.type === 'select' && typeof message.component === 'string') {
                    component = message.component;
                    queue.refresh(0);
                } else if (message.type === 'click') {
                    void click(message);
                }
            }),
            vscode.workspace.onDidChangeTextDocument(event => {
                if (event.contentChanges.length && affected(event.document.uri)) {
                    queue.refresh();
                }
            }),
            vscode.workspace.onDidSaveTextDocument(saved => {
                if (affected(saved.uri)) queue.refresh();
            }),
            vscode.workspace.onDidCloseTextDocument(closed => {
                if (closed === document) panel.dispose();
                else if (affected(closed.uri)) queue.refresh();
            }),
        ];
        panel.onDidDispose(() => {
            disposed = true;
            if (current?.panel === panel) current = undefined;
            for (const event of events) event.dispose();
            void Promise.allSettled([queue.dispose(), session?.renderer.stop(), starting?.stop()])
                .then(() => project)
                .then(async directory => {
                    if (!directory) return;
                    if (buildCwd && fs.existsSync(path.join(directory, 'Cargo.lock'))) {
                        // Remove this panel's executable and incremental data, keeping shared dependencies.
                        await run('cargo', ['clean', '--quiet', '--offline', '--package', name,
                            '--manifest-path', path.join(directory, 'Cargo.toml'), '--target-dir', target],
                        buildCwd).catch(() => {});
                    }
                    await files.rm(directory, { recursive: true, force: true });
                })
                .catch(() => {});
        });
        panel.webview.html = page(panel.webview, media, path.basename(document.fileName));
    }));
}

module.exports = { registerPreview };
