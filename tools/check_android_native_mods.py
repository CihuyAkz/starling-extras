#!/usr/bin/env python3
import json
import pathlib
import sys
import tomllib


def fail(message):
    raise SystemExit(f"android native mod preflight: {message}")


def check(root, bundles, expected=None):
    root = pathlib.Path(root).resolve()
    bundles = pathlib.Path(bundles).resolve()
    if not bundles.is_dir():
        fail(f"mod bundle directory does not exist: {bundles}")
    sys.path.insert(0, str(root / "packaging" / "android"))
    import native_mod_adapt as adapt

    generated_catalog = root / "native-hooks.json"
    old_catalog = root / "packaging" / "android" / "native-hooks-0.8.json"
    if not generated_catalog.is_file() or not generated_catalog.stat().st_size:
        fail("horizon-materialize did not leave a generated native-hooks.json")
    if not old_catalog.is_file() or not old_catalog.stat().st_size:
        fail("Starling 0.8 compatibility hook catalog is missing")

    catalog = json.loads(generated_catalog.read_text(encoding="utf-8"))
    hook_ids = {(item["name"], int(item.get("version", 1))) for item in catalog.get("hooks", [])}
    if not hook_ids:
        fail("generated hook catalog is empty")

    tree = adapt.module_tree(root)
    if not tree:
        fail("could not index the source module tree")

    bundle_dirs = sorted(path for path in bundles.iterdir() if path.is_dir())
    if not bundle_dirs:
        fail(f"no mod bundles found in {bundles}")
    found_ids = set()

    for bundle in bundle_dirs:
        manifest_path = bundle / "horizon-mixin.toml"
        if not manifest_path.is_file():
            fail(f"{bundle.name}: missing horizon-mixin.toml")
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        mod = manifest.get("mod", {})
        mod_id = mod.get("id")
        if not mod_id or mod_id != bundle.name:
            fail(f"{bundle.name}: manifest id does not match bundle directory")
        if mod_id in found_ids:
            fail(f"duplicate bundle id: {mod_id}")
        found_ids.add(mod_id)

        for module in manifest.get("module", []):
            file_name = module.get("file")
            if not file_name:
                fail(f"{mod_id}: module {module.get('id')} has no source file")
            target = root / file_name
            if not target.is_file():
                fail(f"{mod_id}: module source is missing: {file_name}")
            source_file = module.get("source")
            if not source_file or not (bundle / source_file).is_file():
                fail(f"{mod_id}: module source bundle file is missing: {source_file}")

        for injection in manifest.get("injection", []):
            injection_id = injection.get("id", "unnamed")
            helper = injection.get("helper")
            if helper and not (bundle / helper).is_file():
                fail(f"{mod_id}/{injection_id}: helper is missing: {helper}")

            hook = injection.get("hook")
            if hook:
                version = int(injection.get("hook_version", 1))
                if (hook, version) not in hook_ids:
                    fail(f"{mod_id}/{injection_id}: unresolved hook {hook}@{version}")

            raw_target = injection.get("target")
            if not raw_target:
                continue
            target = adapt.relocate(raw_target, tree)
            parts = adapt.split_target(target)
            if not parts:
                continue
            module_name, _, kind, name = parts
            crate = module_name.split("::", 1)[0]
            if module_name not in tree:
                if crate in adapt.CRATES:
                    fail(f"{mod_id}/{injection_id}: missing module target {target}")
                continue
            module = tree[module_name]
            exists = (kind, name) in module.methods if kind else name in module.functions
            if not exists:
                fail(f"{mod_id}/{injection_id}: missing function target {target}")

    if expected:
        expected_ids = []
        for line in pathlib.Path(expected).read_text(encoding="utf-8").splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                expected_ids.extend(line.split())
        if len(expected_ids) != len(set(expected_ids)):
            fail("native-mods/prebuilt.txt contains duplicate mod ids")
        if set(expected_ids) != found_ids:
            missing = sorted(set(expected_ids) - found_ids)
            extra = sorted(found_ids - set(expected_ids))
            fail(f"prebuilt bundle mismatch; missing={missing}, extra={extra}")

    hook_count = len(hook_ids)
    print(
        f"android native mod preflight: {len(found_ids)} bundles, "
        f"{hook_count} generated hooks, all source targets and helpers resolved"
    )


if __name__ == "__main__":
    if len(sys.argv) not in (3, 4):
        fail("usage: check_android_native_mods.py SOURCE MOD_BUNDLES [PREBUILT_LIST]")
    check(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) == 4 else None)
