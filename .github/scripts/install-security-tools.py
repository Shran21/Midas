import argparse
import hashlib
import io
import os
import pathlib
import platform
import tarfile
import urllib.request
import zipfile

tools = {
    "cargo-deny": {
        "base": "https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/",
        "Windows": ("cargo-deny-0.20.2-x86_64-pc-windows-msvc.tar.gz", "975a22143262fd27476d19ee00c7af67978426e40e1dee94eed6bbade1cf87dc"),
        "Linux": ("cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz", "9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"),
    },
    "cargo-audit": {
        "base": "https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/",
        "Windows": ("cargo-audit-x86_64-pc-windows-msvc-v0.22.2.zip", "0a7316540862c13d954f648917ceacca593747baed6eec180fafa590be2710ab"),
        "Linux": ("cargo-audit-x86_64-unknown-linux-musl-v0.22.2.tgz", "7fb9497f8594b389e5fce5ef9b92db08432996895b2e0c5a0167a69ed445c428"),
    },
    "gitleaks": {
        "base": "https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/",
        "Windows": ("gitleaks_8.30.1_windows_x64.zip", "d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e"),
        "Linux": ("gitleaks_8.30.1_linux_x64.tar.gz", "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"),
    },
}

parser = argparse.ArgumentParser()
parser.add_argument("--destination", type=pathlib.Path)
args = parser.parse_args()
system = platform.system()
if system not in ("Windows", "Linux") or platform.machine().lower() not in ("amd64", "x86_64"):
    raise SystemExit("Only Windows/Linux x86_64 binaries are pinned")
destination = args.destination
if destination is None:
    destination = pathlib.Path(os.environ["RUNNER_TEMP"]) / "midas-security-tools"
destination = destination.resolve()
destination.mkdir(parents=True, exist_ok=True)

for name, release in tools.items():
    asset, expected = release[system]
    with urllib.request.urlopen(release["base"] + asset, timeout=60) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != expected:
        raise SystemExit(f"{name}: archive checksum mismatch")
    binary_name = name + (".exe" if system == "Windows" else "")
    if asset.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(archive)) as package:
            members = [m for m in package.infolist() if not m.is_dir() and pathlib.PurePosixPath(m.filename).name == binary_name]
            if len(members) != 1:
                raise SystemExit(f"{name}: expected exactly one binary")
            binary = package.read(members[0])
    else:
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
            members = [m for m in package.getmembers() if m.isfile() and pathlib.PurePosixPath(m.name).name == binary_name]
            if len(members) != 1:
                raise SystemExit(f"{name}: expected exactly one binary")
            with package.extractfile(members[0]) as stream:
                binary = stream.read()
    executable = destination / binary_name
    executable.write_bytes(binary)
    executable.chmod(0o755)
    print(f"Installed {name} from verified {asset}", flush=True)

if "GITHUB_PATH" in os.environ:
    with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as output:
        output.write(str(destination) + "\n")
else:
    print(f"Add to PATH: {destination}")
