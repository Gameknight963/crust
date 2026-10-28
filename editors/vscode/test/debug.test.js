const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { fixture, host, compiler } = require('./helpers');
const { registerTasks } = require('../src/tasks');
const { adapterType, runtimeEnvironment, launchConfiguration, buildBeforeDebug } = require('../src/debug');

test('adapter selection uses installed extensions and validates explicit choices', () => {
    const api = host([]);
    assert.equal(adapterType(api.vscode, undefined, 'win32'), 'cppvsdbg');
    assert.equal(adapterType(api.vscode, undefined, 'darwin'), 'lldb');
    api.installed.delete('vadimcn.vscode-lldb');
    assert.equal(adapterType(api.vscode, undefined, 'linux'), 'cppdbg');
    api.config['debugger.adapter'] = 'lldb';
    assert.throws(() => adapterType(api.vscode), /Install CodeLLDB/);
    api.config['debugger.adapter'] = 'cppvsdbg';
    assert.throws(() => adapterType(api.vscode, undefined, 'linux'), /requires Windows/);
    api.config['debugger.adapter'] = 'auto';
    api.installed.clear();
    assert.throws(() => adapterType(api.vscode), /Install/);
});

test('runtime search paths preserve configured and inherited environments', () => {
    assert.deepEqual(runtimeEnvironment(['one', 'two'], { Path: 'custom', MODE: 'test' }, 'win32', { PATH: 'inherited' }),
        { Path: 'one;two;custom', MODE: 'test' });
    assert.deepEqual(runtimeEnvironment(['native'], {}, 'linux', { LD_LIBRARY_PATH: 'existing' }),
        { LD_LIBRARY_PATH: 'native:existing' });
    assert.deepEqual(runtimeEnvironment(['native'], {}, 'darwin', {}), { DYLD_LIBRARY_PATH: 'native' });
    assert.deepEqual(runtimeEnvironment([], { MODE: 'test' }, 'linux', {}), { MODE: 'test' });
});

test('launch configuration carries arguments and adapter-specific environment settings', () => {
    const project = fixture();
    const api = host([project]);
    api.config['debugger.arguments'] = ['two words', 'a"b', '$value'];
    api.config['debugger.environment'] = { MODE: 'test' };
    api.config['debugger.stopAtEntry'] = true;
    const target = { name: 'app', kind: 'executable', debug: true, output: path.join(project.root, 'app'), runtimePaths: [] };
    const manifest = api.uri(project.manifest);
    const cpp = launchConfiguration(api.vscode, manifest, target, 'cppvsdbg');
    assert.deepEqual(cpp.args, api.config['debugger.arguments']);
    assert.deepEqual(cpp.environment, [{ name: 'MODE', value: 'test' }]);
    assert.equal(cpp.cwd, project.root);
    assert.equal(cpp.stopAtEntry, true);
    const lldb = launchConfiguration(api.vscode, manifest, target, 'lldb');
    assert.deepEqual(lldb.env, { MODE: 'test' });
    assert.equal(lldb.stopOnEntry, true);
    assert.equal(lldb.environment, undefined);
    target.debug = false;
    assert.throws(() => launchConfiguration(api.vscode, manifest, target, 'lldb'), /native.debug/);
});

test('build completion handles early exits and ignores unrelated task events', async () => {
    const api = host([]);
    await buildBeforeDebug(api.vscode, {});
    assert.equal(api.taskListeners.size, 0);
    assert.equal(api.taskEndListeners.size, 0);
    const execution = {};
    api.vscode.tasks.executeTask = async () => {
        for (const listener of api.taskListeners) listener({ execution: {}, exitCode: 0 });
        queueMicrotask(() => { for (const listener of api.taskListeners) listener({ execution, exitCode: 1 }); });
        return execution;
    };
    await assert.rejects(buildBeforeDebug(api.vscode, {}), /failed or was cancelled/);
    assert.equal(api.taskListeners.size, 0);
    api.vscode.tasks.executeTask = async () => { throw new Error('cannot start'); };
    await assert.rejects(buildBeforeDebug(api.vscode, {}), /cannot start/);
    assert.equal(api.taskListeners.size, 0);
    api.vscode.tasks.executeTask = async () => {
        queueMicrotask(() => { for (const listener of api.taskEndListeners) listener({ execution }); });
        return execution;
    };
    await assert.rejects(buildBeforeDebug(api.vscode, {}), /cancelled/);
    assert.equal(api.taskEndListeners.size, 0);
});

test('debug command saves and builds a real executable before starting the debugger', async () => {
    const project = fixture([['app', 'executable']], true);
    const api = host([project]);
    let saved = false;
    api.vscode.workspace.saveAll = async () => { saved = true; return true; };
    api.vscode.tasks.executeTask = async task => {
        assert.equal(saved, true);
        api.executed.push(task);
        await promisify(execFile)(compiler, task.execution.args, { cwd: task.execution.options.cwd, windowsHide: true });
        const execution = { task };
        queueMicrotask(() => { for (const listener of api.taskListeners) listener({ execution, exitCode: 0 }); });
        return execution;
    };
    registerTasks(api.vscode, api.context);
    await api.handlers.get('crust.debugProject')();
    assert.deepEqual(api.errors, []);
    assert.equal(api.debugged.length, 1);
    assert.deepEqual(api.executed[0].execution.args, ['build', '--project', project.manifest, '--target', 'app']);
    assert.equal(fs.statSync(api.debugged[0].configuration.program).isFile(), true);
    assert.equal(api.debugged[0].folder, api.folders[0]);
});

test('debug commands stop on missing symbols, cancelled saves, and failed builds', async () => {
    const project = fixture([['app', 'executable']]);
    const api = host([project]);
    registerTasks(api.vscode, api.context);
    await api.handlers.get('crust.debugProject')();
    assert.match(api.errors.pop(), /native.debug/);
    fs.appendFileSync(project.manifest, '[targets.native]\ndebug=true\n');
    api.vscode.workspace.saveAll = async () => false;
    await api.handlers.get('crust.debugProject')();
    assert.equal(api.executed.length, 0);
    api.vscode.workspace.saveAll = async () => true;
    api.vscode.tasks.executeTask = async () => { throw new Error('failed build'); };
    await api.handlers.get('crust.debugProject')();
    assert.match(api.errors.pop(), /failed build/);
    assert.equal(api.debugged.length, 0);
    assert.equal(api.taskListeners.size, 0);
});
