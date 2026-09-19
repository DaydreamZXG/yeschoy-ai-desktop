"""Publish synchronized candidates without granting the downloader public write access."""
import json
from pathlib import Path
import sys
import publish
import sync

STATE = Path("/var/lib/private/yeschoy-vendor-sync")
PUBLIC = Path("/srv/yeschoy-download/public")
RECEIPTS = Path("/opt/yeschoy-vendor-sync/receipts")
RESULT = Path("/var/lib/yeschoy-vendor-publish/last-run.json")


def run(state=STATE, public=PUBLIC, receipts=RECEIPTS):
    events = []
    # Prevent candidate cleanup/replacement while publishing.
    with sync.locked(sync.private_root(state, create=False)):
        snapshot = publish.load(state / "catalog.json")
        for source in sync.sources():
            source_id = source["id"]
            receipt = None
            candidate = snapshot.get("sources", {}).get(source_id, {}).get("candidate", {})
            digest = candidate.get("sha256", "")
            if len(digest) == 64 and all(c in "0123456789abcdef" for c in digest):
                path = receipts / (digest + ".json")
                if path.exists():
                    receipt = path
            try:
                result = publish.publish(state, public, source_id, receipt)
                events.append(result)
            except Exception as error:
                # Keep other platforms moving; never emit arbitrary exception data.
                code = str(error) if isinstance(error, sync.SyncError) else "publisher_local_error"
                events.append({"sourceId": source_id, "published": False, "error": code})
        try:
            private_retention = sync.prune_private_cache(state, snapshot)
        except Exception as error:
            code = str(error) if isinstance(error, sync.SyncError) else "retention_local_error"
            private_retention = {"status": "failed", "error": code}
    try:
        public_retention = publish.prune_history(public)
    except Exception as error:
        code = str(error) if isinstance(error, sync.SyncError) else "retention_local_error"
        public_retention = {"status": "failed", "error": code}
    return {
        "checkedAt": sync.utc(),
        "events": events,
        "retention": {"private": private_retention, "public": public_retention},
    }


if __name__ == "__main__":
    result = run()
    publish.atomic_public_json(RESULT, result)
    print(json.dumps(result), flush=True)
    retention_ok = all(value.get("status") == "pruned" for value in result["retention"].values())
    sys.exit(0 if all(e["published"] for e in result["events"]) and retention_ok else 2)
