const fs = require('node:fs');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { promisify } = require('node:util');

const execute = promisify(execFile);

function findManifest(filename) {
    let directory = path.dirname(path.resolve(filename));
    while (true) {
        const manifest = path.join(directory, 'crust.toml');
        try {
            if (fs.statSync(manifest).isFile()) return manifest;
        } catch (error) {
            if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
        }
        const parent = path.dirname(directory);
        if (parent === directory) return undefined;
        directory = parent;
    }
}

async function readMetadata(compiler, manifest, signal) {
    let stdout;
    try {
        ({ stdout } = await execute(compiler, ['metadata', '--project', manifest], {
            cwd: path.dirname(manifest), encoding: 'utf8', windowsHide: true,
            maxBuffer: 16 * 1024 * 1024, signal
        }));
    } catch (error) {
        if (error.code === 'ABORT_ERR') throw error;
        throw new Error(error.stderr?.trim() || error.message);
    }
    let metadata;
    try { metadata = JSON.parse(stdout); }
    catch { throw new Error('The Crust compiler returned invalid project metadata.'); }
    if (!metadata || metadata.formatVersion !== 1 || typeof metadata.name !== 'string'
        || !Array.isArray(metadata.targets)
        || !metadata.targets.every(target => target && typeof target.name === 'string' && typeof target.kind === 'string')) {
        throw new Error('Unsupported Crust project metadata. Update the compiler and extension together.');
    }
    return metadata;
}

function commandArguments(command, manifest, target) {
    if (!['build', 'check', 'run'].includes(command)) throw new Error(`Unknown Crust task command: ${command}`);
    const args = [command, '--project', manifest];
    if (target) args.push('--target', target);
    return args;
}

module.exports = { findManifest, readMetadata, commandArguments };
