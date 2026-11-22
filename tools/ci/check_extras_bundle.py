#!/usr/bin/env python3
import argparse
import json
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CATEGORIES = ROOT / "native-mods" / "categories.toml"
PREBUILT = ROOT / "native-mods" / "prebuilt.txt"
EXPECTED_GAME = "26.3"


def fail(message):
    raise SystemExit(f"extras bundle check: {message}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--print-ids", action="store_true")
    args = parser.parse_args()

    if not CATEGORIES.is_file() or not PREBUILT.is_file():
        fail("native-mods/categories.toml or native-mods/prebuilt.txt is missing")

    categories = tomllib.loads(CATEGORIES.read_text(encoding="utf-8"))
    client = categories.get("client_side", {}).get("mods", [])
    server = categories.get("server_side", {}).get("mods", [])
    if len(client) != 16 or len(server) != 4:
        fail(f"expected 16 Client-Side and 4 Server-Side mods, got {len(client)} and {len(server)}")
    if len(set(client)) != len(client) or len(set(server)) != len(server):
        fail("duplicate mod ID in category lists")
    if set(client) & set(server):
        fail("a mod appears in both categories")

    entries = [line.strip() for line in PREBUILT.read_text(encoding="utf-8").splitlines()
               if line.strip() and not line.lstrip().startswith("#")]
    if len(entries) != 2 or entries[0].split() != ["wrench"]:
        fail("prebuilt.txt must list wrench separately, followed by one Extras bundle")
    ids = entries[1].split()
    expected = client + server
    if len(ids) != 20 or len(set(ids)) != 20 or set(ids) != set(expected):
        fail("Extras bundle IDs do not exactly match the 16 Client-Side + 4 Server-Side IDs")
    if "wrench" in ids or "kotost_roulette" in ids:
        fail("base or unrelated mod was included in the Extras bundle")

    current_hooks = set()
    hook_root = ROOT / "crates" / "horizon-mod-vocab" / "src" / "hooks"
    hook_pattern = re.compile(r"#\[hook\s*\((.*?)\)\s*\]\s*fn\s+([A-Za-z_]\w*)\s*\(", re.S)
    for source_path in hook_root.rglob("*.rs"):
        source_text = source_path.read_text(encoding="utf-8")
        for match in hook_pattern.finditer(source_text):
            version = re.search(r"\bversion\s*=\s*(\d+)", match.group(1))
            current_hooks.add((match.group(2), int(version.group(1)) if version else 1))

    old_catalog_path = ROOT / "packaging" / "android" / "native-hooks-0.8.json"
    old_catalog = json.loads(old_catalog_path.read_text(encoding="utf-8"))
    old_hooks = {
        (hook["name"], hook.get("version", 1)): hook["target"]
        for hook in old_catalog.get("hooks", [])
    }
    rust_sources = [path.read_text(encoding="utf-8", errors="replace") for path in (ROOT / "crates").rglob("*.rs")]
    rust_symbols = set(re.findall(r"\b(?:fn|struct|enum|trait|type)\s+([A-Za-z_]\w*)\b", "\n".join(rust_sources)))
    module_targets = {}

    for mod_id in ids:
        bundle = ROOT / "native-mods" / mod_id / "mixins" / mod_id
        manifest_path = bundle / "horizon-mixin.toml"
        metadata_path = ROOT / "native-mods" / mod_id / "horizon-native.json"
        if not manifest_path.is_file() or not metadata_path.is_file():
            fail(f"{mod_id}: missing manifest or horizon-native.json")
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        mod = manifest.get("mod", {})
        compatibility = manifest.get("compatibility", {})
        if mod.get("id") != mod_id or not mod.get("version"):
            fail(f"{mod_id}: manifest ID or version does not match its directory")
        if metadata.get("id") != mod_id or metadata.get("version") != mod["version"]:
            fail(f"{mod_id}: native metadata ID/version does not match its manifest")
        if metadata.get("game") != EXPECTED_GAME:
            fail(f"{mod_id}: game version {metadata.get('game')!r} does not match {EXPECTED_GAME}")
        revision = compatibility.get("horizon_revision", "")
        if not re.fullmatch(r"[0-9a-f]{40}", revision):
            fail(f"{mod_id}: invalid Horizon revision pin")
        if metadata.get("horizon") != revision:
            fail(f"{mod_id}: native metadata Horizon revision does not match its manifest")
        if not re.fullmatch(r"[0-9a-f]{40}", metadata.get("mixins", "")):
            fail(f"{mod_id}: invalid horizon-mixins revision in native metadata")

        module_ids = set()
        for module in manifest.get("module", []):
            module_id = module.get("id")
            if not module_id or module_id in module_ids:
                fail(f"{mod_id}: missing or duplicate module ID {module_id!r}")
            module_ids.add(module_id)
            destination_path = module.get("file", "")
            module_name = module.get("name", "")
            module_key = (destination_path, module_name)
            if not module_name or module_key in module_targets:
                previous = module_targets.get(module_key)
                fail(f"{mod_id}/{module_id}: duplicate or empty module name {module_name!r} in {destination_path!r}; already used by {previous}")
            module_targets[module_key] = f"{mod_id}/{module_id}"
            destination = ROOT / destination_path
            source = bundle / module.get("source", "")
            if not destination.is_file():
                fail(f"{mod_id}/{module_id}: destination file does not exist: {module.get('file')}")
            if not source.is_file():
                fail(f"{mod_id}/{module_id}: module source does not exist: {module.get('source')}")

        injection_ids = set()
        for injection in manifest.get("injection", []):
            injection_id = injection.get("id")
            if not injection_id or injection_id in injection_ids:
                fail(f"{mod_id}: missing or duplicate injection ID {injection_id!r}")
            injection_ids.add(injection_id)
            helper = injection.get("helper")
            if helper and not (bundle / helper).is_file():
                fail(f"{mod_id}/{injection_id}: helper does not exist: {helper}")

            hook = injection.get("hook")
            if hook:
                hook_key = (hook, injection.get("hook_version", 1))
                if hook_key not in current_hooks:
                    legacy_target = old_hooks.get(hook_key)
                    if not legacy_target:
                        fail(f"{mod_id}/{injection_id}: hook {hook_key!r} is absent from current and 0.8 catalogs")
                    legacy_symbol = legacy_target.rsplit("::", 1)[-1]
                    if legacy_symbol not in rust_symbols:
                        fail(f"{mod_id}/{injection_id}: legacy hook target {legacy_target!r} is not implemented in Rust sources")

            target = injection.get("target")
            if target:
                target_symbol = target.rsplit("::", 1)[-1]
                if target_symbol not in rust_symbols:
                    fail(f"{mod_id}/{injection_id}: target symbol {target_symbol!r} does not exist in Rust sources")

        for crate in manifest.get("crate", []):
            crate_path = bundle / crate.get("path", "") / "Cargo.toml"
            if not crate_path.is_file():
                fail(f"{mod_id}: crate manifest does not exist: {crate_path.relative_to(ROOT)}")

    if args.print_ids:
        print(" ".join(ids))
    else:
        print(f"extras bundle check: 20 mods, 16 Client-Side, 4 Server-Side; {len(module_targets)} unique modules, metadata, hooks, targets, helpers, module destinations and crate manifests are present")


if __name__ == "__main__":
    main()
