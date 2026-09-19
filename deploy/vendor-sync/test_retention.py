import json
import os
from pathlib import Path
import tempfile
import unittest

import publish
import sync


def candidate(sha256: str, suffix: str = "zip") -> dict:
    return {
        "path": f"artifacts/{sha256}.{suffix}",
        "sha256": sha256,
        "metadata": {},
        "package": {},
        "stagedAt": "2026-09-19T00:00:00+00:00",
    }


class Retention(unittest.TestCase):
    def test_private_cache_keeps_current_and_one_previous_per_source(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            artifacts = root / "artifacts"
            artifacts.mkdir(mode=0o700)
            current, previous, other, stale = (char * 64 for char in "abcd")
            for sha256 in (current, previous, other, stale):
                (artifacts / f"{sha256}.zip").write_bytes(sha256.encode())
                (artifacts / f"{sha256}.json").write_text(
                    json.dumps(candidate(sha256)), encoding="utf-8"
                )
            catalog = {
                "schemaVersion": 1,
                "sources": {
                    "one": {
                        "candidate": candidate(current),
                        "previousCandidate": candidate(previous),
                    },
                    "two": {"candidate": candidate(other)},
                },
            }

            result = sync.prune_private_cache(root, catalog)

            self.assertEqual(result["filesDeleted"], 2)
            self.assertFalse((artifacts / f"{stale}.zip").exists())
            self.assertFalse((artifacts / f"{stale}.json").exists())
            for sha256 in (current, previous, other):
                self.assertTrue((artifacts / f"{sha256}.zip").is_file())
                self.assertTrue((artifacts / f"{sha256}.json").is_file())

    def test_public_history_keeps_catalog_target_and_newest_previous(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            source_id = "claude-macos-universal"
            directory = root / "apps" / source_id
            directory.mkdir(parents=True)
            current, previous, stale = (char * 64 for char in "abc")
            for index, sha256 in enumerate((stale, previous, current), start=1):
                path = directory / f"{sha256}.zip"
                path.write_bytes(sha256.encode())
                os.utime(path, ns=(index, index))
            configured = {source_id: {"format": "zip"}}
            catalog = {"artifacts": [{"sourceId": source_id, "sha256": current}]}

            result = publish.prune_public_history_locked(root, configured, catalog)

            self.assertEqual(result["filesDeleted"], 1)
            self.assertFalse((directory / f"{stale}.zip").exists())
            self.assertTrue((directory / f"{previous}.zip").is_file())
            self.assertTrue((directory / f"{current}.zip").is_file())

    def test_budget_is_six_gibibytes(self):
        value = (Path(__file__).with_name("budget.env")).read_text(encoding="utf-8")
        self.assertIn("CACHE_BUDGET_BYTES=6442450944", value)


if __name__ == "__main__":
    unittest.main()
