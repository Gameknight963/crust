const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { fixture, host, token, compiler } = require('./helpers');
const { registerTasks } = require('../src/tasks');

test('task discovery offers build, check, and executable targets', async () => {
    const project = fixture();
    const api = host([project]);
    const provider = registerTasks(api.vscode, api.context);
    const tasks = await provider.provideTasks(token);
    assert.deepEqual(tasks.map(task => [task.definition.command, task.definition.target]), [['build', undefined], ['check', undefined], ['run', 'app']]);
    assert.equal(tasks[0].execution.process, compiler);
    assert.deepEqual(tasks[0].execution.args, ['build', '--project', project.manifest]);
    assert.equal(tasks[0].execution.options.cwd, project.root);
    assert.deepEqual(tasks[0].problemMatchers, ['$crust']);
    assert.equal(api.logs.length, 0);
});

test('run commands select an executable and cancel target selection cleanly', async () => {
    const project = fixture([['app', 'executable'], ['other', 'executable'], ['math', 'static-library']]);
    const api = host([project]);
    registerTasks(api.vscode, api.context);
    api.choices.push('other');
    await api.handlers.get('crust.runProject')();
    assert.deepEqual(api.executed[0].execution.args, ['run', '--project', project.manifest, '--target', 'other']);
    api.choices.push(null);
    await api.handlers.get('crust.runProject')();
    assert.equal(api.executed.length, 1);
    assert.equal(api.errors.length, 0);
});

test('configured tasks preserve definition identity, labels, and user task settings', async () => {
    const project = fixture();
    const api = host([project]);
    const provider = registerTasks(api.vscode, api.context);
    const unresolved = {
        definition: { type: 'crust', command: 'run', project: './crust.toml' },
        name: 'My app', scope: api.folders[0], group: { id: 'test' },
        runOptions: { instanceLimit: 2 }, presentationOptions: { focus: true }
    };
    const resolved = await provider.resolveTask(unresolved, token);
    assert.equal(resolved.definition, unresolved.definition);
    assert.equal(resolved.name, 'My app');
    assert.equal(resolved.group, unresolved.group);
    assert.equal(resolved.runOptions, unresolved.runOptions);
    assert.equal(resolved.presentationOptions.focus, true);
    assert.deepEqual(resolved.execution.args, ['run', '--project', project.manifest, '--target', 'app']);
    for (const target of ['unknown', 'math']) {
        unresolved.definition = { type: 'crust', command: 'run', target };
        await assert.rejects(provider.resolveTask(unresolved, token), /target/);
    }
});

test('project commands work for files outside open workspace folders', async () => {
    const project = fixture();
    const api = host([]);
    api.vscode.window.activeTextEditor = { document: { uri: api.uri(path.join(project.root, 'app.cr')) } };
    const provider = registerTasks(api.vscode, api.context);
    await api.handlers.get('crust.buildProject')();
    assert.equal(api.executed[0].scope, api.vscode.TaskScope.Global);
    assert.equal(api.executed[0].definition.project, project.manifest);
    const resolved = await provider.resolveTask(api.executed[0], token);
    assert.deepEqual(resolved.execution.args, ['build', '--project', project.manifest]);
});

test('commands report missing projects and libraries cannot be run', async () => {
    const empty = host([]);
    registerTasks(empty.vscode, empty.context);
    await empty.handlers.get('crust.buildProject')();
    assert.match(empty.errors[0], /No crust.toml/);
    const project = fixture([['math', 'static-library']]);
    const api = host([project]);
    registerTasks(api.vscode, api.context);
    await api.handlers.get('crust.runProject')();
    assert.match(api.errors[0], /no executable target/);
    assert.equal(api.executed.length, 0);
});

test('task discovery respects cancellation and compiler lookup uses server siblings', async () => {
    const project = fixture();
    const api = host([project]);
    api.config['compiler.path'] = '';
    api.config['server.path'] = path.join(path.dirname(compiler), process.platform === 'win32' ? 'crust-lsp.exe' : 'crust-lsp');
    const provider = registerTasks(api.vscode, api.context);
    assert.deepEqual(await provider.provideTasks({ ...token, isCancellationRequested: true }), []);
    await api.handlers.get('crust.checkProject')();
    assert.equal(api.executed[0].execution.process, compiler);
});
