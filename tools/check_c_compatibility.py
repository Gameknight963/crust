"""Run the native C compatibility acceptance tests with the selected host compiler."""

import argparse
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parent.parent
TESTS = (
    "bindings", "native_records", "native_integers", "calling_conventions",
    "dll_imports", "external_variables", "export_headers", "exports",
    "projects", "runtime_locations", "sqlite",
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use cached Cargo dependencies")
    parser.add_argument("--full", action="store_true", help="Include the full compiler regression suite")
    arguments = parser.parse_args()
    # These probes prevent a missing compiler from silently skipping native tests.
    for compiler in dict.fromkeys((os.environ.get("CC", "clang"), "clang")):
        subprocess.run([compiler, "--version"], cwd=ROOT, check=True)
    command = ["cargo", "test", "--locked", "--no-fail-fast"]
    if arguments.offline:
        command.append("--offline")
    if not arguments.full:
        for test in TESTS:
            command.extend(("--test", test))
    subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
