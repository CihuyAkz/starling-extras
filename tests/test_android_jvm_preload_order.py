from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class AndroidJvmPreloadOrderTests(unittest.TestCase):
    def test_libjvm_is_loaded_before_libverify(self):
        source = (ROOT / "crates/horizon-jvm/src/lib.rs").read_text(encoding="utf-8")
        android = source.index("#[cfg(target_os = \"android\")]\n    let create: CreateJavaVm = {")
        block = source[android:source.index("#[cfg(not(target_os = \"android\"))]", android)]
        jvm_load = block.index('Library::open(Some(path.as_os_str()), RTLD_LAZY | RTLD_GLOBAL)')
        verify_load = block.index('"lib/libverify.so"')
        self.assertLess(jvm_load, verify_load)
        self.assertIn('JNI_CreateJavaVM', block)


if __name__ == "__main__":
    unittest.main()
