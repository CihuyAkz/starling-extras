#!/usr/bin/env python3
"""Build Starling's Android APK (Horizon on Android): horizon-android as a
NativeActivity library.

The APK holds the game library, the launcher icon, Starling Visuals and what prepares the game
files on the phone (the Java launchers in native/ and original exporter classes/sources), and, on
arm64-v8a, the libadrenotools Vulkan hook libraries for custom GPU drivers. Nothing of
Minecraft: the resources
horizon-prepare makes from the player's own copy are made on the phone, or copied there.

    python3 packaging/android/build_apk.py                 # build the APK
    python3 packaging/android/build_apk.py --install       # and adb install it
    python3 packaging/android/build_apk.py --zip           # and a zip of it to send
    python3 packaging/android/build_apk.py --preview       # Extras: Preview, the organisation's CI build

Needs the Android SDK (ANDROID_HOME, else the default folder) with the build-tools, platform and
NDK pinned below, the aarch64-linux-android Rust target, cargo-ndk and a JDK (javac, keytool,
apksigner), of the pinned Java major (currently 25). Original exporters are compiled
on the host against checksum-pinned jars, which are never packaged. Without
HORIZON_ANDROID_KEYSTORE it signs with a debug key kept in target/android.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

APPLICATION_ID = "io.github.cihuyakz.starling.extras"
LABEL = "Starling Extras"
VARIANTS = {"preview": (".dev", "Extras: Preview")}
VERSION_NAME = "0.9.0"
VERSION_CODE = 14
MIN_SDK = 26  # AAudio (cpal)
TARGET_SDK = 35
PLATFORM = 36
BUILD_TOOLS = "35.0.0"
NDK = "28.2.13676358"
ABI = "arm64-v8a"
TRIPLE = "aarch64-linux-android"
# Rust target, NDK compiler prefix and NDK C++ runtime directory differ on ARMv7.
TARGETS = {
    "arm64-v8a": (TRIPLE, TRIPLE, TRIPLE),
    "armeabi-v7a": ("armv7-linux-androideabi", "armv7a-linux-androideabi", "arm-linux-androideabi"),
    "x86": ("i686-linux-android", "i686-linux-android", "i686-linux-android"),
    "x86_64": ("x86_64-linux-android", "x86_64-linux-android", "x86_64-linux-android"),
}
LIBRARY = "horizon_android"
# Every LOAD segment aligned for 16 KB pages, which Android 15 devices may use.
PAGE_FLAGS = "-C link-arg=-Wl,-z,max-page-size=16384"
# A BuildId in the library, so the native tombstones in diagnostic ZIPs name it and its frames can be matched to
# the unstripped copy kept beside the APK (`lib<LIBRARY>-symbols.so`, docs/android-diagnostics.md).
BUILD_ID_FLAGS = "-C link-arg=-Wl,--build-id=sha1"
# Rust's Android targets keep each `thread_local!` in a pthread key, and bionic gives a process 128 of them, the
# system's and the GPU driver's included: the game outgrew them once Wasmtime ran in it, and aborted with "out of TLS
# keys" on the first use of one more (saving a world). With `cfg(target_thread_local)` they are the targets' own
# thread-local storage (LLVM's emulated TLS on Android), which has no such limit. No released std is built that way,
# so the std is built with the library from the toolchain's rust-src (`-Zbuild-std`); RUSTC_BOOTSTRAP lets the
# pinned stable Rust take both flags.
TLS_FLAGS = "-Z has-thread-local=yes"
BUILD_STD = "-Zbuild-std=std,panic_unwind"
ICONS = {
    "mipmap-mdpi": "48x48",
    "mipmap-xxhdpi": "128x128",
    "mipmap-xxxhdpi": "256x256",
}
# What horizon-prepare reads from its version folder on the phone (assets/prepare-kit.zip):
# the pins, original sources and verified original exporter classes. The phone
# runs these against the player's jars; ARM32's runtime has no javac module.
PREPARE_KIT = (
    "research-lock.json",
    "crates/horizon-prepare/src/exporters.rs",
    "mods/horizon/src/test/java",
)
BUILTIN_NATIVE_MODS = ()


def sdk_dir():
    env = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if env:
        return Path(env)
    if sys.platform == "win32":
        return Path(os.environ["LOCALAPPDATA"]) / "Android" / "Sdk"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Android" / "sdk"
    return Path.home() / "Android" / "Sdk"


def tool(directory, name):
    """An SDK tool, with Windows' .exe or .bat suffix."""
    suffixes = (".exe", ".bat", ".cmd", "") if os.name == "nt" else ("", ".exe", ".bat", ".cmd")
    for suffix in suffixes:
        path = directory / (name + suffix)
        if path.is_file():
            return path
    sys.exit(f"build_apk: {name} not found in {directory}")


def run(*args, **kwargs):
    print("+", " ".join(str(a) for a in args), flush=True)
    subprocess.run([str(a) for a in args], check=True, **kwargs)


def ndk_tool(ndk, name):
    host = {"win32": "windows-x86_64", "darwin": "darwin-x86_64"}.get(sys.platform, "linux-x86_64")
    return tool(ndk / "toolchains" / "llvm" / "prebuilt" / host / "bin", name)


def emutls_runtime(ndk, abi):
    """The NDK's compiler runtime for `abi`, whose `emutls.c` is LLVM's emulated TLS (`__emutls_get_address`),
    which the thread locals of `TLS_FLAGS` call. Rust links with -nodefaultlibs and its own compiler_builtins, which
    has none, so without this archive the library left the symbol to the phone's dynamic linker, which has none
    either: "dlopen failed: cannot locate symbol "__emutls_get_address"" before the game's first line."""
    arch = TARGETS[abi][2].split("-")[0]
    host = {"win32": "windows-x86_64", "darwin": "darwin-x86_64"}.get(sys.platform, "linux-x86_64")
    clang = ndk / "toolchains" / "llvm" / "prebuilt" / host / "lib" / "clang"
    found = sorted(clang.glob(f"*/lib/linux/libclang_rt.builtins-{arch}-android.a"))
    if not found:
        sys.exit(f"build_apk: no libclang_rt.builtins-{arch}-android.a under {clang}")
    path = found[-1].as_posix()
    if " " in path:
        sys.exit(f"build_apk: {path} has a space, which RUSTFLAGS would split: move the NDK")
    return path


def rust_dylib_build():
    """Whether this build keeps the game's crates in libraries of their own (tools/android_native_sdk.py)."""
    return "prefer-dynamic" in os.environ.get("RUSTFLAGS", "")


def emutls_shared(ndk, abi):
    """`emutls_runtime`'s emulated TLS as one shared library, `libemutls.so`, for a build whose game is several Rust
    libraries (`rust_dylib_build`). Each variable's control block keeps the slot the runtime gave it, and the runtime
    hands slots out from a counter of its own: the archive put a hidden copy of the runtime in every library, so two
    variables of two libraries got one slot, and the first world a phone made aborted in std's thread-local
    destructor list ("already borrowed", 2026-11-19). Every library needs this one instead."""
    archive = emutls_runtime(ndk, abi)
    folder = ROOT / "target" / TARGETS[abi][0] / "emutls"
    folder.mkdir(parents=True, exist_ok=True)
    ar = ndk_tool(ndk, "llvm-ar")
    members = [name for name in subprocess.check_output([str(ar), "t", archive], text=True).split() if "emutls" in name]
    if len(members) != 1:
        sys.exit(f"build_apk: expected one emutls object in {archive}, found {members}")
    run(ar, "x", archive, members[0], cwd=folder)
    library = folder / "libemutls.so"
    run(ndk_tool(ndk, f"{TARGETS[abi][1]}{MIN_SDK}-clang"), "-shared", "-o", library, folder / members[0],
        "-Wl,-soname,libemutls.so", "-Wl,-z,max-page-size=16384", "-Wl,--build-id=sha1")
    path = library.as_posix()
    if " " in path:
        sys.exit(f"build_apk: {path} has a space, which RUSTFLAGS would split: move the checkout")
    return path


def build_library(profile, ndk, sdk, abi=ABI, packages=("horizon-android",)):
    env = dict(os.environ, ANDROID_NDK_HOME=str(ndk), ANDROID_HOME=str(sdk))
    # Preserve source identity in local diagnostic reports, including source builds.
    try:
        env["HORIZON_ANDROID_BUILD_COMMIT"] = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, stderr=subprocess.DEVNULL).strip()
        env["HORIZON_ANDROID_BUILD_DIRTY"] = str(bool(subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=ROOT, text=True))).lower()
    except (OSError, subprocess.CalledProcessError):
        pass  # An exported source archive may not have Git metadata.
    tls = f"{TLS_FLAGS} -C link-arg={emutls_runtime(ndk, abi)}"
    if rust_dylib_build():
        # libemutls.so first: the linker takes __emutls_get_address from it and then pulls only the rest from the
        # archive (its outline atomics, `__aarch64_cas8_acq_rel` and the like, which the phone has nowhere else:
        # without the archive the library did not open), never its emutls object.
        tls = f"{TLS_FLAGS} -C link-arg={emutls_shared(ndk, abi)} -C link-arg={emutls_runtime(ndk, abi)}"
    env["RUSTFLAGS"] = " ".join(
        flags for flags in (env.get("RUSTFLAGS", ""), PAGE_FLAGS, BUILD_ID_FLAGS, tls) if flags
    )
    env["RUSTC_BOOTSTRAP"] = "1"
    # Incremental compilation is part of a profile, and so of the hash in every workspace crate's symbol names:
    # `fast` turns it on, while CI's rust-cache turns it off (CARGO_INCREMENTAL=0). The APK's game built on CI and
    # Compile v2's store's copy of it built elsewhere then named mod_vocab and the rest differently, and every mod
    # library from the store failed to open on the phone ("cannot locate symbol", 2026-11-20). Off here, wherever
    # the build runs, so a native mod built beside any copy of this game build loads in the APK.
    env["CARGO_INCREMENTAL"] = "0"
    env["HORIZON_BRIDGE_PLATFORM"] = "source"
    # The std's source (`TLS_FLAGS`): CI installs it with the toolchain (`--component rust-src`), and the mod
    # service's build cache holds it, with its crates, for its sandbox, which has no network.
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], cwd=ROOT, text=True, env=env).strip()
    if not (Path(sysroot) / "lib" / "rustlib" / "src" / "rust" / "library").is_dir():
        sys.exit("build_apk: the Rust toolchain has no rust-src, which the std build needs: "
                 "rustup component add rust-src")
    run(
        "cargo", "ndk", "-t", abi, "--platform", MIN_SDK,
        # More packages than the game only for native mod libraries (tools/android_native_sdk.py `mods`), built
        # beside it with the same features and flags, so the game's crates are the ones the APK ships.
        "build", BUILD_STD, "--profile", profile, "--features", "horizon-client/java-mods", *(flag for p in packages for flag in ("-p", p)),
        cwd=ROOT, env=env,
    )


def build_launchers(ndk, out, abi=ABI):
    """The `java` and `javac` horizon-prepare runs on the phone (native/java_launcher.c), with the
    C++ runtime the Java runtime's libraries need: an app runs programs only from its library
    folder, so they go in the APK as libraries."""
    _, compiler, sysroot_triple = TARGETS[abi]
    clang = ndk_tool(ndk, f"{compiler}{MIN_SDK}-clang")
    source = HERE / "native" / "java_launcher.c"
    libraries = {}
    for name, flags in (("libstarling_java.so", []), ("libstarling_javac.so", ["-DJAVAC"])):
        target = out / name
        run(
            clang, "-O2", "-fPIE", "-pie", "-Wall", "-Werror", *flags,
            "-Wl,-z,max-page-size=16384", "-o", target, source, "-ldl",
        )
        libraries[name] = target
    sysroot = ndk_tool(ndk, "clang").parent.parent / "sysroot"
    libraries["libc++_shared.so"] = sysroot / "usr" / "lib" / sysroot_triple / "libc++_shared.so"
    return libraries


def build_adrenotools(ndk, out, abi=ABI):
    """libadrenotools (third_party/libadrenotools): the Vulkan driver hook
    (`adrenotools_open_libvulkan`) and its `hook_impl`, `main_hook` and
    `file_redirect_hook` libraries, for custom GPU drivers. Upstream supports
    arm64-v8a only, so other ABIs get none. An app runs libraries only from
    its library folder, so they go in the APK as libraries (extractNativeLibs
    extracts them to nativeLibraryDir, the hook's `hookLibDir`)."""
    if abi != "arm64-v8a":
        return {}
    _, compiler, _ = TARGETS[abi]
    cc = ndk_tool(ndk, f"{compiler}{MIN_SDK}-clang")
    cxx = ndk_tool(ndk, f"{compiler}{MIN_SDK}-clang++")
    src = ROOT / "third_party" / "libadrenotools"
    inc = [
        "-I", src / "include",
        "-I", src / "src",
        "-I", src / "src" / "hook",
        "-I", src / "lib" / "linkernsbypass",
    ]
    page = ["-Wl,-z,max-page-size=16384"]
    # Compiled into each shared library that needs it, as upstream's static
    # liblinkernsbypass is.
    ns = [
        src / "lib" / "linkernsbypass" / "android_linker_ns.cpp",
        src / "lib" / "linkernsbypass" / "elf_soname_patcher.cpp",
    ]
    libraries = {}
    # hook_impl first: the hooks link against it by name, as upstream does.
    hook_impl = out / "libhook_impl.so"
    run(
        cxx, "-O2", "-shared", "-fPIC", "-std=c++17", "-fvisibility=hidden",
        "-Wall", "-Wextra", *inc, *page, "-o", hook_impl,
        src / "src" / "hook" / "hook_impl.cpp", *ns,
        "-llog", "-landroid", "-ldl",
    )
    libraries["libhook_impl.so"] = hook_impl
    for name, source in (
        ("libmain_hook.so", "main_hook.c"),
        ("libfile_redirect_hook.so", "file_redirect_hook.c"),
    ):
        target = out / name
        run(
            cc, "-O2", "-shared", "-fPIC", "-fvisibility=hidden",
            "-Wall", "-Wextra", *inc, *page, "-Wl,-z,global", "-o", target,
            src / "src" / "hook" / source, f"-L{out}", "-l:libhook_impl.so", "-llog",
        )
        libraries[name] = target
    adrenotools = out / "libadrenotools.so"
    # No -fvisibility=hidden here (unlike the hooks, whose sources mark their entry
    # points): driver.cpp exports adrenotools_open_libvulkan with default visibility,
    # which the game finds with dlsym.
    run(
        cxx, "-O2", "-shared", "-fPIC", "-std=c++17",
        "-Wall", "-Wextra", *inc, *page, "-o", adrenotools,
        src / "src" / "driver.cpp", *ns,
        "-llog", "-landroid", "-ldl",
    )
    libraries["libadrenotools.so"] = adrenotools
    return libraries


def zip_folder(folder, archive):
    """`folder`'s files in the zip `archive`, all dated alike: the same files make the same zip,
    which the app tells from the next by its bytes."""
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
        for file in sorted(p for p in folder.rglob("*") if p.is_file()):
            info = zipfile.ZipInfo(file.relative_to(folder).as_posix(), (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, file.read_bytes())


def native_bundle_allowed(relative):
    parts = relative.split("/")
    name = parts[-1]
    in_data = len(parts) >= 2 and (
        parts[0] == "data" or (len(parts) >= 3 and parts[1] == "data")
    )
    text = len(parts) <= 2 and (
        name.startswith(("README", "LICENSE", "LICENCE", "COPYING", "NOTICE"))
        or name == ".gitignore"
    )
    return relative.endswith((".rs", ".toml")) or in_data or text


def bundle_native_mods(assets_folder):
    destination = assets_folder / "native-mods"
    destination.mkdir()
    for mod_id in BUILTIN_NATIVE_MODS:
        source = ROOT / "native-mods" / mod_id
        metadata_path = source / "horizon-native.json"
        mixin_root = source / "mixins" / mod_id
        mixin_path = mixin_root / "horizon-mixin.toml"
        try:
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            mixin = tomllib.loads(mixin_path.read_text(encoding="utf-8"))
        except (OSError, ValueError, tomllib.TOMLDecodeError) as exc:
            raise RuntimeError(f"{mod_id}: unreadable native bundle metadata: {exc}") from exc
        manifest = mixin.get("mod", {})
        compatibility = mixin.get("compatibility", {})
        revision = metadata.get("horizon")
        if (
            metadata.get("schema") != 1
            or metadata.get("id") != mod_id
            or not isinstance(metadata.get("version"), str)
            or not metadata["version"]
            or manifest.get("id") != mod_id
            or manifest.get("version") != metadata.get("version")
            or compatibility.get("horizon_revision") != revision
            or not isinstance(revision, str)
            or not re.fullmatch(r"[0-9a-f]{40}", revision)
            or not re.fullmatch(r"[0-9a-f]{40}", str(metadata.get("mixins", "")))
        ):
            raise RuntimeError(f"{mod_id}: horizon-native.json and horizon-mixin.toml disagree")
        if metadata_path.is_symlink() or mixin_root.is_symlink() or not metadata_path.is_file():
            raise RuntimeError(f"{mod_id}: unsupported native bundle source")
        files = [("horizon-native.json", metadata_path)]
        expanded = metadata_path.stat().st_size
        for path in sorted(mixin_root.rglob("*")):
            if path.is_symlink():
                raise RuntimeError(f"{mod_id}: unsupported bundle entry: {path}")
            if path.is_dir():
                continue
            if not path.is_file():
                raise RuntimeError(f"{mod_id}: unsupported bundle entry: {path}")
            relative = path.relative_to(mixin_root).as_posix()
            if (
                "\\" in relative
                or ":" in relative
                or any(part in ("", ".", "..") for part in relative.split("/"))
                or not native_bundle_allowed(relative)
            ):
                raise RuntimeError(f"{mod_id}: unsupported bundle entry: {relative}")
            size = path.stat().st_size
            expanded += size
            files.append((f"mixins/{mod_id}/{relative}", path))
        if len(files) > 512 or expanded > 64 << 20:
            raise RuntimeError(f"{mod_id}: native bundle exceeds the size or entry limit")
        names = {name for name, _ in files}
        if f"mixins/{mod_id}/horizon-mixin.toml" not in names:
            raise RuntimeError(f"{mod_id}: missing horizon-mixin.toml")
        archive_path = destination / f"{mod_id}.hnmod"
        with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, path in sorted(files):
                info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    compiled_mods = os.environ.get("STARLING_APK_NATIVE_MODS", "").split()
    if compiled_mods:
        if len(compiled_mods) != len(set(compiled_mods)) or any(
            not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", mod_id) for mod_id in compiled_mods
        ):
            raise RuntimeError("STARLING_APK_NATIVE_MODS contains invalid or duplicate mod ids")
        (destination / "compiled.txt").write_text(" ".join(sorted(compiled_mods)) + "\n", encoding="utf-8")


def assets(out, abi=None):
    """The APK's assets/: prepare-kit.zip (PREPARE_KIT, with the paths horizon-prepare reads),
    lowbudget.zip, the skin the setup draws the menus with (packaging/lowbudget/pack, made by its
    make.py), and StarlingVisuals.zip, the shader pack the app puts in shaderpacks/ (also left in
    `out` for the release page). The same for every ABI: `abi` is kept for the callers, from when
    the wasm mods' std went in too (Starling runs no wasm mods since 2026-11-20)."""
    run(sys.executable, ROOT / "tools/build_android_exporters.py")
    folder = out / "assets"
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir()
    bundle_native_mods(folder)
    fonts = folder / "diagnostic-fonts"
    fonts.mkdir()
    for name in ("Geist-Regular.ttf", "Geist-SemiBold.ttf"):
        shutil.copyfile(ROOT / "crates/horizon-theme/fonts" / name, fonts / name)
    with zipfile.ZipFile(folder / "prepare-kit.zip", "w", zipfile.ZIP_DEFLATED) as kit:
        for entry in PREPARE_KIT:
            path = ROOT / entry
            files = sorted(p for p in path.rglob("*.java")) if path.is_dir() else [path]
            for file in files:
                kit.write(file, file.relative_to(ROOT).as_posix())
        bundle = ROOT / "exporters"
        manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
        for name in ["manifest.json", *sorted(manifest["classes"])]:
            kit.write(bundle / name, "exporters/" + name)
    zip_folder(HERE.parent / "lowbudget" / "pack", folder / "lowbudget.zip")
    zip_folder(HERE.parent / "shaderpacks" / "starling-visuals", folder / "StarlingVisuals.zip")
    shutil.copyfile(folder / "StarlingVisuals.zip", out / "StarlingVisuals.zip")
    return folder


def compile_java(build_tools, android_jar, out):
    """StarlingActivity and LauncherActivity (java/) as classes.dex."""
    classes, dex = out / "classes", out / "dex"
    for folder in (classes, dex):
        shutil.rmtree(folder, ignore_errors=True)
        folder.mkdir()
    sources = sorted((HERE / "java").rglob("*.java")) + [out / "generated" / "io/github/juanluci/starling/R.java"]
    run("javac", "--release", "8", "-nowarn", "-classpath", android_jar, "-d", classes, *sources)
    # A single archive also avoids cmd.exe's command-line limit on Windows.
    archive = out / "classes.jar"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as jar:
        for path in sorted(classes.rglob("*.class")):
            jar.write(path, path.relative_to(classes).as_posix())
    run(
        tool(build_tools, "d8"), "--release", "--min-api", MIN_SDK, "--lib", android_jar,
        "--output", dex, archive,
    )
    return dex / "classes.dex"


def version_name(variant):
    """The APK's versionName: the release's VERSION_NAME, and a test build's (Preview, Dev) with
    its variant and commit after it. Compile asks the native-mod build service for the game of this
    version, and the service has the released versions' code only: a test build numbered as the
    release got that release's game in place of its own (NativeMods.chosen loaded it)."""
    if not variant:
        return VERSION_NAME
    try:
        commit = subprocess.check_output(
            ["git", "rev-parse", "--short=10", "HEAD"], cwd=ROOT, text=True,
            stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        commit = ""
    return f"{VERSION_NAME}-{variant}" + (f".{commit}" if commit else "")


def link_resources(aapt2, android_jar, out, assets, variant, abi=ABI):
    """The manifest, the launcher icon, the themes and `assets`, compiled and linked into
    `out/base.apk`; `variant` is None (the release's app) or a key of VARIANTS."""
    manifest = (HERE / "AndroidManifest.xml").read_text(encoding="utf-8")
    suffix, label = VARIANTS[variant] if variant else ("", LABEL)
    application_id = APPLICATION_ID + suffix
    manifest = manifest.replace("@APPLICATION_ID@", application_id).replace("@LABEL@", label)
    if abi != ABI:
        # Additional architectures may expose GLES only. Validate actual GPU limits.
        manifest = manifest.replace('android:version="0x400003" android:required="true"',
                                    'android:version="0x400003" android:required="false"')
    (out / "AndroidManifest.xml").write_text(manifest, encoding="utf-8")
    res = out / "res"
    shutil.rmtree(res, ignore_errors=True)
    for folder, size in ICONS.items():
        target = res / folder
        target.mkdir(parents=True)
        shutil.copyfile(
            ROOT / "packaging" / "icons" / "hicolor" / size / "apps" / "horizon.png",
            target / "ic_launcher.png",
        )
    # The window themes (res/: the starting window's icon and background).
    shutil.copytree(HERE / "res", res, dirs_exist_ok=True)
    shortcuts = res / "xml" / "shortcuts.xml"
    shortcuts.write_text(shortcuts.read_text(encoding="utf-8").replace("@APPLICATION_ID@", application_id), encoding="utf-8")
    # Match the setup dialogs without loading Rust or the game's renderer.
    theme = (ROOT / "crates/horizon-theme/src/lib.rs").read_text(encoding="utf-8")
    dark = theme.split("pub const DARK: Palette = Palette {", 1)[1].split("};", 1)[0]
    colors = dict(re.findall(r"(\w+): 0x([0-9a-fA-F]{6})", dark))
    entries = "\n".join(
        f'<color name="starling_{name}">#{colors[name]}</color>'
        for name in ("page", "rule", "ink", "ink_soft", "accent", "on_accent")
    )
    (res / "values/starling-colors.xml").write_text(f"<resources>\n{entries}\n</resources>\n", encoding="utf-8")
    compiled = out / "compiled"
    shutil.rmtree(compiled, ignore_errors=True)
    compiled.mkdir()
    run(aapt2, "compile", "--dir", res, "-o", compiled)
    (out / "generated").mkdir(exist_ok=True)
    # Debuggable (adb's run-as reaches the app's own files) except when signed for a release.
    debug = [] if os.environ.get("HORIZON_ANDROID_KEYSTORE") else ["--debug-mode"]
    run(
        aapt2, "link", "-o", out / "base.apk",
        "--manifest", out / "AndroidManifest.xml",
        "--java", out / "generated", "--custom-package", "io.github.juanluci.starling",
        "-I", android_jar,
        "--min-sdk-version", MIN_SDK, "--target-sdk-version", TARGET_SDK,
        "--version-code", VERSION_CODE, "--version-name", version_name(variant),
        *debug,
        "-A", assets,
        *sorted(compiled.glob("*.flat")),
    )


def add_code(base, libraries, dex, unsigned, abi=ABI):
    """`base` plus the activity's dex and the libraries, compressed: Android extracts them at
    install (extractNativeLibs), as it must for the Java launchers to run."""
    shutil.copyfile(base, unsigned)
    with zipfile.ZipFile(unsigned, "a") as apk:
        apk.write(dex, "classes.dex", compress_type=zipfile.ZIP_DEFLATED)
        for name, path in sorted(libraries.items()):
            apk.write(path, f"lib/{abi}/{name}", compress_type=zipfile.ZIP_DEFLATED)


def sign(build_tools, aligned, apk, out, variant=None):
    keystore = os.environ.get("HORIZON_ANDROID_KEYSTORE")
    env = dict(os.environ)
    if not keystore:
        previous = ROOT / ".local" / "android" / "debug.keystore"
        if previous.is_file():
            keystore = previous
            default_alias = "androiddebugkey"
            env.setdefault("HORIZON_ANDROID_STORE_PASSWORD", "android")
            env.setdefault("HORIZON_ANDROID_KEY_PASSWORD", "android")
        else:
            keystore = out / ("preview.keystore" if variant == "preview" else "debug.keystore")
            default_alias = "horizon-preview" if variant == "preview" else "horizon-debug"
            env["HORIZON_ANDROID_STORE_PASSWORD"] = env["HORIZON_ANDROID_KEY_PASSWORD"] = "android"
            if not keystore.exists():
                run(
                    "keytool", "-genkeypair", "-noprompt", "-storetype", "JKS", "-keystore", keystore,
                    "-storepass", "android", "-keypass", "android", "-alias", default_alias, "-keyalg", "RSA",
                    "-keysize", "2048", "-validity", "10000", "-dname", f"CN=Starling Extras {variant or 'Local'}",
                )
                try:
                    keystore.chmod(0o600)
                except OSError:
                    pass
    else:
        default_alias = "horizon-debug"
        missing = [name for name in ("HORIZON_ANDROID_STORE_PASSWORD", "HORIZON_ANDROID_KEY_PASSWORD") if not env.get(name)]
        if missing:
            sys.exit("build_apk: signing keystore is set but these password variables are missing: " + ", ".join(missing))
    alias = os.environ.get("HORIZON_ANDROID_KEY_ALIAS", default_alias)
    apksigner = tool(build_tools, "apksigner")
    run(
        apksigner, "sign", "--ks", keystore, "--ks-key-alias", alias,
        "--ks-pass", "env:HORIZON_ANDROID_STORE_PASSWORD",
        "--key-pass", "env:HORIZON_ANDROID_KEY_PASSWORD",
        "--out", apk, aligned, env=env,
    )
    run(apksigner, "verify", apk)


def rust_dylibs(ndk, built, out):
    """The game's Rust libraries the entry library needs, stripped into `out`: none in a usual build, every crate
    being inside `built`. A build with `-C prefer-dynamic` after tools/android_native_sdk.py's `prepare` keeps the
    game and the mod API in libraries of their own, which native mod libraries link against (and the std inside
    the lowest of them): Cargo's deps folder holds them, and each may need more of them."""
    if not rust_dylib_build():
        return {}
    readelf = ndk_tool(ndk, "llvm-readelf")
    # Cargo's deps folder, and `emutls_shared`'s for the emulated TLS every one of them needs.
    folders = (built.parent / "deps", built.parent.parent / "emutls")
    found, pending = {}, [built]
    while pending:
        needs = subprocess.check_output([str(readelf), "-d", str(pending.pop())], text=True)
        for name in re.findall(r"\(NEEDED\)\s+Shared library: \[(.+?)\]", needs):
            source = next((folder / name for folder in folders if (folder / name).is_file()), None)
            if name in found or source is None:
                continue
            target = out / name
            run(ndk_tool(ndk, "llvm-strip"), "--strip-unneeded", "-o", target, source)
            found[name] = target
            pending.append(source)
    print(f"build_apk: the game's Rust libraries: {', '.join(sorted(found)) or 'none'}")
    return found


def compiler_runtime_resolves(ndk, libraries):
    """The compiler runtime's symbols (outline atomics, emulated TLS) every library of a several-library game leaves
    undefined, each defined by one of `libraries`: the phone has them in no system library, and a missing one stopped
    the game before its first line ("cannot locate symbol __aarch64_cas8_acq_rel", 2026-11-19)."""
    nm = str(ndk_tool(ndk, "llvm-nm"))
    runtime = re.compile(r"__(aarch64_|emutls_)\w+")
    defined, wanted = set(), {}
    for path in libraries:
        defined.update(subprocess.check_output([nm, "-D", "--defined-only", "--format=just-symbols", str(path)],
                                               text=True).split())
        for name in subprocess.check_output([nm, "-D", "--undefined-only", "--format=just-symbols", str(path)],
                                            text=True).split():
            if runtime.fullmatch(name):
                wanted.setdefault(name, []).append(Path(path).name)
    missing = {name: users for name, users in wanted.items() if name not in defined}
    if missing:
        sys.exit("build_apk: compiler runtime symbols no library defines: "
                 + "; ".join(f"{name} ({', '.join(users)})" for name, users in sorted(missing.items())))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--profile", default="quick",
        help="Cargo profile (default: quick, for iteration; fast or release to measure and ship)",
    )
    parser.add_argument("--no-build", action="store_true", help="skip compiling the native library and package the existing build")
    parser.add_argument("--install", action="store_true", help="adb install the APK")
    parser.add_argument(
        "--zip", action="store_true",
        help="also zip the APK, for chats that take no .apk",
    )
    parser.add_argument(
        "--keep-symbols", action="store_true", help="ship the library unstripped (backtraces)"
    )
    parser.add_argument(
        "--library-only", action="store_true", help="stop at the stripped library: no APK"
    )
    parser.add_argument(
        "--preview", action="store_true",
        help=f"Extras: Preview ({APPLICATION_ID}{VARIANTS['preview'][0]}), the organisation's CI build",
    )
    parser.add_argument("--abi", choices=TARGETS, default=ABI, help="Android ABI (default: arm64-v8a)")
    args = parser.parse_args()
    triple, _, _ = TARGETS[args.abi]

    sdk = sdk_dir()
    ndk = Path(os.environ.get("ANDROID_NDK_HOME") or sdk / "ndk" / NDK)
    build_tools = sdk / "build-tools" / BUILD_TOOLS
    android_jar = sdk / "platforms" / f"android-{PLATFORM}" / "android.jar"
    for path in (ndk, build_tools, android_jar):
        if not path.exists():
            sys.exit(f"build_apk: missing {path}")

    if not args.no_build:
        build_library(args.profile, ndk, sdk, args.abi)
    profile_dir = "debug" if args.profile == "dev" else args.profile
    built = ROOT / "target" / triple / profile_dir / f"lib{LIBRARY}.so"
    if not built.is_file():
        sys.exit(f"build_apk: {built} not built")
    # The check the phone's dynamic linker makes when it opens the library (`emutls_runtime`), made here first.
    undefined = subprocess.check_output([str(ndk_tool(ndk, "llvm-nm")), "-D", "--undefined-only", str(built)], text=True)
    # A build whose game is several Rust libraries takes it from libemutls.so (`emutls_shared`), checked below.
    if "__emutls_get_address" in undefined.split() and not rust_dylib_build():
        sys.exit(f"build_apk: {built} leaves __emutls_get_address undefined, and no phone defines it")

    out = ROOT / "target" / "android"
    out.mkdir(parents=True, exist_ok=True)
    library = out / f"lib{LIBRARY}.so"
    if args.keep_symbols:
        shutil.copyfile(built, library)
    else:
        run(ndk_tool(ndk, "llvm-strip"), "--strip-unneeded", "-o", library, built)
    if args.library_only:
        return
    if not args.keep_symbols:
        # The same library with its symbols, never shipped: CI keeps it to read crash frames. Native-mod
        # builds (--library-only) don't need one.
        shutil.copyfile(built, out / f"lib{LIBRARY}-symbols.so")

    variant = "preview" if args.preview else None
    link_resources(
        tool(build_tools, "aapt2"), android_jar, out, assets(out, args.abi), variant, args.abi
    )
    dex = compile_java(build_tools, android_jar, out)
    libraries = build_launchers(ndk, out, args.abi)
    # Custom GPU drivers (libadrenotools): arm64-v8a only, beside the launchers.
    libraries.update(build_adrenotools(ndk, out, args.abi))
    libraries[library.name] = library
    libraries.update(rust_dylibs(ndk, built, out))
    if rust_dylib_build():
        if "libemutls.so" not in libraries:
            sys.exit("build_apk: no library of the game needs libemutls.so: its thread locals would have no runtime")
        compiler_runtime_resolves(ndk, [library, *(libraries[name] for name in libraries if name.endswith(".so"))])
    add_code(out / "base.apk", libraries, dex, out / "unsigned.apk", args.abi)
    aligned = out / "aligned.apk"
    run(tool(build_tools, "zipalign"), "-P", "16", "-f", "4", out / "unsigned.apk", aligned)
    apk = out / f"starling{'-' + variant if variant else ''}-{VERSION_NAME}-{args.abi}.apk"
    sign(build_tools, aligned, apk, out, variant)
    print(apk)
    if args.zip:
        # Sent where an .apk is refused (a chat, a phone away from adb): unzip, then install.
        packed = apk.with_suffix(".apk.zip")
        with zipfile.ZipFile(packed, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            z.write(apk, apk.name)
        print(packed)
    if args.install:
        adb = tool(sdk / "platform-tools", "adb")
        run(adb, "install", "-r", apk)


if __name__ == "__main__":
    main()
