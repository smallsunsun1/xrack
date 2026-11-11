const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const { registerPreview } = require('./preview');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

let client;

function serverCommand(context) {
    const active = vscode.window.activeTextEditor?.document;
    const document = active?.languageId === 'xrack'
        ? active
        : vscode.workspace.textDocuments.find(doc => doc.languageId === 'xrack');
    const workspace = (document && vscode.workspace.getWorkspaceFolder(document.uri))
        ?? vscode.workspace.workspaceFolders?.[0];
    const root = workspace?.uri.fsPath;
    const configured = vscode.workspace.getConfiguration('xrack', document?.uri)
        .get('server.path', '').trim();
    if (configured) {
        return root && !path.isAbsolute(configured) ? path.resolve(root, configured) : configured;
    }
    const executable = process.platform === 'win32' ? 'xrack-lsp.exe' : 'xrack-lsp';
    const bundled = context.asAbsolutePath(path.join('server', executable));
    if (fs.existsSync(bundled)) {
        return bundled;
    }
    if (root) {
        const built = path.join(root, 'bazel-bin', 'crates', 'xrack-lsp', executable);
        if (fs.existsSync(built)) {
            return built;
        }
    }
    return executable;
}

async function activate(context) {
    registerPreview(context);
    const command = serverCommand(context);
    client = new LanguageClient(
        'xrack',
        'XRack',
        { command, transport: TransportKind.stdio },
        { documentSelector: [{ language: 'xrack' }] },
    );
    try {
        await client.start();
    } catch (error) {
        void vscode.window.showErrorMessage(
            `XRack could not start ${command}: ${error.message}. Build xrack-lsp or set xrack.server.path.`,
        );
    }
}

async function deactivate() {
    await client?.dispose();
}

module.exports = { activate, deactivate };
