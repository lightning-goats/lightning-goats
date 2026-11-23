"""Export a ready operator token into a new private file without logging the token."""
import argparse
import os
import sqlite3
from pathlib import Path
from uuid import UUID


def export(database, feed, output):
    with sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True) as db:
        row = db.execute("SELECT token FROM operator_ecash WHERE feed=?", (str(UUID(feed)),)).fetchone()
    if not row or row[0] is None:
        raise ValueError("operator token is not ready to collect")
    fd = os.open(output, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        with os.fdopen(fd, "w") as private:
            private.write(row[0] + "\n")
            private.flush()
            os.fsync(private.fileno())
    except OSError:
        output.unlink(missing_ok=True)
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feed")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error("operator export requires root")
    export(Path("/var/lib/lightning-goats-project-collector/project-claims.sqlite"), args.feed, args.output)
