const vscode = require('vscode');
const path = require('node:path');
const fs = require('node:fs');
const { LanguageClient } = require('vscode-languageclient/node');
const { registerTasks } = require('./tasks');

let client;
let pending = Promise.resolve();

function serverPath(context) {
    const configured = vscode.workspace.getConfiguration('crust').get('server.path', '');
    if (configured) {
        return configured;
    }
    if (context.extensionMode === vscode.ExtensionMode.Development) {
        const filename = process.platform === 'win32' ? 'crust-lsp.exe' : 'crust-lsp';
        const local = path.resolve(context.extensionPath, '..', '..', 'target', 'debug', filename);
        if (fs.existsSync(local)) {
            return local;
        }
    }
    return 'crust-lsp';
}

function restart(context) {
    pending = pending.catch(() => {}).then(async () => {
        if (client) {
            await client.stop();
        }
        client = new LanguageClient('crust', 'Crust', {
            command: serverPath(context),
            args: ['--stdio']
        }, {
            documentSelector: [
                { scheme: 'file', language: 'crust' },
                { scheme: 'untitled', language: 'crust' }
            ]
        });
        await client.start();
    });
    return pending;
}

async function activate(context) {
    registerTasks(vscode, context);
    context.subscriptions.push(vscode.commands.registerCommand('crust.restartLanguageServer', () => restart(context)));
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(event => {
        if (event.affectsConfiguration('crust.server.path')) {
            restart(context).catch(error => vscode.window.showErrorMessage(`Crust: ${error.message}`));
        }
    }));
    await restart(context).catch(error => vscode.window.showErrorMessage(`Crust: ${error.message}`));
}

async function deactivate() {
    await pending.catch(() => {});
    if (client) {
        await client.stop();
        client = undefined;
    }
}

module.exports = { activate, deactivate };
