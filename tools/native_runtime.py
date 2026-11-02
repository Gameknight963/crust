"""Locate the libclang runtime copied into compiler archives."""
import os
from pathlib import Path
import shutil
import subprocess
import sys


def libclang_path():
    override = os.environ.get("CRUST_LIBCLANG")
    if override:
        path = Path(override).resolve(strict=True)
        if not path.is_file():
            raise RuntimeError("CRUST_LIBCLANG must identify a library file")
        return path
    candidates = []
    clang = shutil.which("clang")
    if sys.platform == "darwin":
        clang = subprocess.check_output(["xcrun", "--find", "clang"], text=True).strip()
    if clang:
        directory = Path(clang).resolve().parent
        candidates.extend((directory / "libclang.dll", directory / "../lib/libclang.dylib"))
    if os.name == "nt":
        candidates.append(Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "LLVM/bin/libclang.dll")
    for root in (Path("/usr/lib"), Path("/usr/local/lib"), Path("/lib")):
        if root.is_dir():
            candidates.extend(sorted(root.glob("llvm-*/lib/libclang.so*"), reverse=True))
            candidates.extend(sorted(root.glob("*/libclang-*.so*"), reverse=True))
            candidates.extend(sorted(root.glob("libclang.so*"), reverse=True))
    for path in candidates:
        if path.is_file():
            return path.resolve()
    raise RuntimeError("libclang runtime not found; set CRUST_LIBCLANG when building the archive")


def runtime_files():
    path = libclang_path()
    name = "libclang.dll" if os.name == "nt" else "libclang.dylib" if sys.platform == "darwin" else "libclang.so"
    files = {f"bin/{name}": path}
    if sys.platform.startswith("linux"):
        # libclang supplied by distributions commonly shares libLLVM.
        dependencies = subprocess.check_output(["ldd", str(path)], text=True)
        for line in dependencies.splitlines():
            parts = line.split()
            if len(parts) >= 3 and parts[0].startswith("libLLVM") and parts[1] == "=>":
                library = Path(parts[2])
                if not library.is_file():
                    raise RuntimeError(f"Unresolved LLVM runtime: {line}")
                files[f"bin/{parts[0]}"] = library.resolve()
    return files, name


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    args = parser.parse_args()
    args.stage.mkdir(parents=True, exist_ok=True)
    files, _ = runtime_files()
    for name, source in files.items():
        shutil.copy2(source, args.stage / Path(name).name)
