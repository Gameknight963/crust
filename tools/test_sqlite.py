"""Prepare SQLite and run the real-library integration checks."""

import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use cached SQLite and Cargo inputs")
    options = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    prepare = [sys.executable, str(root / "examples/sqlite_project/prepare.py")]
    command = ["cargo", "test", "--locked", "--test", "sqlite"]
    if options.offline:
        prepare.append("--offline")
        command.append("--offline")
    subprocess.run(prepare, cwd=root, check=True)
    environment = dict(os.environ, CRUST_TEST_SQLITE="1")
    subprocess.run(command, cwd=root, env=environment, check=True)


if __name__ == "__main__":
    main()
