import json
import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
EXTRAS_MODS = (
    "zoom",
    "recipes",
    "smartf3",
    "freecam_plus",
    "connected_textures",
    "armorhud",
    "jade",
    "visuality_horizon",
    "punchy",
    "freelook",
    "cinematic_interactions",
    "firstpersonmodel",
    "eatinganimation",
    "veinminer",
    "liteminer",
    "appleskin",
    "rightharvest",
    "customhud",
    "carryblock",
)


def fail(message):
    print(f"Starling Extras bundle check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_sets():
    lines = (ROOT / "native-mods" / "prebuilt.txt").read_text(encoding="utf-8").splitlines()
    return [line.split() for line in lines if line.strip() and not line.lstrip().startswith("#")]


def main():
    sets = read_sets()
    expected_sets = [["wrench"], list(EXTRAS_MODS)]
    if sets != expected_sets:
        fail(f"native-mods/prebuilt.txt must contain wrench and the complete Starling Extras set; found {sets}")

    checked = []
    for mod_id in EXTRAS_MODS:
        mod_root = ROOT / "native-mods" / mod_id
        bundle_root = mod_root / "mixins" / mod_id
        metadata_path = mod_root / "horizon-native.json"
        manifest_path = bundle_root / "horizon-mixin.toml"

        for path in (metadata_path, manifest_path):
            if not path.is_file():
                fail(f"{mod_id}: required file is missing: {path.relative_to(ROOT)}")

        try:
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as exc:
            fail(f"{mod_id}: invalid metadata or manifest: {exc}")

        if metadata.get("id") != mod_id:
            fail(f"{mod_id}: horizon-native.json id is {metadata.get('id')!r}")
        if manifest.get("mod", {}).get("id") != mod_id:
            fail(f"{mod_id}: horizon-mixin.toml [mod].id does not match its directory")
        if metadata.get("version") != manifest.get("mod", {}).get("version"):
            fail(f"{mod_id}: metadata and manifest versions differ")

        for module in manifest.get("module", []):
            source = module.get("source")
            if not source or not (bundle_root / source).is_file():
                fail(f"{mod_id}: module source is missing: {source!r}")

        for injection in manifest.get("injection", []):
            helper = injection.get("helper")
            if helper and not (bundle_root / helper).is_file():
                fail(f"{mod_id}: injection helper is missing: {helper}")

        checked.append(mod_id)

    print(f"Validated {len(checked)} Starling Extras native mods: {', '.join(checked)}")
    print("Validated wrench as a separate base native-mod build.")
    print("All bundled module sources and injection helpers are present.")


if __name__ == "__main__":
    main()
