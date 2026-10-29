"""Fetch the pinned SQLite amalgamation used by this example."""

import argparse
import hashlib
from pathlib import Path
import urllib.request
import zipfile


VERSION = "3500400"
URL = f"https://www.sqlite.org/2025/sqlite-amalgamation-{VERSION}.zip"
SHA256 = "1d3049dd0f830a025a53105fc79fd2ab9431aea99e137809d064d8ee8356b032"
DIRECTORY = Path(__file__).resolve().parent / "target" / "sqlite"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use the cached archive")
    options = parser.parse_args()
    DIRECTORY.mkdir(parents=True, exist_ok=True)
    archive = DIRECTORY / "sqlite.zip"
    if not archive.exists():
        if options.offline:
            parser.error("Run prepare.py once online to cache the SQLite archive")
        with urllib.request.urlopen(URL, timeout=60) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != SHA256:
            raise RuntimeError("SQLite archive checksum mismatch")
        archive.write_bytes(data)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise RuntimeError(f"SQLite archive checksum mismatch: {archive}")
    with zipfile.ZipFile(archive) as bundle:
        for name in ["sqlite3.c", "sqlite3.h"]:
            (DIRECTORY / name).write_bytes(bundle.read(f"sqlite-amalgamation-{VERSION}/{name}"))
    print(f"Prepared SQLite 3.50.4 in {DIRECTORY}")


if __name__ == "__main__":
    main()
