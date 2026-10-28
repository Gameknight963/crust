const fs = require('node:fs');
const path = require('node:path');
const { findManifest, readMetadata, commandArguments } = require('./project');
const { adapterType, launchConfiguration, buildBeforeDebug } = require('./debug');

function registerTasks(vscode, context) {
    const output = vscode.window.createOutputChannel('Crust builds');
    context.subscriptions.push(output);
    let debugPending = false;

    function compilerPath(resource) {
        const config = vscode.workspace.getConfiguration('crust', resource);
        const configured = config.get('compiler.path', '');
        if (configured) return configured;
        const filename = process.platform === 'win32' ? 'crust.exe' : 'crust';
        const server = config.get('server.path', '');
        if (path.isAbsolute(server)) {
            const sibling = path.join(path.dirname(server), filename);
            if (fs.existsSync(sibling)) return sibling;
        }
        if (context.extensionMode === vscode.ExtensionMode.Development) {
            const local = path.resolve(context.extensionPath, '..', '..', 'target', 'debug', filename);
            if (fs.existsSync(local)) return local;
        }
        return 'crust';
    }

    async function metadata(manifest, token) {
        const controller = new AbortController();
        const cancellation = token?.onCancellationRequested(() => controller.abort());
        if (token?.isCancellationRequested) controller.abort();
        try {
            return await readMetadata(compilerPath(manifest), manifest.fsPath, controller.signal);
        } finally {
            cancellation?.dispose();
        }
    }

    function task(manifest, info, command, target, suppliedDefinition, suppliedName) {
        const folder = vscode.workspace.getWorkspaceFolder(manifest);
        if (!folder && !vscode.workspace.workspaceFolders?.length) {
            throw new Error('Open a workspace folder to run Crust project tasks.');
        }
        const project = folder ? path.relative(folder.uri.fsPath, manifest.fsPath) : manifest.fsPath;
        const definition = suppliedDefinition || { type: 'crust', command, project, ...(target ? { target } : {}) };
        const name = suppliedName || `${info.name}: ${command}${target ? ` ${target}` : ''}`;
        const execution = new vscode.ProcessExecution(compilerPath(manifest),
            commandArguments(command, manifest.fsPath, target), { cwd: path.dirname(manifest.fsPath) });
        const result = new vscode.Task(definition, folder || vscode.TaskScope.Workspace, name, 'Crust', execution, ['$crust']);
        if (command === 'build') result.group = vscode.TaskGroup.Build;
        result.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated };
        return result;
    }

    async function manifests() {
        return await vscode.workspace.findFiles('**/crust.toml', '**/{target,node_modules,bin,obj,.git,.vs}/**');
    }

    async function selectProject() {
        const active = vscode.window.activeTextEditor?.document.uri;
        if (active?.scheme === 'file') {
            const nearest = findManifest(active.fsPath);
            if (nearest) return vscode.Uri.file(nearest);
        }
        const projects = await manifests();
        if (projects.length === 0) throw new Error('No crust.toml was found. Open a Crust project folder.');
        if (projects.length === 1) return projects[0];
        const picked = await vscode.window.showQuickPick(projects.map(uri => ({
            label: path.basename(path.dirname(uri.fsPath)),
            description: vscode.workspace.asRelativePath(uri), uri
        })), { placeHolder: 'Select a Crust project' });
        return picked?.uri;
    }

    async function execute(command) {
        const manifest = await selectProject();
        if (!manifest) return;
        if (command === 'debug' && !await vscode.workspace.saveAll(false)) return;
        const info = await metadata(manifest);
        let target;
        if (command === 'run' || command === 'debug') {
            const executables = info.targets.filter(candidate => candidate.kind === 'executable');
            if (executables.length === 0) throw new Error('This project has no executable target.');
            if (executables.length === 1) target = executables[0].name;
            else {
                const picked = await vscode.window.showQuickPick(executables.map(candidate => ({ label: candidate.name })),
                    { placeHolder: 'Select a Crust executable target' });
                if (!picked) return;
                target = picked.label;
            }
        }
        if (command === 'debug') {
            const folder = vscode.workspace.getWorkspaceFolder(manifest) || vscode.workspace.workspaceFolders?.[0];
            if (!folder) throw new Error('Open a workspace folder to debug a Crust project.');
            const type = adapterType(vscode, manifest);
            launchConfiguration(vscode, manifest, info.targets.find(candidate => candidate.name === target), type);
            await buildBeforeDebug(vscode, task(manifest, info, 'build', target));
            const refreshed = await metadata(manifest);
            const selected = refreshed.targets.find(candidate => candidate.name === target);
            if (!selected) throw new Error(`Crust target '${target}' was removed during the build.`);
            const config = launchConfiguration(vscode, manifest, selected, type);
            if (!fs.statSync(config.program).isFile()) throw new Error('The Crust build did not produce an executable.');
            if (!await vscode.debug.startDebugging(folder, config)) throw new Error('The native debugger could not start.');
        } else {
            await vscode.tasks.executeTask(task(manifest, info, command, target));
        }
    }

    const provider = {
        async provideTasks(token) {
            const tasks = [];
            for (const manifest of await manifests()) {
                if (token.isCancellationRequested) break;
                try {
                    const info = await metadata(manifest, token);
                    tasks.push(task(manifest, info, 'build'), task(manifest, info, 'check'));
                    for (const target of info.targets.filter(candidate => candidate.kind === 'executable')) {
                        tasks.push(task(manifest, info, 'run', target.name));
                    }
                } catch (error) {
                    if (error.code !== 'ABORT_ERR') output.appendLine(`${manifest.fsPath}: ${error.message}`);
                }
            }
            return tasks;
        },
        async resolveTask(unresolved, token) {
            const definition = unresolved.definition;
            const folder = unresolved.scope;
            const project = definition.project || 'crust.toml';
            if (!folder?.uri && !path.isAbsolute(project)) return undefined;
            const manifest = vscode.Uri.file(path.isAbsolute(project) ? project : path.resolve(folder.uri.fsPath, project));
            const info = await metadata(manifest, token);
            let target = definition.target;
            if (target && !info.targets.some(candidate => candidate.name === target)) {
                throw new Error(`Unknown Crust target: ${target}`);
            }
            if (definition.command === 'run') {
                const executables = info.targets.filter(candidate => candidate.kind === 'executable');
                if (!target && executables.length === 1) target = executables[0].name;
                if (!executables.some(candidate => candidate.name === target)) {
                    throw new Error('Set target to an executable target in the Crust task.');
                }
            }
            const resolved = task(manifest, info, definition.command || 'build', target, definition, unresolved.name);
            if (unresolved.group) resolved.group = unresolved.group;
            if (unresolved.runOptions) resolved.runOptions = unresolved.runOptions;
            resolved.presentationOptions = { ...resolved.presentationOptions, ...unresolved.presentationOptions };
            return resolved;
        }
    };
    context.subscriptions.push(vscode.tasks.registerTaskProvider('crust', provider));
    for (const command of ['build', 'check', 'run', 'debug']) {
        context.subscriptions.push(vscode.commands.registerCommand(`crust.${command}Project`, async () => {
            if (command === 'debug' && debugPending) return;
            if (command === 'debug') debugPending = true;
            try { await execute(command); }
            catch (error) { await vscode.window.showErrorMessage(`Crust: ${error.message}`); }
            finally { if (command === 'debug') debugPending = false; }
        }));
    }
    return provider;
}

module.exports = { registerTasks };
