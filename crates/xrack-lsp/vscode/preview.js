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

async function presentNativeDialog(panel, cwd) {
    if (panel.kind === 'file') {
        if (panel.filter_error) return { outcome: 'failed', message: panel.filter_error };
        const title = panel.title || undefined;
        const directory = panel.directory && path.resolve(cwd, panel.directory);
        const location = panel.file_name ? path.join(directory || cwd, panel.file_name) : directory;
        const defaultUri = location ? vscode.Uri.file(location) : undefined;
        const filters = Object.create(null);
        const allFiles = panel.filters.some(([, extensions]) => extensions.includes('*'));
        if (!(process.platform === 'darwin' && allFiles)) {
            for (const [name, extensions] of panel.filters) {
                filters[name] = [...new Set([...(filters[name] || []), ...extensions])];
            }
        }
        const fileFilters = Object.keys(filters).length ? filters : undefined;
        if (panel.save) {
            const file = await vscode.window.showSaveDialog({ title, defaultUri, filters: fileFilters });
            return file ? { outcome: 'files', paths: [file.fsPath] } : { outcome: 'canceled' };
        }
        const files = await vscode.window.showOpenDialog({
            title, defaultUri, filters: fileFilters, canSelectFiles: !panel.directories,
            canSelectFolders: panel.directories, canSelectMany: panel.multiple,
        });
        return files?.length ? { outcome: 'files', paths: files.map(file => file.fsPath) } : { outcome: 'canceled' };
    }
    const labels = { ok: vscode.l10n.t('OK'), yes: vscode.l10n.t('Yes'),
        no: vscode.l10n.t('No'), cancel: vscode.l10n.t('Cancel') };
    const choices = panel.buttons.map(id => ({ id, title: labels[id], isCloseAffordance: id === 'cancel' }));
    const show = {
        info: 'showInformationMessage', warning: 'showWarningMessage', error: 'showErrorMessage',
    }[panel.level];
    const detail = (panel.title ? [panel.text, panel.informative_text] : [panel.informative_text])
        .filter(Boolean).join('\n\n');
    const choice = await vscode.window[show](panel.title || panel.text, { modal: true, detail }, ...choices);
    return { outcome: 'button', button: choice?.id ?? 'cancel' };
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

function interpreterFor(context, checkout) {
    const executable = process.platform === 'win32' ? 'xrack-preview.exe' : 'xrack-preview';
    const built = checkout && path.join(checkout, 'bazel-bin/crates/xrack-preview', executable);
    if (built && fs.existsSync(built)) return built;
    const bundled = context.asAbsolutePath(path.join('server', executable));
    return fs.existsSync(bundled) ? bundled : undefined;
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
        <label class="component" id="window-picker" for="window" hidden>Window <select id="window"></select></label>
        <div class="view-controls" id="view-controls" role="group" aria-label="Preview view" hidden>
            <button type="button" id="pan-mode" aria-pressed="false" title="Pan by dragging (or Alt + drag)">Pan</button>
            <button type="button" id="zoom-out" aria-label="Zoom out" title="Zoom out (Alt + wheel)">−</button>
            <button type="button" id="zoom-actual" aria-label="Reset to actual size" title="Reset to actual size">100%</button>
            <button type="button" id="zoom-in" aria-label="Zoom in" title="Zoom in (Alt + wheel)">+</button>
            <button type="button" id="zoom-fit" title="Fit preview to panel">Fit</button>
        </div>
        <span class="status" id="status" data-phase="busy" role="status">Starting preview…</span>
    </div>
</header>
<details id="diagnostics" class="diagnostics" hidden open>
    <summary>Preview diagnostics</summary>
    <pre id="error"></pre>
</details>
<main class="canvas" id="canvas" aria-label="Component preview">
    <div class="waiting" id="waiting">
        <strong>Waiting for the first frame</strong>
        <p>The rendered component appears here once loading completes.</p>
    </div>
    <img id="snapshot" alt="XRack component preview" draggable="false" hidden>
    <button type="button" id="restore-window" class="restore-window" hidden>Restore preview window</button>
    <textarea id="editor" class="preview-editor" aria-label="XRack preview keyboard input" tabindex="-1"></textarea>
</main>
<footer class="statusbar">
    <span>Refreshes as you edit</span>
    <span id="dimensions">No frame yet</span>
</footer>
<script nonce="${nonce}">
const vscode = acquireVsCodeApi();
const select = document.getElementById('component');
const windowPicker = document.getElementById('window-picker');
const windowSelect = document.getElementById('window');
const status = document.getElementById('status');
const diagnostics = document.getElementById('diagnostics');
const error = document.getElementById('error');
const waiting = document.getElementById('waiting');
const dimensions = document.getElementById('dimensions');
const image = document.getElementById('snapshot');
const editor = document.getElementById('editor');
const canvas = document.getElementById('canvas');
const viewControls = document.getElementById('view-controls');
const panMode = document.getElementById('pan-mode');
const zoomActual = document.getElementById('zoom-actual');
const restoreWindow = document.getElementById('restore-window');
restoreWindow.addEventListener('click', () => vscode.postMessage({ type: 'restore_window' }));
const edit = (action, text, select) => vscode.postMessage({ type: 'edit', action, text, select });
let nextComposition = 0;
let composition;
let compositionCommit;
const compose = (phase, id, text) => vscode.postMessage({ type: 'composition', phase, id, text });
function cancelComposition() {
    if (composition !== undefined) {
        compose('cancel', composition);
        composition = undefined;
    }
}
const isMac = navigator.platform.startsWith('Mac');
const sendKey = (event, pressed, text = '') => vscode.postMessage({
    type: 'key', key: event.key, pressed, text, repeat: event.repeat,
    control: event.ctrlKey, shift: event.shiftKey, alt: event.altKey, meta: event.metaKey,
    primary: isMac ? event.metaKey : event.ctrlKey,
});
editor.addEventListener('keydown', event => {
    if (!dynamic || event.isComposing || composition !== undefined) return;
    compositionCommit = undefined;
    const modified = event.ctrlKey || event.metaKey || event.altKey;
    const text = Array.from(event.key).length === 1
        && (!modified || event.getModifierState('AltGraph')) ? event.key : '';
    const command = !event.altKey && (event.ctrlKey || event.metaKey)
        && ['a', 'c', 'v', 'x', 'y', 'z'].includes(event.key.toLowerCase());
    if (text || command || ['Enter', 'Escape', 'Backspace', 'Delete', 'ArrowLeft',
        'ArrowRight', 'Home', 'End', 'BrowserBack'].includes(event.key)
        || event.key === 'ContextMenu' || event.key === 'F10' && event.shiftKey
        || event.key === 'Tab' && !modified
        || (treeNavigationEnabled || splitNavigationEnabled || controlNavigationEnabled || menuOpen)
            && ['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown'].includes(event.key)) {
        event.preventDefault();
    }
    if (event.key !== 'Tab' || !modified) sendKey(event, true, text);
});
editor.addEventListener('keyup', event => {
    if (dynamic && !event.isComposing && composition === undefined) sendKey(event, false);
});
editor.addEventListener('blur', () => {
    cancelComposition();
    if (dynamic) vscode.postMessage({ type: 'delay_key', key: null, pressed: false });
});
editor.addEventListener('beforeinput', event => {
    if (event.isComposing) return;
    event.preventDefault();
    const duplicate = compositionCommit?.text === event.data;
    compositionCommit = undefined;
    if (composition !== undefined) return;
    if (event.inputType === 'insertText' && event.data && !duplicate) edit('insert', event.data);
    if (event.inputType === 'insertLineBreak') edit('insert', '\\n');
});
editor.addEventListener('compositionstart', () => {
    if (!dynamic) return;
    cancelComposition();
    compositionCommit = undefined;
    composition = ++nextComposition;
    compose('start', composition);
});
editor.addEventListener('compositionupdate', event => {
    if (composition !== undefined) compose('update', composition, event.data);
});
editor.addEventListener('compositionend', event => {
    if (composition !== undefined) {
        compose(event.data ? 'commit' : 'cancel', composition, event.data);
        composition = undefined;
    }
    if (event.data) {
        const commit = compositionCommit = { text: event.data };
        setTimeout(() => {
            if (compositionCommit === commit) compositionCommit = undefined;
        }, 0);
    }
    editor.value = '';
});
editor.addEventListener('paste', event => {
    event.preventDefault();
    const text = event.clipboardData?.getData('text/plain');
    if (text) edit('paste', text);
});
let frame;
let dynamic = false;
let treeNavigationEnabled = false;
let controlNavigationEnabled = false;
let splitNavigationEnabled = false;
let menuOpen = false;
let imeArea;
let logicalWidth;
let logicalHeight;
const drags = new Map();
let skipClick = false;
let zoom = 1;
let offsetX = 0;
let offsetY = 0;
let fit = true;
let pan;
let densityTimer;
let reportedDensity;

function reportDensity(type) {
    const density = Math.min(4, Math.max(1, (window.devicePixelRatio || 1) * zoom));
    if (density === reportedDensity && type !== 'ready') return;
    reportedDensity = density;
    vscode.postMessage({ type, scale: density });
}

function layoutPreview() {
    if (!logicalWidth || !logicalHeight) return;
    if (fit) {
        const style = getComputedStyle(canvas);
        const paddingX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
        const paddingY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
        zoom = Math.max(0.05, Math.min(1,
            (canvas.clientWidth - paddingX) / logicalWidth,
            (canvas.clientHeight - paddingY) / logicalHeight,
        ));
    }
    image.style.width = (logicalWidth * zoom) + 'px';
    image.style.height = (logicalHeight * zoom) + 'px';
    image.style.transform = 'translate(calc(-50% + ' + offsetX + 'px), calc(-50% + ' + offsetY + 'px))';
    zoomActual.textContent = Math.round(zoom * 100) + '%';
    positionEditor();
}

function updateZoom(value, anchor) {
    const next = Math.max(0.05, Math.min(4, value));
    if (anchor) {
        const bounds = canvas.getBoundingClientRect();
        const x = anchor.clientX - bounds.left - bounds.width / 2;
        const y = anchor.clientY - bounds.top - bounds.height / 2;
        offsetX = x + (offsetX - x) * next / zoom;
        offsetY = y + (offsetY - y) * next / zoom;
    } else {
        offsetX = 0;
        offsetY = 0;
    }
    zoom = next;
    fit = false;
    layoutPreview();
    clearTimeout(densityTimer);
    densityTimer = setTimeout(() => {
        densityTimer = undefined;
        reportDensity('scale');
    }, 100);
}

panMode.addEventListener('click', () => {
    const enabled = canvas.classList.toggle('pan-mode');
    panMode.setAttribute('aria-pressed', String(enabled));
    if (enabled && dynamic && !drags.size) vscode.postMessage({ type: 'leave' });
});
document.getElementById('zoom-out').addEventListener('click', () => updateZoom(zoom / 1.25, canvasCenter()));
document.getElementById('zoom-in').addEventListener('click', () => updateZoom(zoom * 1.25, canvasCenter()));
zoomActual.addEventListener('click', () => updateZoom(1));
document.getElementById('zoom-fit').addEventListener('click', () => {
    fit = true;
    offsetX = 0;
    offsetY = 0;
    layoutPreview();
    reportDensity('scale');
});
function canvasCenter() {
    const bounds = canvas.getBoundingClientRect();
    return { clientX: bounds.left + bounds.width / 2, clientY: bounds.top + bounds.height / 2 };
}
new ResizeObserver(() => {
    layoutPreview();
    reportDensity('scale');
}).observe(canvas);

canvas.addEventListener('pointerdown', event => {
    if (event.button !== 0 || !event.isPrimary || pan || drags.size || image.hidden
        || (!event.altKey && !canvas.classList.contains('pan-mode') && event.target === image)) return;
    event.preventDefault();
    event.stopPropagation();
    skipClick = true;
    fit = false;
    pan = { id: event.pointerId, x: event.clientX - offsetX, y: event.clientY - offsetY };
    canvas.setPointerCapture(event.pointerId);
    canvas.classList.add('panning');
    if (dynamic) vscode.postMessage({ type: 'leave' });
}, true);
canvas.addEventListener('pointermove', event => {
    if (pan?.id !== event.pointerId) return;
    offsetX = event.clientX - pan.x;
    offsetY = event.clientY - pan.y;
    layoutPreview();
});
function endPan(event) {
    if (pan?.id !== event.pointerId) return;
    pan = undefined;
    canvas.classList.remove('panning');
}
canvas.addEventListener('pointerup', endPan);
canvas.addEventListener('pointercancel', endPan);
canvas.addEventListener('lostpointercapture', endPan);
canvas.addEventListener('wheel', event => {
    if (image.hidden || pan || drags.size
        || !(event.altKey || (event.ctrlKey && canvas.classList.contains('pan-mode')))) return;
    event.preventDefault();
    event.stopPropagation();
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? canvas.clientHeight : 1;
    updateZoom(zoom * Math.exp(-event.deltaY * unit / 450), event);
}, { capture: true, passive: false });

function positionEditor() {
    const bounds = image.getBoundingClientRect();
    if (!imeArea || !logicalWidth || !logicalHeight) {
        editor.style.left = '';
        editor.style.top = '';
        editor.style.height = '';
        return;
    }
    const [x, y, height] = imeArea;
    editor.style.left = (bounds.left + x * bounds.width / logicalWidth) + 'px';
    editor.style.top = (bounds.top + y * bounds.height / logicalHeight) + 'px';
    editor.style.height = Math.max(1, height * bounds.height / logicalHeight) + 'px';
}
window.addEventListener('resize', positionEditor);
window.addEventListener('scroll', positionEditor, true);

function imagePoint(event) {
    const bounds = image.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    return {
        x: (event.clientX - bounds.left) * logicalWidth / bounds.width,
        y: (event.clientY - bounds.top) * logicalHeight / bounds.height,
    };
}

function pointerMessage(drag, event, phase, extra = {}) {
    vscode.postMessage({
        type: 'pointer', phase, contact: event.pointerId, primary: drag.primary,
        touch: drag.touch, button: drag.button, pan: drag.pan, ...drag.current,
        control: event.ctrlKey, shift: event.shiftKey, alt: event.altKey, meta: event.metaKey,
        ...extra,
    });
}

image.addEventListener('dragover', event => {
    if (dynamic && frame !== undefined && event.dataTransfer?.types.includes('text/uri-list')) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
    }
});
image.addEventListener('drop', event => {
    event.preventDefault();
    if (!dynamic || frame === undefined) return;
    const uris = event.dataTransfer?.getData('text/uri-list');
    const point = imagePoint(event);
    if (uris && point) vscode.postMessage({ type: 'file_drop', frame, ...point, uris });
});
image.addEventListener('pointerdown', event => {
    if (event.button < 0 || event.button > 2 || !event.isPrimary && event.pointerType !== 'touch') return;
    if (event.isPrimary) skipClick = false;
    if (!dynamic || frame === undefined) return;
    const point = imagePoint(event);
    if (!point) return;
    if (event.isPrimary) cancelComposition();
    const drag = {
        current: point, clientX: event.clientX, clientY: event.clientY,
        moved: false, pan: event.button === 1, primary: event.isPrimary,
        touch: event.pointerType === 'touch',
        button: ['primary', 'middle', 'secondary'][event.button],
    };
    drags.set(event.pointerId, drag);
    skipClick = true;
    if (drag.pan || drag.button === 'secondary') event.preventDefault();
    image.setPointerCapture(event.pointerId);
    editor.focus({ preventScroll: true });
    pointerMessage(drag, event, 'press', { frame });
});
function trackPointer(drag, event) {
    drag.moved ||= Math.hypot(event.clientX - drag.clientX, event.clientY - drag.clientY) >= 4;
    drag.current = imagePoint(event) ?? drag.current;
}
image.addEventListener('pointermove', event => {
    const drag = drags.get(event.pointerId);
    if (!drag) {
        if (!dynamic || frame === undefined || event.pointerType === 'touch'
            || canvas.classList.contains('pan-mode')) return;
        const point = imagePoint(event);
        if (point) vscode.postMessage({ type: 'hover', frame, ...point });
        return;
    }
    trackPointer(drag, event);
    pointerMessage(drag, event, 'move');
});
function finishPointer(event, phase) {
    const drag = drags.get(event.pointerId);
    if (!drag) return;
    drags.delete(event.pointerId);
    trackPointer(drag, event);
    const click = phase === 'release' && !drag.moved && drag.button === 'primary';
    pointerMessage(drag, event, phase, { click });
    if (phase === 'release' && drag.button === 'secondary' && frame !== undefined) {
        vscode.postMessage({ type: 'context_menu', frame, ...drag.current });
    }
}
image.addEventListener('pointerleave', () => {
    if (!drags.size && dynamic) vscode.postMessage({ type: 'leave' });
});
image.addEventListener('pointerup', event => finishPointer(event, 'release'));
image.addEventListener('pointercancel', event => finishPointer(event, 'cancel'));
image.addEventListener('lostpointercapture', event => finishPointer(event, 'cancel'));
select.onchange = () => vscode.postMessage({ type: 'select', component: select.value });
windowSelect.onchange = () => vscode.postMessage({
    type: 'select_window', window: windowSelect.value === '' ? null : Number(windowSelect.value),
});
image.addEventListener('wheel', event => {
    if (!dynamic || frame === undefined) return;
    const bounds = image.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    event.preventDefault();
    const scaleX = logicalWidth / bounds.width;
    const scaleY = logicalHeight / bounds.height;
    const unit = event.deltaMode === 1 ? 28 : 1;
    vscode.postMessage({
        type: 'wheel', frame, shift: event.shiftKey, control: event.ctrlKey,
        alt: event.altKey, meta: event.metaKey,
        lines: event.deltaMode === 1 ? [event.deltaX, event.deltaY] : undefined,
        x: (event.clientX - bounds.left) * scaleX,
        y: (event.clientY - bounds.top) * scaleY,
        dx: event.deltaX * (event.deltaMode === 2 ? logicalWidth : scaleX * unit),
        dy: event.deltaY * (event.deltaMode === 2 ? logicalHeight : scaleY * unit),
    });
}, { passive: false });
image.addEventListener('contextmenu', event => {
    if (dynamic) event.preventDefault();
});
image.addEventListener('click', event => {
    if (skipClick || frame === undefined || event.button !== 0) return;
    const point = imagePoint(event);
    if (!point) return;
    editor.focus({ preventScroll: true });
    vscode.postMessage({ type: 'click', frame, ...point });
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
        cancelComposition();
        select.replaceChildren(...data.components.map(name => new Option(name, name)));
        select.value = data.component;
        select.disabled = false;
    }
    if (data.windows) {
        const windows = data.windows;
        const selectedWindow = String(data.window ?? '');
        if (windowSelect.value !== selectedWindow) cancelComposition();
        windowPicker.hidden = windows.length < 2;
        if (windows.length !== windowSelect.options.length || windows.some((window, index) => {
            const option = windowSelect.options[index];
            return option.value !== String(window.id ?? '') || option.text !== window.title;
        })) {
            windowSelect.replaceChildren(...windows.map(window => new Option(window.title, String(window.id ?? ''))));
        }
        windowSelect.value = selectedWindow;
    } else if (data.image && !data.dynamic) {
        windowPicker.hidden = true;
    }
    if (data.windowClosed) {
        cancelComposition();
        if (pan && canvas.hasPointerCapture(pan.id)) canvas.releasePointerCapture(pan.id);
        pan = undefined;
        canvas.classList.remove('panning');
        imeArea = undefined;
        positionEditor();
        windowPicker.hidden = true;
        image.onload = null;
        frame = undefined;
        dynamic = false;
        image.hidden = true;
        restoreWindow.hidden = true;
        viewControls.hidden = true;
        logicalWidth = undefined;
        logicalHeight = undefined;
        waiting.textContent = 'Preview window closed';
        waiting.hidden = false;
        dimensions.textContent = 'No frame';
    }
    if (data.image) {
        const source = 'data:image/png;base64,' + data.image;
        const show = () => {
            [logicalWidth, logicalHeight] = data.size ?? [image.naturalWidth, image.naturalHeight];
            frame = data.frame;
            dynamic = Boolean(data.dynamic);
            treeNavigationEnabled = Boolean(data.treeNavigationEnabled);
            controlNavigationEnabled = Boolean(data.controlNavigationEnabled);
            splitNavigationEnabled = Boolean(data.splitNavigationEnabled);
            menuOpen = Boolean(data.menuOpen);
            image.dataset.cursor = data.cursor ?? "auto";
            const minimized = Boolean(data.windowMinimized);
            image.hidden = minimized;
            restoreWindow.hidden = !minimized;
            viewControls.hidden = minimized;
            waiting.hidden = true;
            imeArea = minimized ? undefined : data.imeArea;
            if (minimized) {
                cancelComposition();
                editor.blur();
            }
            layoutPreview();
            if (!densityTimer) reportDensity('scale');
            dimensions.textContent = logicalWidth + ' × ' + logicalHeight + ' px';
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
function watchPixelRatio(type) {
    const scale = window.devicePixelRatio || 1;
    reportDensity(type);
    window.matchMedia('(resolution: ' + scale + 'dppx)').addEventListener(
        'change', () => watchPixelRatio('scale'), { once: true },
    );
}
watchPixelRatio('ready');
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
        let interpreter;
        let interactions = Promise.resolve();
        let animationTimer;
        let disposed = false;
        let frame = 0;
        let ready = false;
        let scale = 1;
        const showFrame = (image, metadata = {}) => {
            void panel.webview.postMessage({
                ...metadata, image, size: session?.size, dynamic: session?.dynamic, frame: ++frame,
            });
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
                const executable = interpreterFor(context, checkout);
                if (executable) {
                    if (!interpreter?.alive) {
                        interpreter = new PreviewProcess(executable, undefined, cwd);
                    }
                    const renderer = interpreter;
                    const requestedScale = scale;
                    const reply = await renderer.request({
                        type: 'load', filename: document.uri.fsPath, text: document.getText(),
                        buffers, component, scale: requestedScale,
                    });
                    if (!isCurrent()) return;
                    if (!reply.unsupported) {
                        if (reply.error) throw new Error(reply.error);
                        if (!reply.image) throw new Error('Preview interpreter returned no image');
                        const activation = renderer.request({ type: 'activate' });
                        if (session?.renderer !== renderer) void session?.renderer.stop();
                        component = reply.component;
                        dependencies = new Set(reply.inputs.map(canonical));
                        pointerSessions.clear();
                        const active = session = {
                            renderer, image: reply.image, component, dynamic: true, cwd, window: reply.window ?? null,
                            size: reply.size, scale: requestedScale, windowMinimized: reply.window_minimized,
                        };
                        showFrame(reply.image, {
                            status: 'Live preview', phase: 'live',
                            components: reply.components, component,
                            windows: reply.windows, window: reply.window,
                            imeArea: reply.ime_area,
                            windowMinimized: reply.window_minimized,
                        });
                        const activated = await activation;
                        if (activated.error) throw new Error(activated.error);
                        if (isCurrent()) {
                            if (activated.window_closed) closePreviewWindow(active);
                            else {
                                scheduleNativeDialog(active, activated.native_dialog);
                                scheduleAnimation(active, reply.next_frame_ms);
                            }
                        }
                        return;
                    }
                    post({ status: 'Building preview for this component…', phase: 'busy' });
                }
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
                        Object.assign(previous, await previous.renderer.frame('select\t' + selected));
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
                const requestedScale = scale;
                const renderer = new PreviewProcess(program, selected, cwd, requestedScale);
                starting = renderer;
                try {
                    const rendered = await renderer.frame();
                    if (!isCurrent()) return;
                    if (session?.renderer !== interpreter) void session?.renderer.stop();
                    session = { renderer, ...rendered, scale: requestedScale, rust, config, component: selected };
                    showFrame(rendered.image, { status: 'Live preview', phase: 'live', components, component: selected });
                } finally {
                    starting = undefined;
                    if (session?.renderer !== renderer) void renderer.stop();
                }
            } catch (error) {
                if (isCurrent()) dependencies = undefined;
                post({ status: 'Preview unavailable', phase: 'error', error: error.message });
            }
        });
        function scheduleAnimation(active, delay) {
            clearTimeout(animationTimer);
            if (!Number.isFinite(delay) || disposed || session !== active) return;
            animationTimer = setTimeout(() => {
                if (disposed || session !== active) return;
                if (queue.busy) scheduleAnimation(active, 16);
                else interact({ type: 'frame' });
            }, delay);
        }
        let nativePanels = Promise.resolve();
        function scheduleNativeDialog(active, request) {
            if (!request) return;
            nativePanels = nativePanels.then(async () => {
                if (disposed || session !== active || active.closed) return;
                let result;
                try {
                    result = await presentNativeDialog(request, active.cwd);
                } catch (error) {
                    result = { outcome: 'failed', message: error.message || String(error) };
                }
                if (!disposed && session === active && !active.closed) {
                    interact({ type: 'native_dialog_result', id: request.id, ...result });
                }
            });
        }
        function closePreviewWindow(active) {
            if (session !== active || disposed) return;
            active.closed = true;
            clearTimeout(animationTimer);
            post({ status: 'Preview window closed', phase: 'waiting', windowClosed: true });
        }
        let pendingUpdate;
        let updatingScale = false;
        let fileDropSequence = 0;
        let compositionSession;
        const pointerSessions = new Map();
        function updateScale() {
            if (disposed || updatingScale || session?.closed) return;
            updatingScale = true;
            let pending;
            if (queue.busy) {
                pending = queue.running;
            } else if (session?.size && session.scale !== scale) {
                pending = interact({ type: 'scale', scale });
            }
            if (pending) {
                void pending.finally(() => {
                    updatingScale = false;
                    updateScale();
                });
            } else {
                updatingScale = false;
            }
        }
        function interact(message) {
            const pointer = message.type === 'pointer';
            const composing = message.type === 'composition';
            const ending = pointer && (message.phase === 'release' || message.phase === 'cancel')
                || composing && (message.phase === 'commit' || message.phase === 'cancel')
                || (message.type === 'delay_key' || message.type === 'key') && !message.pressed
                || message.type === 'native_dialog_result';
            if ((!ending && queue.busy) || !session || session.closed) return;
            const active = session;
            if (active.windowMinimized && !ending
                && !['restore_window', 'select_window', 'frame', 'scale'].includes(message.type)) return;
            const inputWindow = active.window;
            if (['click', 'wheel', 'context_menu', 'hover', 'file_drop'].includes(message.type) && (message.frame !== frame
                || !Number.isFinite(message.x) || !Number.isFinite(message.y)
                || message.x < 0 || message.y < 0 || message.x >= 2048 || message.y >= 2048)) return;
            if (!['click', 'scale'].includes(message.type) && !active.dynamic) return;
            if (composing) {
                if (message.phase === 'start') compositionSession = { active, inputWindow, id: message.id };
                else if (compositionSession?.active !== active || compositionSession.inputWindow !== inputWindow
                    || compositionSession.id !== message.id) return;
                if (ending) compositionSession = undefined;
            }
            if (message.type === 'file_drop') {
                if (typeof message.uris !== 'string') return;
                try {
                    const uris = message.uris.split(/\r?\n/).map(line => line.trim())
                        .filter(line => line && !line.startsWith('#'))
                        .map(line => vscode.Uri.parse(line, true));
                    if (!uris.length || uris.some(uri => uri.scheme !== 'file')) return;
                    message = {
                        type: 'file_drag', sequence: ++fileDropSequence, x: message.x, y: message.y,
                        paths: uris.map(uri => uri.fsPath), drop: true,
                    };
                } catch {
                    return;
                }
            }
            const contact = message.contact ?? 0;
            if (pointer && message.phase === 'press') {
                const additionalTouch = message.touch && !message.primary
                    && [...pointerSessions.values()].includes(active);
                if (message.frame === frame || additionalTouch) pointerSessions.set(contact, active);
                else pointerSessions.delete(contact);
            }
            if (pointer && pointerSessions.get(contact) !== active) return;
            if (pointer && ending) pointerSessions.delete(contact);
            if (message.type === 'select_window') pointerSessions.clear();
            let move;
            if (pointer && message.phase === 'move' || message.type === 'hover'
                || composing && message.phase === 'update') {
                if (pendingUpdate?.active === active && pendingUpdate.message.type === message.type
                    && pendingUpdate.message.contact === message.contact
                    && pendingUpdate.message.id === message.id) {
                    pendingUpdate.message = message;
                    return;
                }
                move = pendingUpdate = { active, message };
            } else if (pointer || composing || message.type === 'leave' || message.type === 'select_window') {
                pendingUpdate = undefined;
            }
            return interactions = interactions.then(async () => {
                if (move) {
                    message = move.message;
                    if (pendingUpdate === move) pendingUpdate = undefined;
                }
                if (disposed || session !== active || active.closed) return;
                if (active.windowMinimized && !ending
                    && !['restore_window', 'select_window', 'frame', 'scale'].includes(message.type)) return;
                if (active.window !== inputWindow
                    && !['select_window', 'frame', 'scale', 'native_dialog_result'].includes(message.type)) return;
                if (queue.busy && !ending) {
                    if (message.type === 'frame') scheduleAnimation(active, 16);
                    return;
                }
                if (pointer && ending && queue.busy) message = { ...message, phase: 'cancel', click: false };
                if (composing && ending && queue.busy) message = { ...message, phase: 'cancel' };
                try {
                    if (active.dynamic) {
                        let reply = await active.renderer.request(message);
                        scheduleNativeDialog(active, reply.native_dialog);
                        while (reply.clipboard && session === active && !disposed) {
                            const { id, text: write } = reply.clipboard;
                            const result = { type: 'clipboard_result', id, success: false };
                            try {
                                if (write === null) result.text = await vscode.env.clipboard.readText();
                                else await vscode.env.clipboard.writeText(write);
                                result.success = true;
                            } catch {
                                // A failed write must leave the editor selection untouched.
                            }
                            const updated = await active.renderer.request(result);
                            scheduleNativeDialog(active, updated.native_dialog);
                            if (!updated.image && !updated.error) break;
                            reply = updated;
                        }
                        if (reply.error) {
                            if (session === active) void panel.webview.postMessage({
                                status: 'Preview input error', phase: 'error', error: reply.error,
                            });
                            return;
                        }
                        if (reply.window_closed) {
                            closePreviewWindow(active);
                            return;
                        }
                        if (!reply.image) return;
                        active.image = reply.image;
                        active.size = reply.size;
                        active.treeNavigationEnabled = reply.tree_navigation_enabled;
                        active.controlNavigationEnabled = reply.control_navigation_enabled;
                        active.splitNavigationEnabled = reply.split_navigation_enabled;
                        active.menuOpen = reply.menu_open;
                        active.cursor = reply.cursor;
                        active.imeArea = reply.ime_area;
                        active.windows = reply.windows;
                        active.window = reply.window ?? null;
                        active.windowMinimized = reply.window_minimized;
                        if (session === active && !disposed) scheduleAnimation(active, reply.next_frame_ms);
                    } else {
                        const command = message.type === 'scale'
                            ? `scale\t${message.scale}`
                            : `click\t${message.x}\t${message.y}`;
                        Object.assign(active, await active.renderer.frame(command));
                    }
                    if (message.type === 'scale') active.scale = message.scale;
                    if (!disposed && session === active) {
                        showFrame(active.image, {
                            treeNavigationEnabled: active.treeNavigationEnabled,
                            controlNavigationEnabled: active.controlNavigationEnabled,
                            splitNavigationEnabled: active.splitNavigationEnabled,
                            menuOpen: active.menuOpen,
                            cursor: active.cursor,
                            imeArea: active.imeArea,
                            windows: active.windows,
                            window: active.window,
                            windowMinimized: active.windowMinimized,
                        });
                    }
                } catch (error) {
                    if (!disposed && session === active) {
                        session = undefined;
                        void panel.webview.postMessage({ status: 'Preview unavailable', phase: 'error', error: error.message });
                    }
                }
            });
        }
        const affected = uri => ready && uri.scheme === 'file' && (
            uri.toString() === document.uri.toString()
            || (dependencies ? dependencies.has(canonical(uri.fsPath)) : path.extname(uri.fsPath) === '.xr')
        );
        const watcher = vscode.workspace.createFileSystemWatcher('**/*.{xr,png,svg,gif,ttf,otf,ttc,otc,toml,ftl}');
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
                    if (Number.isFinite(message.scale)) scale = Math.min(4, Math.max(1, message.scale));
                    ready = true;
                    queue.refresh(0);
                } else if (message.type === 'scale' && Number.isFinite(message.scale)) {
                    scale = Math.min(4, Math.max(1, message.scale));
                    updateScale();
                } else if (message.type === 'select' && typeof message.component === 'string') {
                    component = message.component;
                    queue.refresh(0);
                } else if (['click', 'wheel', 'edit', 'composition', 'escape', 'pointer', 'delay_key', 'key', 'context_menu', 'hover', 'leave', 'select_window', 'restore_window', 'file_drop'].includes(message.type)) {
                    interact(message);
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
            clearTimeout(animationTimer);
            if (current?.panel === panel) current = undefined;
            for (const event of events) event.dispose();
            void Promise.allSettled([queue.dispose(), session?.renderer.stop(), interpreter?.stop(), starting?.stop()])
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
