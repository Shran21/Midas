import hashlib
import os
import pathlib
import platform
import shutil
import tempfile
import zipfile

from download import fetch

if platform.system() != 'Windows' or platform.machine().lower() not in ('amd64', 'x86_64'):
    raise SystemExit('This package is for Windows x86_64')

url = 'https://github.com/StrawberryPerl/Perl-Dist-Strawberry/releases/download/SP_54231_64bit/strawberry-perl-5.42.3.1-64bit-portable.zip'
expected = '6a081a811781c30aca51dbc036afd93092af91e3297901f02c17043795a10690'
destination = (pathlib.Path(os.environ.get('RUNNER_TEMP', '.local/tools')) / 'perl-5.42.3.1').resolve()
destination.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryFile() as archive:
    fetch(url, archive)
    archive.seek(0)
    if hashlib.file_digest(archive, 'sha256').hexdigest() != expected:
        raise SystemExit('Perl archive checksum mismatch')
    archive.seek(0)
    with zipfile.ZipFile(archive) as package:
        for member in package.infolist():
            path = pathlib.PurePosixPath(member.filename)
            if path.parts[:2] not in [('perl', 'bin'), ('perl', 'lib'), ('perl', 'site'), ('c', 'bin')]:
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

binary = destination / 'perl/bin/perl.exe'
if 'GITHUB_ENV' in os.environ:
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as output:
        output.write(f'OPENSSL_SRC_PERL={binary}\n')
print(f'Native Perl installed: {binary}')
