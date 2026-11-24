from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class AndroidJavaModStorageTests(unittest.TestCase):
    def read(self, relative):
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_android_root_is_public_starling_extras(self):
        source = self.read("packaging/android/java/io/github/juanluci/starling/StarlingActivity.java")
        self.assertIn('new File(Environment.getExternalStorageDirectory(), "StarlingExtras").getPath()', source)
        self.assertIn("onStorage(sharedFolder(this))", source)

    def test_android_game_home_must_be_inside_shared_root_for_java_mods(self):
        source = self.read("crates/horizon-client/src/android.rs")
        self.assertIn("pub fn shared_game_home(game_home: &Path) -> Option<PathBuf>", source)
        self.assertIn("game_home.starts_with(shared)", source)

    def test_java_bridge_scans_only_resolved_java_game_home(self):
        source = self.read("crates/horizon-client/src/java_mods.rs")
        self.assertIn("let game_home = match java_game_home(game_home)", source)
        self.assertIn("let game_home = java_game_home(game_home)?", source)
        self.assertIn("horizon_jvm::jars(&mods_directory(&game_home))", source)

    def test_android_import_destination_comes_from_public_shared_root(self):
        source = self.read("crates/horizon-client/src/game/starling/imports.rs")
        self.assertIn("crate::android::shared_game_home(&game)", source)
        self.assertIn("home.join(crate::JAVA_MODS_DIRECTORY)", source)
        importer = self.read("crates/horizon-client/src/import.rs")
        self.assertIn("places.java_mods.as_ref().ok_or_else", importer)
        self.assertNotIn('places.game.join("mods-java")', importer)

    def test_modrinth_jar_fallback_uses_the_same_public_root_guard(self):
        source = self.read("crates/horizon-client/src/ha_launcher/catalog.rs")
        start = source.index("fn install_modrinth_jar_fallback(")
        section = source[start:start + 900]
        self.assertIn("crate::java_mods::java_game_home(home)?", section)

    def test_documentation_identifies_public_root_and_private_runtime_only(self):
        source = self.read("docs/starling-android-java-mod-storage.md")
        self.assertIn("/storage/emulated/0/StarlingExtras/", source)
        self.assertIn("does not load JARs from the app-private", source)


if __name__ == "__main__":
    unittest.main()
