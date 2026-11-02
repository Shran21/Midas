import hashlib
import os
import pathlib
import platform
import shutil
import tempfile
import urllib.request
import zipfile

if platform.system() != 'Windows' or platform.machine().lower() not in ('amd64', 'x86_64'):
    raise SystemExit('This package is for Windows x86_64')

url = 'https://get.enterprisedb.com/postgresql/postgresql-18.6-5-windows-x64-binaries.zip'
expected = 'e2246ba91d22345bc3d017586c09ede52d9df180b1eeb480f050445f1cad84e2'
destination = pathlib.Path(os.environ.get('RUNNER_TEMP', '.local/tools')) / 'postgres-18.6'
destination = destination.resolve()
destination.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryFile() as archive:
    with urllib.request.urlopen(url, timeout=60) as response:
        shutil.copyfileobj(response, archive)
    archive.seek(0)
    if hashlib.file_digest(archive, 'sha256').hexdigest() != expected:
        raise SystemExit('PostgreSQL archive checksum mismatch')
    archive.seek(0)
    with zipfile.ZipFile(archive) as package:
        for member in package.infolist():
            path = pathlib.PurePosixPath(member.filename)
            if path.parts[:2] not in [('pgsql', 'bin'), ('pgsql', 'lib'), ('pgsql', 'share')]:
                continue
            output = destination.joinpath(*path.parts).resolve()
            if not output.is_relative_to(destination):
                raise SystemExit('Unsafe archive path')
            if member.is_dir():
                output.mkdir(parents=True, exist_ok=True)
            else:
                output.parent.mkdir(parents=True, exist_ok=True)
                with package.open(member) as source, output.open('wb') as target:
                    shutil.copyfileobj(source, target)

binary_path = destination / 'pgsql/bin'
if 'GITHUB_ENV' in os.environ:
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as output:
        output.write(f'MIDAS_TEST_POSTGRES_BIN={binary_path}\n')
print(f'PostgreSQL 18.6 installed: {binary_path}')
