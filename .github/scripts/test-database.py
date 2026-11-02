import os
import pathlib
import secrets
import socket
import subprocess
import sys
import tempfile

sys.stdout.reconfigure(encoding='utf-8')
command = ['cargo', 'test', '--locked', '-p', 'midas-server', '--test', 'database', '--', '--ignored']
flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0

def stream(arguments, environment):
    process = subprocess.Popen(arguments, env=environment, creationflags=flags,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               text=True, encoding='utf-8', errors='replace')
    for line in process.stdout:
        print(line, end='', flush=True)
    return process.wait()

if os.environ.get('MIDAS_TEST_DATABASE_ADMIN_URL'):
    raise SystemExit(stream(command, os.environ))

binary_path = pathlib.Path(os.environ.get('MIDAS_TEST_POSTGRES_BIN', '.local/tools/postgres-18.6/pgsql/bin')).resolve()
extension = '.exe' if os.name == 'nt' else ''
if not (binary_path / ('initdb' + extension)).is_file():
    raise SystemExit('Set MIDAS_TEST_POSTGRES_BIN or MIDAS_TEST_DATABASE_ADMIN_URL; see docs/adatbazis.md')

temporary_root = pathlib.Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())).resolve()
with tempfile.TemporaryDirectory(prefix='midas-postgres-', dir=temporary_root) as directory:
    work = pathlib.Path(directory).resolve()
    assert work.is_relative_to(temporary_root) and work != temporary_root
    data = work / 'data'
    password = secrets.token_urlsafe(32)
    password_file = work / 'password'
    password_file.write_text(password, encoding='utf-8')
    environment = {**os.environ, 'PGPASSWORD': password}
    with socket.socket() as port_selector:
        port_selector.bind(('127.0.0.1', 0))
        port = port_selector.getsockname()[1]

    def run(name, *args, check=True):
        arguments = [str(binary_path / (name + extension)), *map(str, args)]
        output = work / f'{name}-{secrets.token_hex(4)}.output'
        with output.open('wb') as log:
            result = subprocess.run(arguments, env=environment, creationflags=flags,
                                    stdout=log, stderr=subprocess.STDOUT).returncode
        print(output.read_text(encoding='utf-8', errors='replace'), end='', flush=True)
        if check and result:
            raise subprocess.CalledProcessError(result, arguments)
        return result

    run('initdb', '-D', data, '-U', 'midas_test_admin', '--pwfile', password_file,
        '--auth-host=scram-sha-256', '--auth-local=scram-sha-256', '--encoding=UTF8', '--locale=C', '--no-instructions')
    password_file.unlink()
    started = False
    try:
        run('pg_ctl', '-D', data, '-l', work / 'postgres.log', '-o', f'-h 127.0.0.1 -p {port}', '-w', 'start')
        started = True
        run('createdb', '-h', '127.0.0.1', '-p', port, '-U', 'midas_test_admin', 'midas_test_isolation')
        environment['MIDAS_TEST_DATABASE_ADMIN_URL'] = f'postgresql://midas_test_admin:{password}@127.0.0.1:{port}/midas_test_isolation'
        result = stream(command, environment)
    finally:
        if started:
            run('pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
    raise SystemExit(result)
