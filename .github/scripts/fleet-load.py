"""Fleet load trial: many simulated agents against a release server on a throwaway local database.

Measures server and database load, device-list latency, a reconnection wave after a server
restart, and tenant isolation. Local measurement tool; it never contacts anything but 127.0.0.1.
"""
import argparse
import csv
import datetime
import http.client
import json
import os
import pathlib
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--agents', type=int, default=1000)
parser.add_argument('--tenants', type=int, default=10)
parser.add_argument('--share', type=int, default=50, help='percent of agents in the first tenant')
parser.add_argument('--minutes', type=float, default=60)
parser.add_argument('--wave-at', type=float, default=45, help='minute of the server restart')
parser.add_argument('--sample', type=int, default=30, help='seconds between samples')
args = parser.parse_args()

sys.stdout.reconfigure(encoding='utf-8')
root = pathlib.Path(__file__).resolve().parents[2]
work = root / '.local' / 'load'
windows = os.name == 'nt'
extension = '.exe' if windows else ''
flags = subprocess.CREATE_NO_WINDOW if windows else 0
binaries = pathlib.Path(os.environ.get('MIDAS_TEST_POSTGRES_BIN', root / '.local/tools/postgres-18.6/pgsql/bin')).resolve()
release = root / 'target' / 'release'
origin = 'https://localhost:5173'


def free_port():
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        return probe.getsockname()[1]


def log(message):
    print(f'[{datetime.datetime.now():%H:%M:%S}] {message}', flush=True)


subprocess.run(['cargo', 'build', '--release', '--locked', '-q', '-p', 'midas-server', '--bin', 'midas-server',
                '--bin', 'midas-db', '--example', 'dev_seed', '--example', 'dev_fleet'], cwd=root, check=True)
if work.exists():
    data = work / 'data'
    if (data / 'postmaster.pid').exists():
        subprocess.run([str(binaries / ('pg_ctl' + extension)), '-D', str(data), '-m', 'fast', '-w', 'stop'],
                       creationflags=flags)
    shutil.rmtree(work)
work.mkdir(parents=True)
if windows:
    identity = subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], text=True, creationflags=flags)
    sid = next(csv.reader(identity.strip().splitlines()))[1]
    subprocess.run(['icacls', str(work), '/grant:r', f'*{sid}:(OI)(CI)F'], check=True, stdout=subprocess.DEVNULL,
                   creationflags=flags)
state = {key: secrets.token_urlsafe(24) for key in ('admin', 'migrator', 'runtime')}
port, web, agent_port = free_port(), free_port(), free_port()
pg_env = {**os.environ, 'PGPASSWORD': state['admin'], 'PGCLIENTENCODING': 'UTF8'}


def pg(name, *arguments, capture=False):
    result = subprocess.run([str(binaries / (name + extension)), *map(str, arguments)], env=pg_env,
                            creationflags=flags, capture_output=capture, text=True, encoding='utf-8')
    if result.returncode != 0:
        raise SystemExit(f'{name} failed: {(result.stderr or "").strip()}')
    return result.stdout


admin = ['-h', '127.0.0.1', '-p', port, '-U', 'midas_load_admin', '-v', 'ON_ERROR_STOP=1', '-d', 'midas']


def sql(query):
    # A file keeps non-ASCII text intact regardless of the console code page.
    script = work / 'query.sql'
    script.write_text(query, encoding='utf-8')
    return pg('psql', *admin, '-qAt', '-f', script, capture=True).strip()


password_file = work / 'admin-password'
password_file.write_text(state['admin'], encoding='utf-8')
data = work / 'data'
pg('initdb', '-D', data, '-U', 'midas_load_admin', '--pwfile', password_file, '--auth-host=scram-sha-256',
   '--auth-local=scram-sha-256', '--encoding=UTF8', '--locale=C', '--no-instructions')
password_file.unlink()
pg('pg_ctl', '-D', data, '-l', work / 'postgres.log', '-o', f'-h 127.0.0.1 -p {port} -N 300', '-w', 'start')
try:
    pg('createdb', *admin[:-4], 'midas')
    pg('psql', *admin, '-f', root / 'server/database/bootstrap.sql')
    roles = work / 'roles.sql'
    roles.write_text(f"CREATE ROLE midas_load_migrator LOGIN PASSWORD '{state['migrator']}'; GRANT midas_owner TO midas_load_migrator;\n"
                     f"CREATE ROLE midas_load_runtime LOGIN PASSWORD '{state['runtime']}'; GRANT midas_runtime TO midas_load_runtime;\n",
                     encoding='utf-8')
    pg('psql', *admin, '-f', roles)
    roles.unlink()
    base = f'127.0.0.1:{port}/midas'
    local = {**os.environ, 'MIDAS_DATABASE_LOCAL_PLAINTEXT': '1'}
    runtime_url = f"postgresql://midas_load_runtime:{state['runtime']}@{base}"
    subprocess.run([release / ('midas-db' + extension)], check=True,
                   env={**local, 'MIDAS_MIGRATION_DATABASE_URL': f"postgresql://midas_load_migrator:{state['migrator']}@{base}"})
    subprocess.run([release / 'examples' / ('dev_seed' + extension)], check=True,
                   env={**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_DEV_DIR': str(work), 'MIDAS_AUTH_ORIGIN': origin})
    fleet_binary = release / 'examples' / ('dev_fleet' + extension)
    subprocess.run([fleet_binary, 'ca'], check=True, env={**local, 'MIDAS_DEV_DIR': str(work)})
    existing = int(sql('SELECT count(*) FROM midas.tenants'))
    for n in range(existing, args.tenants):
        tenant = uuid.uuid4()
        sql(f"INSERT INTO midas.tenants (id, name) VALUES ('{tenant}', 'Terhelés {n + 1:02d}'); "
            f"INSERT INTO midas.sites (tenant_id, id, name) VALUES ('{tenant}', '{uuid.uuid4()}', 'Telephely {n + 1:02d}')")
    server_env = {**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_AUTH_ORIGIN': origin,
                  'MIDAS_AUTH_KEY_FILE': str(work / 'auth.key'), 'RUST_LOG': 'warn'}
    server_command = [release / ('midas-server' + extension), '--bind', f'127.0.0.1:{web}', '--tls-cert', work / 'tls.pem',
                      '--tls-key', work / 'tls.key', '--agent-bind', f'127.0.0.1:{agent_port}', '--agent-ca',
                      work / 'agent-ca.pem', '--agent-ca-key', work / 'agent-ca-key.pem']

    def start_server():
        return subprocess.Popen(server_command, env=server_env, stdout=open(work / 'server.log', 'ab'),
                                stderr=subprocess.STDOUT)

    server = start_server()
    context = ssl.create_default_context(cafile=str(work / 'tls.pem'))
    cookie = {'value': None}

    def call(method, path, body=None):
        # The address avoids the IPv6 attempt Windows makes for localhost before falling back.
        connection = http.client.HTTPSConnection('127.0.0.1', web, context=context, timeout=30)
        headers = {'origin': origin}
        if body is not None:
            headers['content-type'] = 'application/json'
        if cookie['value']:
            headers['cookie'] = cookie['value']
        started = time.perf_counter()
        connection.request(method, path, body=None if body is None else json.dumps(body), headers=headers)
        response = connection.getresponse()
        raw = response.read()
        elapsed = (time.perf_counter() - started) * 1000
        for name, value in response.getheaders():
            if name.lower() == 'set-cookie' and value.startswith('__Host-midas-session='):
                cookie['value'] = value.split(';', 1)[0]
        return response.status, (json.loads(raw) if raw else None), elapsed, len(raw)

    for _ in range(120):
        try:
            if call('GET', '/health/live')[0] == 200:
                break
        except OSError:
            time.sleep(1)
    codes = (work / 'credentials.txt').read_text(encoding='utf-8').split('admin (', 1)[1].splitlines()[1:11]
    for code in codes:
        status, body, _, _ = call('POST', '/api/auth/recovery', {'login': 'admin', 'code': code.strip()})
        if status == 200:
            break
    tenants = {item['name']: item['id'] for item in call('GET', '/api/tenants')[1]}
    first = tenants[min(tenants)]
    fleet = subprocess.Popen([fleet_binary, 'run'], stdout=open(work / 'fleet.log', 'ab'), stderr=subprocess.STDOUT,
                             env={**local, 'MIDAS_DEV_DIR': str(work), 'MIDAS_FLEET_SIZE': str(args.agents),
                                  'MIDAS_FLEET_FIRST_SHARE': str(args.share), 'MIDAS_FLEET_WEB_PORT': str(web),
                                  'MIDAS_FLEET_AGENT_PORT': str(agent_port), 'RUST_LOG': 'warn',
                                  'MIDAS_ADMIN_DATABASE_URL': f"postgresql://midas_load_admin:{state['admin']}@{base}"})

    def usage():
        """CPU seconds and resident memory of the server, the database and the load generator."""
        if windows:
            script = (f"$s = Get-Process -Id {server.pid}; $f = Get-Process -Id {fleet.pid}; "
                      f"$p = Get-Process postgres | Where-Object {{ $_.Path -like '{binaries}*' }} | Measure-Object -Property CPU,WorkingSet64 -Sum; "
                      "$c = [Globalization.CultureInfo]::InvariantCulture; "
                      "(@($s.CPU, $s.WorkingSet64, $p[0].Sum, $p[1].Sum, $f.CPU, $f.WorkingSet64) | ForEach-Object { ([double]$_).ToString($c) }) -join ';'")
            values = subprocess.check_output(['powershell', '-NoProfile', '-Command', script], text=True,
                                             creationflags=flags).strip().split(';')
            return [float(value or 0) for value in values]
        ticks = os.sysconf('SC_CLK_TCK')

        def stat(pid):
            fields = pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
            rss = int(pathlib.Path(f'/proc/{pid}/statm').read_text().split()[1]) * os.sysconf('SC_PAGE_SIZE')
            return (int(fields[11]) + int(fields[12])) / ticks, rss
        postgres = [int(p) for p in subprocess.check_output(['pgrep', '-f', str(binaries / 'postgres')], text=True).split()]
        sums = [sum(values) for values in zip(*(stat(pid) for pid in postgres))]
        return [*stat(server.pid), *sums, *stat(fleet.pid)]

    samples = []
    started = time.monotonic()
    previous = (started, usage())
    wave = None
    connected_at = None
    while (elapsed := time.monotonic() - started) < args.minutes * 60:
        time.sleep(args.sample)
        now = time.monotonic()
        if wave is None and now - started >= args.wave_at * 60:
            log('restarting the server for the reconnection wave')
            expected = int(sql('SELECT count(*) FROM midas.devices WHERE online'))
            server.terminate()
            server.wait(timeout=60)
            stopped = time.monotonic()
            server = start_server()
            restart = sql('SELECT clock_timestamp()')
            for _ in range(60):
                try:
                    if call('GET', '/health/live')[0] == 200:
                        break
                except OSError:
                    time.sleep(0.5)
            back = None
            while time.monotonic() - stopped < 600:
                seen = int(sql(f"SELECT count(*) FROM midas.devices WHERE last_seen_at > '{restart}'"))
                if seen >= expected:
                    back = time.monotonic() - stopped
                    break
                time.sleep(1)
            wave = {'down_and_back_seconds': back, 'reconnected': seen, 'online_before': expected}
            log(f'reconnection wave: {wave}')
            previous = (time.monotonic(), usage())
            continue
        current = usage()
        interval = now - previous[0]
        status, devices, list_ms, list_bytes = call('GET', f'/api/tenants/{first}/devices')
        counts = sql("SELECT count(*) FILTER (WHERE online), count(*), (SELECT count(*) FROM midas.device_metrics), "
                     "(SELECT count(*) FROM midas.device_inventory), (SELECT count(*) FROM midas.device_health), "
                     "pg_database_size('midas') FROM midas.devices WHERE enrollment = 'active'").split('|')
        if connected_at is None and int(counts[0]) >= args.agents:
            connected_at = now - started
        sample = {
            'minute': round((now - started) / 60, 2),
            'online': int(counts[0]),
            'enrolled': int(counts[1]),
            'metric_rows': int(counts[2]),
            'inventory_rows': int(counts[3]),
            'health_rows': int(counts[4]),
            'database_mib': round(int(counts[5]) / 2**20, 1),
            'server_cpu_percent': round((current[0] - previous[1][0]) / interval * 100, 1),
            'server_rss_mib': round(current[1] / 2**20, 1),
            'postgres_cpu_percent': round((current[2] - previous[1][2]) / interval * 100, 1),
            'postgres_rss_mib': round(current[3] / 2**20, 1),
            'fleet_cpu_percent': round((current[4] - previous[1][4]) / interval * 100, 1),
            'fleet_rss_mib': round(current[5] / 2**20, 1),
            'list_status': status,
            'list_devices': len(devices) if isinstance(devices, list) else None,
            'list_ms': round(list_ms, 1),
            'list_kib': round(list_bytes / 1024, 1),
        }
        samples.append(sample)
        previous = (now, current)
        log(json.dumps(sample))

    # Isolation: the runtime role sees one tenant at a time, and the API answers only for members.
    isolation = []
    tenant_rows = sql('SELECT id, name, (SELECT count(*) FROM midas.devices d WHERE d.tenant_id = t.id) FROM midas.tenants t ORDER BY name')
    runtime = ['-h', '127.0.0.1', '-p', port, '-U', 'midas_load_runtime', '-d', 'midas', '-qAt', '-v', 'ON_ERROR_STOP=1']
    runtime_env = {**os.environ, 'PGPASSWORD': state['runtime'], 'PGCLIENTENCODING': 'UTF8'}
    for line in tenant_rows.splitlines():
        tenant, name, total = line.split('|')
        query = (f"BEGIN; SELECT set_config('midas.tenant_id', '{tenant}', true); "
                 "SELECT count(*), count(DISTINCT tenant_id) FROM midas.devices; "
                 "SELECT count(DISTINCT tenant_id) FROM midas.device_metrics; "
                 "SELECT count(DISTINCT tenant_id) FROM midas.device_inventory; COMMIT;")
        output = subprocess.run([str(binaries / ('psql' + extension)), *map(str, runtime), '-c', query], env=runtime_env,
                                capture_output=True, text=True, check=True, creationflags=flags).stdout.split()
        visible, tenants_seen, metric_tenants, inventory_tenants = output[1].split('|') + output[2:4]
        status = call('GET', f'/api/tenants/{tenant}/devices')[0]
        isolation.append({'tenant': name, 'devices': int(total), 'visible_under_rls': int(visible),
                          'distinct_tenants': int(tenants_seen), 'metric_tenants': int(metric_tenants),
                          'inventory_tenants': int(inventory_tenants), 'api_status': status})
    fleet.terminate()
    server.terminate()
    results = {
        'agents': args.agents, 'tenants': args.tenants, 'first_share_percent': args.share, 'minutes': args.minutes,
        'all_connected_after_seconds': connected_at, 'wave': wave, 'samples': samples, 'isolation': isolation,
        'machine': {'cpus': os.cpu_count(), 'platform': sys.platform},
    }
    (work / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
    log(f'results written to {work / "results.json"}')
finally:
    for process in ('fleet', 'server'):
        if process in dir() and globals()[process].poll() is None:
            globals()[process].kill()
    pg('pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
