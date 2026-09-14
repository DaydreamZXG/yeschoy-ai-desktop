import json
import tempfile
import unittest
from pathlib import Path
import publish


class HealthCompatibility(unittest.TestCase):
    def test_preserves_update_channels_on_publish_and_disable(self):
        for channels in [None, {"official": True, "partner": True}, {"official": False, "partner": True}]:
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder).resolve()
                health = dict(publish.HEALTH)
                if channels is not None:
                    health.update(updateChannels=channels, updatesPublished=any(channels.values()))
                (root / "health.json").write_text(json.dumps(health))
                for artifacts in [[{}], []]:
                    publish.reconcile_health(root, {"artifacts": artifacts})
                    expected = {**health, "thirdPartyInstallersPublished": bool(artifacts)}
                    self.assertEqual(json.loads((root / "health.json").read_text()), expected)

    def test_rejects_invalid_channels(self):
        for channels in [{"official": "true", "partner": False}, {"official": True}, {"official": True, "partner": False, "other": True}]:
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder).resolve()
                (root / "health.json").write_text(json.dumps({**publish.HEALTH, "updatesPublished": True, "updateChannels": channels}))
                with self.assertRaises(publish.SyncError):
                    publish.public_root(root)


if __name__ == "__main__":
    unittest.main()
