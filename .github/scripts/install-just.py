import hashlib
import io
import os
import pathlib
import platform
import tarfile
import zipfile

from download import fetch

version = "1.59.0"
assets = {
    "Windows": ("x86_64-pc-windows-msvc.zip", "9c5463e2d0974e31bee71346aa5cbf19167de1585a4abc1bae7b3b04604ea9c0"),
    "Linux": ("x86_64-unknown-linux-musl.tar.gz", "409044617b6f06517c1035e55ca38c8c856ff8b650e41bc2aa1ef82d5903a23e"),
}
suffix, expected = assets[platform.system()]
url = f"https://github.com/casey/just/releases/download/{version}/just-{version}-{suffix}"
archive = fetch(url)
if hashlib.sha256(archive).hexdigest() != expected:
    raise SystemExit("just archive checksum mismatch")
destination = pathlib.Path(os.environ["RUNNER_TEMP"]) / "midas-tools"
destination.mkdir(exist_ok=True)
if platform.system() == "Windows":
    with zipfile.ZipFile(io.BytesIO(archive)) as package:
        binary = package.read("just.exe")
    executable = destination / "just.exe"
else:
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        member = next(item for item in package.getmembers() if pathlib.PurePosixPath(item.name).name == "just" and item.isfile())
        with package.extractfile(member) as stream:
            binary = stream.read()
    executable = destination / "just"
executable.write_bytes(binary)
executable.chmod(0o755)
with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as output:
    output.write(str(destination) + "\n")
