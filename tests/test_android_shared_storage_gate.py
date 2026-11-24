from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class AndroidSharedStorageGateTests(unittest.TestCase):
    def test_launcher_requires_shared_storage_before_starting_game(self):
        source = (ROOT / "packaging/android/java/io/github/juanluci/starling/LauncherActivity.java").read_text(encoding="utf-8")
        play = source[source.index("private void play()") : source.index("private boolean hasPublicStorageAccess()")]
        self.assertIn("hasPublicStorageAccess()", play)
        self.assertIn("openPublicStorageSettings()", play)
        self.assertIn("startGame()", play)
        self.assertLess(play.index("if (hasPublicStorageAccess())"), play.index("startGame()"))

    def test_android_11_uses_all_files_access(self):
        source = (ROOT / "packaging/android/java/io/github/juanluci/starling/LauncherActivity.java").read_text(encoding="utf-8")
        self.assertIn("Environment.isExternalStorageManager()", source)
        self.assertIn("Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION", source)
        self.assertIn("/storage/emulated/0/StarlingExtras", source)

    def test_private_fallback_is_logged(self):
        source = (ROOT / "crates/horizon-client/src/android.rs").read_text(encoding="utf-8")
        self.assertIn("All files access is not enabled for this app", source)


if __name__ == "__main__":
    unittest.main()
