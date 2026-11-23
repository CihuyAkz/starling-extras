from pathlib import Path
import os
import shutil

root = Path(__file__).resolve().parent
source = root / "starling-extras-0.9.0-preview"

if not (root / ".git").exists():
    raise SystemExit("Error: jalankan script ini dari working tree Git.")

if not source.is_dir() or source.is_symlink():
    raise SystemExit(f"Error: folder sumber tidak ditemukan: {source.name}")

for source_path in source.rglob("*"):
    relative = source_path.relative_to(source)
    target_path = root / relative
    target_exists = target_path.exists() or target_path.is_symlink()
    if not target_exists:
        continue
    source_is_directory = source_path.is_dir() and not source_path.is_symlink()
    target_is_directory = target_path.is_dir() and not target_path.is_symlink()
    if source_is_directory != target_is_directory:
        raise SystemExit(f"Error: konflik tipe file pada {relative}")

items = sorted(source.iterdir(), key=lambda item: item.name)
for item in items:
    target = root / item.name
    if item.is_symlink():
        if target.exists() or target.is_symlink():
            if target.is_dir() and not target.is_symlink():
                raise SystemExit(f"Error: target adalah folder: {target}")
            target.unlink()
        os.symlink(os.readlink(item), target)
    elif item.is_dir():
        shutil.copytree(item, target, dirs_exist_ok=True, symlinks=True, copy_function=shutil.copy2)
    else:
        shutil.copy2(item, target)

shutil.rmtree(source)
print(f"Berhasil mengekstrak isi {source.name} ke root repository.")
Path(__file__).unlink()
