import contextlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import publish_pending as runner


class AutoPublish(unittest.TestCase):
    def test_failures_do_not_block_other_platforms_and_receipts_match_current_hash(self):
        digest = "a" * 64
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / (digest + ".json")).write_text("{}")
            snapshot = {"sources": {"mac": {"candidate": {"sha256": digest}}, "win": {"candidate": {"sha256": "b" * 64}}}}
            with patch.object(runner.sync, "private_root", return_value=root), patch.object(runner.sync, "locked", return_value=contextlib.nullcontext()), patch.object(runner.sync, "sources", return_value=[{"id": "mac"}, {"id": "win"}]), patch.object(runner.publish, "load", return_value=snapshot), patch.object(runner.publish, "publish", side_effect=[runner.sync.SyncError("native_verification_required"), {"published": True}]) as publish:
                result = runner.run(root, root, root)
            self.assertEqual(publish.call_args_list[0].args[3], root / (digest + ".json"))
            self.assertIsNone(publish.call_args_list[1].args[3])
            self.assertEqual(result["events"][0]["error"], "native_verification_required")
            self.assertTrue(result["events"][1]["published"])


if __name__ == "__main__":
    unittest.main()
