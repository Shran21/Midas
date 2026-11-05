"""Local authenticated development stack: PostgreSQL, HTTPS server with sign-in and the web panel.

All state, generated certificates and demo credentials stay in .local/dev-auth. Use --reset to start over.
"""
import csv
import json
import os
import pathlib
import secrets
import shutil
import socket
import subprocess
import sys

sys.stdout.reconfigure(encoding='utf-8')
root = pathlib.Path(__file__).resolve().parents[2]
work = root / '.local' / 'dev-auth'
binaries = pathlib.Path(os.environ.get('MIDAS_TEST_POSTGRES_BIN', root / '.local/tools/postgres-18.6/pgsql/bin')).resolve()
extension = '.exe' if os.name == 'nt' else ''
flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
origin = 'https://localhost:5173'
if not (binaries / ('initdb' + extension)).is_file():
    raise SystemExit('PostgreSQL 18 binaries missing; run python .github/scripts/install-postgres.py')
if '--reset' in sys.argv and work.exists():
    data = work / 'data'
    if (data / 'postmaster.pid').exists():
        subprocess.run([str(binaries / ('pg_ctl' + extension)), '-D', str(data), '-m', 'fast', '-w', 'stop'], creationflags=flags)
    shutil.rmtree(work)
work.mkdir(parents=True, exist_ok=True)
secrets_file = work / 'secrets.json'
if secrets_file.exists():
    state = json.loads(secrets_file.read_text(encoding='utf-8'))
else:
    with socket.socket() as selector:
        selector.bind(('127.0.0.1', 0))
        port = selector.getsockname()[1]
    state = {key: secrets.token_urlsafe(24) for key in ('admin', 'migrator', 'runtime')} | {'port': port}
    secrets_file.write_text(json.dumps(state), encoding='utf-8')
port = state['port']
environment = {**os.environ, 'PGPASSWORD': state['admin']}

def pg(name, *args):
    subprocess.run([str(binaries / (name + extension)), *map(str, args)], env=environment, check=True, creationflags=flags)

data = work / 'data'
fresh = not data.exists()
if fresh:
    if os.name == 'nt':
        identity = subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], text=True, creationflags=flags)
        sid = next(csv.reader(identity.strip().splitlines()))[1]
        subprocess.run(['icacls', str(work), '/grant:r', f'*{sid}:(OI)(CI)F'], check=True, stdout=subprocess.DEVNULL, creationflags=flags)
    password_file = work / 'admin-password'
    password_file.write_text(state['admin'], encoding='utf-8')
    pg('initdb', '-D', data, '-U', 'midas_dev_admin', '--pwfile', password_file, '--auth-host=scram-sha-256',
       '--auth-local=scram-sha-256', '--encoding=UTF8', '--locale=C', '--no-instructions')
    password_file.unlink()
if not (data / 'postmaster.pid').exists():
    pg('pg_ctl', '-D', data, '-l', work / 'postgres.log', '-o', f'-h 127.0.0.1 -p {port}', '-w', 'start')
admin = ['-h', '127.0.0.1', '-p', port, '-U', 'midas_dev_admin', '-v', 'ON_ERROR_STOP=1']
if fresh:
    pg('createdb', *admin[:-2], 'midas')
    pg('psql', *admin, '-d', 'midas', '-f', root / 'server/database/bootstrap.sql')
    roles = work / 'roles.sql'
    roles.write_text(f"CREATE ROLE midas_dev_migrator LOGIN PASSWORD '{state['migrator']}'; GRANT midas_owner TO midas_dev_migrator;\n"
                     f"CREATE ROLE midas_dev_runtime LOGIN PASSWORD '{state['runtime']}'; GRANT midas_runtime TO midas_dev_runtime;\n",
                     encoding='utf-8')
    try:
        pg('psql', *admin, '-d', 'midas', '-f', roles)
    finally:
        roles.unlink()
base = f'127.0.0.1:{port}/midas'
local = {**os.environ, 'MIDAS_DATABASE_LOCAL_PLAINTEXT': '1'}
subprocess.run(['cargo', 'run', '--locked', '-q', '-p', 'midas-server', '--bin', 'midas-db'], cwd=root, check=True,
               env={**local, 'MIDAS_MIGRATION_DATABASE_URL': f"postgresql://midas_dev_migrator:{state['migrator']}@{base}"})
runtime_url = f"postgresql://midas_dev_runtime:{state['runtime']}@{base}"
if not (work / 'credentials.txt').exists():
    subprocess.run(['cargo', 'run', '--locked', '-q', '-p', 'midas-server', '--example', 'dev_seed'], cwd=root, check=True,
                   env={**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_DEV_DIR': str(work), 'MIDAS_AUTH_ORIGIN': origin})
print(f'Demo credentials: {work / "credentials.txt"}', flush=True)
print(f'Open {origin}', flush=True)
server = {**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_AUTH_ORIGIN': origin, 'MIDAS_AUTH_KEY_FILE': str(work / 'auth.key')}
panel = {**os.environ, 'MIDAS_DEV_TLS_CERT': str(work / 'tls.pem'), 'MIDAS_DEV_TLS_KEY': str(work / 'tls.key'),
         'MIDAS_DEV_API': 'https://127.0.0.1:7443'}
processes = [
    subprocess.Popen(['cargo', 'run', '--locked', '-q', '-p', 'midas-server', '--', '--tls-cert', str(work / 'tls.pem'),
                      '--tls-key', str(work / 'tls.key')], cwd=root, env=server),
    subprocess.Popen(['pnpm', '--filter', '@midas/web-panel', 'dev'], cwd=root, env=panel, shell=os.name == 'nt'),
]
try:
    while all(process.poll() is None for process in processes):
        try:
            processes[0].wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
finally:
    for process in processes:
        if process.poll() is None:
            process.terminate()
    pg('pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
