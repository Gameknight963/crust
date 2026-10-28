"""Build and verify a compiler archive for the host platform."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import uuid
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def command(arguments, cwd=ROOT):
    return subprocess.check_output(arguments, cwd=cwd, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use cached Cargo dependencies")
    arguments = parser.parse_args()
    rustc = command(["rustc", "-vV"])
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    build = ["cargo", "build", "--release", "--locked", "--bins", "--target", host]
    if arguments.offline:
        build.append("--offline")
    subprocess.run(build, cwd=ROOT, check=True)
    metadata = json.loads(command(["cargo", "metadata", "--offline", "--no-deps", "--format-version", "1"]))
    target = Path(metadata["target_directory"])
    output = target / "packages"
    output.mkdir(parents=True, exist_ok=True)
    name = f"crust-{version}-{host}"
    suffix = ".exe" if os.name == "nt" else ""
    files = {}
    for binary in ["crust", "crust-lsp"]:
        path = target / host / "release" / f"{binary}{suffix}"
        subprocess.run([str(path), "--help"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        files[f"bin/{path.name}"] = path
    tracked = subprocess.check_output([
        "git", "ls-files", "-z", "--", "Cargo.toml", "Cargo.lock", "README.md",
        "src", "lib", "docs", "examples", "editors", "tests", "tools",
    ], cwd=ROOT)
    for entry in tracked.decode("utf-8").split("\0"):
        if entry:
            files[entry] = ROOT / entry
    for entry in ["tools/package.py", "docs/installation.md"]:
        files[entry] = ROOT / entry
    manifest = {
        "formatVersion": 1,
        "version": version,
        "target": host,
        "revision": command(["git", "rev-parse", "HEAD"]),
        "dirty": bool(command(["git", "status", "--porcelain"])),
        "rustc": rustc.splitlines()[0],
        "files": {entry: hashlib.sha256(path.read_bytes()).hexdigest() for entry, path in sorted(files.items())},
    }
    manifest_bytes = (json.dumps(manifest, indent=2) + "\n").encode("utf-8")
    extension = ".zip" if os.name == "nt" else ".tar.gz"
    archive = output / (name + extension)
    partial = archive.with_name(archive.name + ".partial")
    if os.name == "nt":
        with zipfile.ZipFile(partial, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for entry, path in sorted(files.items()):
                bundle.write(path, f"{name}/{entry}")
            bundle.writestr(f"{name}/manifest.json", manifest_bytes)
    else:
        import io
        with tarfile.open(partial, "w:gz") as bundle:
            for entry, path in sorted(files.items()):
                bundle.add(path, arcname=f"{name}/{entry}", recursive=False)
            info = tarfile.TarInfo(f"{name}/manifest.json")
            info.size = len(manifest_bytes)
            info.mode = 0o644
            bundle.addfile(info, io.BytesIO(manifest_bytes))
    # Verify and execute the archived files before replacing an earlier package.
    destination = (output / ("smoke-" + uuid.uuid4().hex)).resolve()
    destination.mkdir()
    if not destination.is_relative_to(output.resolve()):
        raise RuntimeError("Smoke-test directory must stay inside the package output directory")
    if os.name == "nt":
        with zipfile.ZipFile(partial) as bundle:
            bundle.extractall(destination)
    else:
        with tarfile.open(partial) as bundle:
            bundle.extractall(destination, filter="data")
    extracted = destination / name
    archived_manifest = json.loads((extracted / "manifest.json").read_text(encoding="utf-8"))
    for entry, digest in archived_manifest["files"].items():
        if hashlib.sha256((extracted / entry).read_bytes()).hexdigest() != digest:
            raise RuntimeError(f"Archive checksum mismatch: {entry}")
    compiler = extracted / "bin" / f"crust{suffix}"
    server = extracted / "bin" / f"crust-lsp{suffix}"
    subprocess.run([str(server), "--help"], cwd=extracted, check=True, stdout=subprocess.DEVNULL)
    text = command([str(compiler), "run", "examples/text.cr"], cwd=extracted)
    if text != "Hello, Crust!\nCrust":
        raise RuntimeError(f"Unexpected text example output: {text!r}")
    subprocess.run([str(compiler), "run", "--project", "examples/file_project"], cwd=extracted,
                   check=True, stdout=subprocess.DEVNULL)
    os.replace(partial, archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(archive)


if __name__ == "__main__":
    main()
