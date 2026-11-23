#!/usr/bin/env python3
"""Create persistent Android signing keys and store them in GitHub Environments."""

import argparse
import base64
import secrets
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SECRET_NAMES = (
    "HORIZON_ANDROID_KEYSTORE_BASE64",
    "HORIZON_ANDROID_KEY_ALIAS",
    "HORIZON_ANDROID_STORE_PASSWORD",
    "HORIZON_ANDROID_KEY_PASSWORD",
)
LEGACY_PUBLIC = (
    "STARLING_KEYSTORE_BASE64",
    "STARLING_KEY_ALIAS",
    "STARLING_STORE_PASSWORD",
    "STARLING_KEY_PASSWORD",
)


def command(args, *, input_text=None, check=True):
    return subprocess.run(
        args,
        input=input_text,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=check,
    )


def names(repo, environment=None):
    args = ["gh", "secret", "list", "--repo", repo, "--json", "name", "--jq", ".[].name"]
    if environment:
        args.extend(("--env", environment))
    result = command(args)
    return set(line.strip() for line in result.stdout.splitlines() if line.strip())


def create_environment(repo, environment):
    result = command(["gh", "api", f"repos/{repo}/environments/{environment}"], check=False)
    if result.returncode == 0:
        return
    if "404" not in result.stderr:
        raise subprocess.CalledProcessError(result.returncode, result.args, result.stdout, result.stderr)
    command(["gh", "api", "--method", "PUT", f"repos/{repo}/environments/{environment}"])


def store_secret(repo, environment, key, value):
    command(["gh", "secret", "set", key, "--repo", repo, "--env", environment], input_text=value)


def create_keystore(environment):
    keytool = shutil.which("keytool")
    if not keytool:
        raise RuntimeError("keytool tidak ditemukan. Install JDK dan pastikan keytool ada di PATH.")
    alias = "horizon-preview" if environment == "preview" else "horizon-release"
    store_password = secrets.token_urlsafe(32)
    key_password = secrets.token_urlsafe(32)
    with tempfile.TemporaryDirectory(prefix="starling-android-signing-") as temporary:
        keystore = Path(temporary) / "starling-android.jks"
        command([
            keytool, "-genkeypair", "-noprompt", "-storetype", "JKS", "-keystore", str(keystore),
            "-storepass", store_password, "-keypass", key_password, "-alias", alias,
            "-keyalg", "RSA", "-keysize", "3072", "-validity", "10000",
            "-dname", f"CN=Starling Extras {environment.title()}",
        ])
        encoded = base64.b64encode(keystore.read_bytes()).decode("ascii")
    return {
        "HORIZON_ANDROID_KEYSTORE_BASE64": encoded,
        "HORIZON_ANDROID_KEY_ALIAS": alias,
        "HORIZON_ANDROID_STORE_PASSWORD": store_password,
        "HORIZON_ANDROID_KEY_PASSWORD": key_password,
    }


def configure_environment(repo, environment, repository_secret_names):
    create_environment(repo, environment)
    environment_secret_names = names(repo, environment)
    complete = set(SECRET_NAMES)
    present = complete & environment_secret_names
    if present == complete:
        print(f"{environment}: signing key sudah tersedia; tidak diubah.")
        return
    if present:
        missing = sorted(complete - present)
        raise RuntimeError(f"Environment {environment} memiliki signing secret sebagian. Kurang: {', '.join(missing)}")
    repository_generic = complete & repository_secret_names
    if repository_generic == complete:
        print(f"{environment}: signing key repository-level sudah tersedia; tidak diubah.")
        return
    if repository_generic:
        missing = sorted(complete - repository_generic)
        raise RuntimeError(f"Repository memiliki signing secret HORIZON_ANDROID_* sebagian. Kurang: {', '.join(missing)}")

    if environment == "preview":
        legacy_preview = {"STARLING_DEBUG_KEYSTORE_BASE64", "STARLING_PREVIEW_KEYSTORE_BASE64"}
        if legacy_preview & repository_secret_names:
            print("preview: memakai Preview signing key lama pada repository; tidak membuat kunci baru.")
            return
        if legacy_preview & environment_secret_names:
            print("preview: memakai Preview signing key lama pada environment; tidak membuat kunci baru.")
            return

    if environment == "release":
        legacy = set(LEGACY_PUBLIC) & environment_secret_names
        if "STARLING_KEYSTORE_BASE64" in legacy:
            print("release: signing key STARLING_* lama ditemukan; tidak diubah.")
            return
        if legacy:
            raise RuntimeError("Environment release memiliki metadata STARLING_* tanpa STARLING_KEYSTORE_BASE64.")
        legacy_repository = set(LEGACY_PUBLIC) & repository_secret_names
        if "STARLING_KEYSTORE_BASE64" in legacy_repository:
            print("release: signing key STARLING_* lama pada repository ditemukan; tidak diubah.")
            return
        if legacy_repository:
            raise RuntimeError("Repository memiliki metadata STARLING_* tanpa STARLING_KEYSTORE_BASE64.")

    values = create_keystore(environment)
    for key in SECRET_NAMES:
        store_secret(repo, environment, key, values[key])
    print(f"{environment}: signing key baru dibuat dan disimpan sebagai Environment Secrets.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="GitHub repository, e.g. CihuyAkz/starling-extras")
    parser.add_argument("--mode", choices=("preview", "public", "both"), default="both")
    args = parser.parse_args()

    if not shutil.which("gh"):
        parser.error("GitHub CLI (gh) tidak ditemukan di PATH.")
    command(["gh", "auth", "status"])
    repository_secret_names = names(args.repo)
    selected = ("preview", "release") if args.mode == "both" else ("release",) if args.mode == "public" else ("preview",)
    for environment in selected:
        configure_environment(args.repo, environment, repository_secret_names)

    print("Selesai. Workflow memakai kunci yang sama untuk semua ABI dan run berikutnya.")
    print("Jangan hapus atau mengganti signing secrets setelah APK dipublikasikan.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"setup_signing: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.strip(), file=sys.stderr)
        sys.exit(1)
