#!/usr/bin/env python3
"""Restore a quiesced pair into a NEW private directory; never start services."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import uuid


def check(connection, required):
    if connection.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
        raise ValueError("SQLite integrity check failed")
    if connection.execute("PRAGMA foreign_key_check").fetchone() is not None:
        raise ValueError("SQLite foreign key check failed")
    tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if not required <= tables:
        raise ValueError("snapshot is missing required final-schema tables")


def restore(source, destination):
    source = source.resolve(strict=True)
    # Exclusive directory creation prevents replacing any existing restore/data.
    destination.mkdir(mode=0o700)
    marker = destination / "INCOMPLETE"
    marker.write_text("Do not start services against this directory.\n")
    requirements = {
        "daemon.db": {"settled_payments", "ledger_entries", "event_log", "feed_attempts",
                      "strike_receive_requests", "strike_inbox", "strike_recovery_scan",
                      "message_outbox", "message_cursor", "overlay_identity"},
        "gateway.db": {"feeder_requests", "feeder_request_events", "feeder_refusals", "weather_high_water"},
    }
    manifest = {"format": 1, "writers_stopped_required": True, "files": {}}
    for name, required in requirements.items():
        original = source / name
        if not original.is_file() or original.is_symlink():
            raise ValueError("snapshot must contain regular daemon.db and gateway.db files")
        with sqlite3.connect(original.as_uri() + "?mode=ro", uri=True) as reader:
            check(reader, required)
            with sqlite3.connect(destination / name) as writer:
                reader.backup(writer)
                check(writer, required)
                if name == "daemon.db":
                    # An older backup may reuse sequences seen by connected browsers.
                    new_stream = str(uuid.uuid4())
                    writer.execute("INSERT INTO overlay_identity(singleton,stream_id) VALUES(1,?) "
                                   "ON CONFLICT(singleton) DO UPDATE SET stream_id=excluded.stream_id", (new_stream,))
                    writer.commit()
                    manifest["overlay_stream_id"] = new_stream
                writer.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        # Connections must be closed before hashes or a service startup.
        reader.close()
        writer.close()
        path = destination / name
        os.chmod(path, 0o600)
        with path.open("rb") as handle:
            digest = hashlib.sha256()
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(block)
            manifest["files"][name] = digest.hexdigest()
    manifest_path = destination / "RESTORE.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    os.chmod(manifest_path, 0o600)
    # fsync the final files and directory before removing the incomplete marker.
    for name in [*requirements, "RESTORE.json"]:
        with (destination / name).open("rb") as handle:
            os.fsync(handle.fileno())
    marker.unlink()
    directory_fd = os.open(destination, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    parent_fd = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(parent_fd)
    finally:
        os.close(parent_fd)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-directory", required=True, type=Path)
    parser.add_argument("--destination-directory", required=True, type=Path)
    parser.add_argument("--writers-stopped", required=True, action="store_true",
                        help="attest all daemon/gateway/signer workers for this snapshot pair are stopped")
    args = parser.parse_args()
    os.umask(0o077)
    try:
        manifest = restore(args.source_directory, args.destination_directory)
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Restore failed ({type(error).__name__}); destination is not accepted.\n")
    print(json.dumps(manifest, sort_keys=True))


if __name__ == "__main__":
    main()
