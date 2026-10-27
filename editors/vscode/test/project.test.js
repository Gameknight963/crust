const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { fixture, compiler } = require('./helpers');
const { findManifest, readMetadata, commandArguments } = require('../src/project');

test('project discovery selects the closest ancestor manifest', () => {
    const project = fixture();
    assert.equal(findManifest(path.join(project.root, 'nested', 'app.cr')), project.manifest);
    fs.mkdirSync(path.join(project.root, 'nested'));
    fs.writeFileSync(path.join(project.root, 'nested', 'crust.toml'), '[project]');
    assert.equal(findManifest(path.join(project.root, 'nested', 'app.cr')), path.join(project.root, 'nested', 'crust.toml'));
});

test('metadata preserves Unicode and shell characters in paths and tolerates source errors', async () => {
    const project = fixture();
    fs.writeFileSync(path.join(project.root, 'app.cr'), 'unfinished source');
    const info = await readMetadata(compiler, project.manifest);
    assert.equal(info.name, 'test-project');
    assert.deepEqual(info.targets.map(target => target.kind), ['executable', 'static-library']);
    assert.equal(fs.existsSync(path.join(project.root, 'target')), false);
    assert.deepEqual(commandArguments('run', project.manifest, 'app'), ['run', '--project', project.manifest, '--target', 'app']);
    assert.throws(() => commandArguments('unknown', project.manifest), /Unknown Crust task/);
});

test('metadata errors and cancellation propagate without hiding compiler messages', async () => {
    const project = fixture();
    const script = path.join(project.root, 'metadata');
    fs.writeFileSync(script, 'process.stderr.write("broken manifest"); process.exit(1);');
    await assert.rejects(readMetadata(process.execPath, project.manifest), /broken manifest/);
    for (const response of ['not JSON', 'null', '{"formatVersion":2}', '{"formatVersion":1,"name":"test","targets":[null]}']) {
        fs.writeFileSync(script, `process.stdout.write(${JSON.stringify(response)});`);
        await assert.rejects(readMetadata(process.execPath, project.manifest), /project metadata/);
    }
    fs.writeFileSync(script, 'setTimeout(() => {}, 10000);');
    const controller = new AbortController();
    const request = readMetadata(process.execPath, project.manifest, controller.signal);
    controller.abort();
    await assert.rejects(request, error => error.code === 'ABORT_ERR');
});

test('compiler errors produce Problems-panel paths that point to the original file', async () => {
    const project = fixture();
    const source = path.join(project.root, 'app.cr');
    fs.writeFileSync(source, 'int main() { return missing; }');
    const manifest = require('../package.json');
    const matcher = new RegExp(manifest.contributes.problemMatchers[0].pattern.regexp);
    await assert.rejects(promisify(execFile)(compiler, ['check', '--project', project.manifest, '--target', 'app']), error => {
        const match = error.stderr.trim().match(matcher);
        assert.ok(match, error.stderr);
        assert.equal(match[1], fs.realpathSync(source));
        assert.equal(match[2], '1');
        assert.match(match[4], /missing/);
        return true;
    });
});
