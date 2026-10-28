const fs = require('node:fs');
const path = require('node:path');

const repository = path.resolve(__dirname, '..', '..', '..');
const compiler = process.env.CRUST_TEST_COMPILER || path.join(repository, 'target', 'debug', process.platform === 'win32' ? 'crust.exe' : 'crust');

function fixture(targets = [['app', 'executable'], ['math', 'static-library']], debug = false) {
    fs.mkdirSync(path.join(repository, 'target'), { recursive: true });
    const root = fs.mkdtempSync(path.join(repository, 'target', 'vscode task ü & '));
    let manifest = '[project]\nname="test-project"\n';
    for (const [name, kind] of targets) {
        manifest += `[[targets]]\nname="${name}"\nkind="${kind}"\nentry="${name}.cr"\n`;
        if (debug) manifest += '[targets.native]\ndebug=true\n';
        fs.writeFileSync(path.join(root, `${name}.cr`), kind === 'executable' ? 'int main() { return 0; }' : 'extern int answer() { return 42; }');
    }
    const file = path.join(root, 'crust.toml');
    fs.writeFileSync(file, manifest);
    return { root, manifest: file };
}

function host(projects) {
    const handlers = new Map();
    const executed = [];
    const errors = [];
    const logs = [];
    const choices = [];
    const debugged = [];
    const taskListeners = new Set();
    const taskEndListeners = new Set();
    const installed = new Set(['ms-vscode.cpptools', 'vadimcn.vscode-lldb']);
    const uri = filename => ({ scheme: 'file', fsPath: path.resolve(filename) });
    const folders = projects.map(project => ({ uri: uri(project.root), name: path.basename(project.root), index: 0 }));
    const config = { 'compiler.path': compiler, 'server.path': '' };
    const vscode = {
        ExtensionMode: { Development: 2 },
        Uri: { file: uri },
        TaskScope: { Global: 1, Workspace: 2 },
        TaskGroup: { Build: { id: 'build' } },
        TaskRevealKind: { Always: 1 }, TaskPanelKind: { Dedicated: 2 },
        ProcessExecution: class { constructor(process, args, options) { Object.assign(this, { process, args, options }); } },
        Task: class {
            constructor(definition, scope, name, source, execution, problemMatchers) {
                if (scope === 1) throw new Error('Global tasks are unsupported by VS Code');
                Object.assign(this, { definition, scope, name, source, execution, problemMatchers });
            }
        },
        workspace: {
            saveAll: async () => true,
            workspaceFolders: folders,
            getConfiguration: () => ({ get: (key, fallback) => config[key] ?? fallback }),
            getWorkspaceFolder: resource => folders.find(folder => resource.fsPath.startsWith(folder.uri.fsPath + path.sep)),
            findFiles: async () => projects.map(project => uri(project.manifest)),
            asRelativePath: resource => resource.fsPath
        },
        window: {
            activeTextEditor: projects.length ? { document: { uri: uri(path.join(projects[0].root, 'app.cr')) } } : undefined,
            createOutputChannel: () => ({ appendLine: line => logs.push(line), dispose() {} }),
            showErrorMessage: async message => errors.push(message),
            showQuickPick: async items => {
                const choice = choices.length ? choices.shift() : 0;
                return choice === null ? undefined : typeof choice === 'number' ? items[choice] : items.find(item => item.label === choice);
            }
        },
        tasks: {
            registerTaskProvider: () => ({ dispose() {} }),
            onDidEndTaskProcess: handler => { taskListeners.add(handler); return { dispose() { taskListeners.delete(handler); } }; },
            onDidEndTask: handler => { taskEndListeners.add(handler); return { dispose() { taskEndListeners.delete(handler); } }; },
            executeTask: async task => {
                executed.push(task);
                const execution = { task };
                queueMicrotask(() => { for (const listener of taskListeners) listener({ execution, exitCode: 0 }); });
                return execution;
            }
        },
        extensions: { getExtension: name => installed.has(name) ? {} : undefined },
        debug: { startDebugging: async (folder, configuration) => { debugged.push({ folder, configuration }); return true; } },
        commands: { registerCommand: (name, handler) => { handlers.set(name, handler); return { dispose() {} }; } }
    };
    const context = { subscriptions: [], extensionMode: 1, extensionPath: path.join(repository, 'editors', 'vscode') };
    return { vscode, context, config, handlers, executed, errors, logs, choices, folders, uri, debugged, taskListeners, taskEndListeners, installed };
}

const token = { isCancellationRequested: false, onCancellationRequested: () => ({ dispose() {} }) };
module.exports = { repository, compiler, fixture, host, token };
