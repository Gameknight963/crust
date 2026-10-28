const fs = require('node:fs');
const path = require('node:path');

function adapterType(vscode, resource, platform = process.platform) {
    const configured = vscode.workspace.getConfiguration('crust', resource).get('debugger.adapter', 'auto');
    const cpp = !!vscode.extensions.getExtension('ms-vscode.cpptools');
    const lldb = !!vscode.extensions.getExtension('vadimcn.vscode-lldb');
    if (configured === 'auto') {
        if (platform === 'win32' && cpp) return 'cppvsdbg';
        if (lldb) return 'lldb';
        if (cpp) return 'cppdbg';
        throw new Error('Install the C/C++ or CodeLLDB extension to debug Crust.');
    }
    if (!['cppvsdbg', 'cppdbg', 'lldb'].includes(configured)) throw new Error('Unknown Crust debugger adapter.');
    if (configured === 'cppvsdbg' && platform !== 'win32') throw new Error('cppvsdbg requires Windows.');
    if (configured === 'lldb' ? !lldb : !cpp) {
        throw new Error(`Install ${configured === 'lldb' ? 'CodeLLDB' : 'the C/C++ extension'} for ${configured}.`);
    }
    return configured;
}

function runtimeEnvironment(directories, overrides, platform = process.platform, inherited = process.env) {
    const environment = { ...overrides };
    const variable = platform === 'win32' ? 'PATH' : platform === 'darwin' ? 'DYLD_LIBRARY_PATH' : 'LD_LIBRARY_PATH';
    const equal = key => platform === 'win32' ? key.toUpperCase() === variable : key === variable;
    const key = Object.keys(environment).find(equal) || Object.keys(inherited).find(equal) || variable;
    if (directories.length) {
        const previous = environment[key] ?? inherited[key] ?? '';
        environment[key] = [...directories, previous].filter(Boolean).join(platform === 'win32' ? ';' : ':');
    }
    return environment;
}

function launchConfiguration(vscode, manifest, target, type) {
    if (target.kind !== 'executable') throw new Error('Select an executable target to debug.');
    if (!target.debug) throw new Error(`Enable native.debug in crust.toml for target '${target.name}'.`);
    const settings = vscode.workspace.getConfiguration('crust', manifest);
    const args = settings.get('debugger.arguments', []);
    const overrides = settings.get('debugger.environment', {});
    if (!Array.isArray(args) || !args.every(value => typeof value === 'string')) {
        throw new Error('crust.debugger.arguments must be an array of strings.');
    }
    if (!overrides || Array.isArray(overrides) || typeof overrides !== 'object'
        || !Object.values(overrides).every(value => typeof value === 'string')) {
        throw new Error('crust.debugger.environment must contain string values.');
    }
    const cwd = path.resolve(path.dirname(manifest.fsPath), settings.get('debugger.workingDirectory', '') || '.');
    if (!fs.statSync(cwd).isDirectory()) throw new Error(`Debug working directory is not a directory: ${cwd}`);
    const environment = runtimeEnvironment(target.runtimePaths || [], overrides);
    const config = { name: `Crust: ${target.name}`, type, request: 'launch', program: target.output, cwd, args };
    const stop = settings.get('debugger.stopAtEntry', false);
    if (type === 'lldb') return { ...config, env: environment, stopOnEntry: stop, terminal: 'integrated' };
    config.environment = Object.entries(environment).map(([name, value]) => ({ name, value }));
    config.stopAtEntry = stop;
    if (type === 'cppdbg') {
        config.MIMode = process.platform === 'darwin' ? 'lldb' : 'gdb';
        const debuggerPath = settings.get('debugger.miDebuggerPath', '');
        if (debuggerPath) config.miDebuggerPath = debuggerPath;
        config.externalConsole = false;
    } else {
        config.console = 'integratedTerminal';
    }
    return config;
}

async function buildBeforeDebug(vscode, task) {
    let execution;
    const early = [];
    let finish;
    const completed = new Promise(resolve => { finish = resolve; });
    const accept = event => {
        if (!execution) early.push(event);
        else if (event.execution === execution) finish(event.exitCode);
    };
    const subscription = vscode.tasks.onDidEndTaskProcess(accept);
    const ended = vscode.tasks.onDidEndTask(event => accept({ execution: event.execution }));
    try {
        execution = await vscode.tasks.executeTask(task);
        for (const event of early) {
            if (event.execution === execution) finish(event.exitCode);
        }
        const code = await completed;
        if (code !== 0) throw new Error('The Crust build failed or was cancelled.');
    } finally {
        subscription.dispose();
        ended.dispose();
    }
}

module.exports = { adapterType, runtimeEnvironment, launchConfiguration, buildBeforeDebug };
