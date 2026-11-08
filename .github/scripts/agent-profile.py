"""Agent resource profile: the real Windows agent and tray program against a local release server.

Runs the agent in console mode with its own data directory, the tray program beside it, and samples
CPU, memory, handles, threads, disk I/O and network traffic of every MIDAS process, including any
WebView2 processes they start. Traffic is counted by a local relay between the agent and the
server. The tray status view is opened at set times, so background and open-interface use can be
told apart. Local measurement tool: it contacts only 127.0.0.1, installs nothing and changes no
system setting. Requires `python -m pip install psutil`.
"""
import argparse
import csv
import ctypes
import datetime
import hashlib
import json
import os
import pathlib
import secrets
import shutil
import socket
import statistics
import subprocess
import sys
import threading
import time

import psutil

parser = argparse.ArgumentParser()
parser.add_argument('--hours', type=float, default=24)
parser.add_argument('--sample', type=int, default=60, help='seconds between samples')
parser.add_argument('--view-every', type=float, default=6, help='hours between opening the tray status view')
parser.add_argument('--view-minutes', type=float, default=10, help='how long the status view stays open')
parser.add_argument('--summarize', action='store_true', help='only rewrite results.json from the saved samples')
args = parser.parse_args()

if os.name != 'nt':
    raise SystemExit('the agent profile measures the Windows agent and runs on Windows only')
sys.stdout.reconfigure(encoding='utf-8')
root = pathlib.Path(__file__).resolve().parents[2]
work = root / '.local' / 'profile'
flags = subprocess.CREATE_NO_WINDOW
binaries = pathlib.Path(os.environ.get('MIDAS_TEST_POSTGRES_BIN', root / '.local/tools/postgres-18.6/pgsql/bin')).resolve()
release = root / 'target' / 'release'
origin = 'https://localhost:5173'
user32 = ctypes.windll.user32
user32.FindWindowW.restype = ctypes.c_void_p
user32.FindWindowW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t]
WM_CLOSE, WM_LBUTTONUP, TRAY_CALLBACK = 0x0010, 0x0202, 0x8000 + 1


def log(message):
    print(f'[{datetime.datetime.now():%H:%M:%S}] {message}', flush=True)


def free_port():
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        return probe.getsockname()[1]


class Relay:
    """Forwards the agent's TLS connection unchanged and counts the bytes in each direction."""

    def __init__(self, target):
        self.target = target
        self.listener = socket.create_server(('127.0.0.1', 0))
        self.port = self.listener.getsockname()[1]
        self.lock = threading.Lock()
        self.up = self.down = self.connections = 0
        threading.Thread(target=self.accept, daemon=True).start()

    def accept(self):
        while True:
            client, _ = self.listener.accept()
            try:
                server = socket.create_connection(('127.0.0.1', self.target))
            except OSError:
                client.close()
                continue
            with self.lock:
                self.connections += 1
            threading.Thread(target=self.pump, args=(client, server, 'up'), daemon=True).start()
            threading.Thread(target=self.pump, args=(server, client, 'down'), daemon=True).start()

    def pump(self, source, sink, direction):
        try:
            while data := source.recv(65536):
                sink.sendall(data)
                with self.lock:
                    setattr(self, direction, getattr(self, direction) + len(data))
        except OSError:
            pass
        finally:
            for end in (source, sink):
                try:
                    end.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                end.close()

    def totals(self):
        with self.lock:
            return self.up, self.down, self.connections


samples_file = work / 'samples.jsonl'
views_file = work / 'views.json'


def summarize():
    """Writes results.json from the samples; also usable after an interrupted run."""
    lines = [json.loads(line) for line in samples_file.read_text(encoding='utf-8').splitlines()]
    views = json.loads(views_file.read_text(encoding='utf-8')) if views_file.exists() else []

    def rates(role, condition=lambda sample: True):
        rows = [sample for sample in lines if sample[role] and condition(sample)]
        cpu = [(b[role]['cpu_seconds'] - a[role]['cpu_seconds']) / (b['at'] - a['at']) * 100
               for a, b in zip(rows, rows[1:]) if b['at'] > a['at'] and b['hour'] - a['hour'] < 2 * args.sample / 3600]
        if not rows:
            return None
        pick = lambda key: [sample[role][key] for sample in rows]
        def stats(values, scale=1):
            ordered = sorted(values)
            return {'median': round(statistics.median(ordered) / scale, 2),
                    'p95': round(ordered[max(0, round(len(ordered) * 0.95) - 1)] / scale, 2),
                    'max': round(ordered[-1] / scale, 2)}
        return {
            'samples': len(rows),
            'cpu_percent_of_core': stats(cpu) if cpu else None,
            'cpu_seconds_total': round(rows[-1][role]['cpu_seconds'] - rows[0][role]['cpu_seconds'], 1),
            'working_set_mib': stats(pick('working_set'), 2**20),
            'private_mib': stats(pick('private'), 2**20),
            'handles': {'first': rows[0][role]['handles'], 'last': rows[-1][role]['handles'], 'max': max(pick('handles'))},
            'threads': {'first': rows[0][role]['threads'], 'last': rows[-1][role]['threads'], 'max': max(pick('threads'))},
            'disk_write_mib': round((rows[-1][role]['io_write'] - rows[0][role]['io_write']) / 2**20, 2),
            'disk_read_mib': round((rows[-1][role]['io_read'] - rows[0][role]['io_read']) / 2**20, 2),
        }

    hours = lines[-1]['hour'] - lines[0]['hour'] if len(lines) > 1 else 0
    traffic = lines[-1]['traffic'] if lines else {'up': 0, 'down': 0, 'connections': 0}
    results = {
        'hours': round(hours, 3), 'sample_seconds': args.sample, 'samples': len(lines),
        'agent': rates('agent'),
        'tray_view_closed': rates('tray', lambda sample: not sample['view_open']),
        'tray_view_open': rates('tray', lambda sample: sample['view_open']),
        'webview_samples': sum(1 for sample in lines if sample['webview']),
        'webview': rates('webview'),
        'traffic': {'up_bytes': traffic['up'], 'down_bytes': traffic['down'], 'connections': traffic['connections'],
                    'up_kib_per_hour': round(traffic['up'] / 1024 / hours, 1) if hours else None,
                    'down_kib_per_hour': round(traffic['down'] / 1024 / hours, 1) if hours else None},
        'online_share': round(sum(1 for s in lines if s['device'] and s['device']['online']) / len(lines), 4) if lines else None,
        'mains_share': round(sum(1 for s in lines if s['mains']) / len(lines), 4) if lines else None,
        'device': lines[-1]['device'] if lines else None,
        'views': views,
        'machine': {'cpus': os.cpu_count(), 'platform': sys.platform},
    }
    (work / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
    log(f'results written to {work / "results.json"}')


if args.summarize:
    summarize()
    raise SystemExit(0)
subprocess.run(['cargo', 'build', '--release', '--locked', '-q', '-p', 'midas-server', '--bin', 'midas-server',
                '--bin', 'midas-db', '--example', 'dev_seed', '--example', 'dev_fleet', '-p', 'midas-agent-windows',
                '--bin', 'midas-agent', '--bin', 'midas-tray'], cwd=root, check=True)
if work.exists():
    data = work / 'data'
    if (data / 'postmaster.pid').exists():
        subprocess.run([str(binaries / 'pg_ctl.exe'), '-D', str(data), '-m', 'fast', '-w', 'stop'], creationflags=flags)
    shutil.rmtree(work)
work.mkdir(parents=True)
identity = subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], text=True, creationflags=flags)
sid = next(csv.reader(identity.strip().splitlines()))[1]
subprocess.run(['icacls', str(work), '/grant:r', f'*{sid}:(OI)(CI)F'], check=True, stdout=subprocess.DEVNULL,
               creationflags=flags)
state = {key: secrets.token_urlsafe(24) for key in ('admin', 'migrator', 'runtime')}
port, web, agent_port = free_port(), free_port(), free_port()
pg_env = {**os.environ, 'PGPASSWORD': state['admin'], 'PGCLIENTENCODING': 'UTF8'}
admin = ['-h', '127.0.0.1', '-p', port, '-U', 'midas_profile_admin', '-v', 'ON_ERROR_STOP=1', '-d', 'midas']


def pg(name, *arguments, capture=False):
    result = subprocess.run([str(binaries / (name + '.exe')), *map(str, arguments)], env=pg_env, creationflags=flags,
                            capture_output=capture, text=True, encoding='utf-8')
    if result.returncode != 0:
        raise SystemExit(f'{name} failed: {(result.stderr or "").strip()}')
    return result.stdout


def sql(query):
    script = work / 'query.sql'
    script.write_text(query, encoding='utf-8')
    return pg('psql', *admin, '-qAt', '-f', script, capture=True).strip()


password_file = work / 'admin-password'
password_file.write_text(state['admin'], encoding='utf-8')
data = work / 'data'
pg('initdb', '-D', data, '-U', 'midas_profile_admin', '--pwfile', password_file, '--auth-host=scram-sha-256',
   '--auth-local=scram-sha-256', '--encoding=UTF8', '--locale=C', '--no-instructions')
password_file.unlink()
pg('pg_ctl', '-D', data, '-l', work / 'postgres.log', '-o', f'-h 127.0.0.1 -p {port}', '-w', 'start')
# Without this an idle machine may sleep during the run; it holds only while this process lives.
ctypes.windll.kernel32.SetThreadExecutionState(0x80000000 | 0x00000001)
agent = tray = server = None
try:
    pg('createdb', *admin[:-4], 'midas')
    pg('psql', *admin, '-f', root / 'server/database/bootstrap.sql')
    roles = work / 'roles.sql'
    roles.write_text(f"CREATE ROLE midas_profile_migrator LOGIN PASSWORD '{state['migrator']}'; GRANT midas_owner TO midas_profile_migrator;\n"
                     f"CREATE ROLE midas_profile_runtime LOGIN PASSWORD '{state['runtime']}'; GRANT midas_runtime TO midas_profile_runtime;\n",
                     encoding='utf-8')
    pg('psql', *admin, '-f', roles)
    roles.unlink()
    base = f'127.0.0.1:{port}/midas'
    local = {**os.environ, 'MIDAS_DATABASE_LOCAL_PLAINTEXT': '1'}
    runtime_url = f"postgresql://midas_profile_runtime:{state['runtime']}@{base}"
    subprocess.run([release / 'midas-db.exe'], check=True,
                   env={**local, 'MIDAS_MIGRATION_DATABASE_URL': f"postgresql://midas_profile_migrator:{state['migrator']}@{base}"})
    subprocess.run([release / 'examples' / 'dev_seed.exe'], check=True, stdout=subprocess.DEVNULL,
                   env={**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_DEV_DIR': str(work), 'MIDAS_AUTH_ORIGIN': origin})
    subprocess.run([release / 'examples' / 'dev_fleet.exe', 'ca'], check=True, stdout=subprocess.DEVNULL,
                   env={**local, 'MIDAS_DEV_DIR': str(work)})
    (work / 'credentials.txt').unlink(missing_ok=True)
    server = subprocess.Popen(
        [release / 'midas-server.exe', '--bind', f'127.0.0.1:{web}', '--tls-cert', work / 'tls.pem', '--tls-key',
         work / 'tls.key', '--agent-bind', f'127.0.0.1:{agent_port}', '--agent-ca', work / 'agent-ca.pem',
         '--agent-ca-key', work / 'agent-ca-key.pem'],
        env={**local, 'MIDAS_DATABASE_URL': runtime_url, 'MIDAS_AUTH_ORIGIN': origin,
             'MIDAS_AUTH_KEY_FILE': str(work / 'auth.key'), 'RUST_LOG': 'warn'},
        stdout=open(work / 'server.log', 'ab'), stderr=subprocess.STDOUT, creationflags=flags)
    relay = Relay(agent_port)

    name = 'MIDAS-PROFILE'
    tenant = sql("SELECT id FROM midas.tenants ORDER BY name LIMIT 1")
    token = 'midas-enroll-' + secrets.token_hex(32)
    digest = hashlib.sha256(b'enrollment-token\0' + token.encode()).hexdigest()
    sql("INSERT INTO midas.enrollment_tokens (digest, id, tenant_id, platform, kind, preapproved, expires_at) "
        f"VALUES ('\\x{digest}', gen_random_uuid(), '{tenant}', 'windows', 'workstation', true, "
        "clock_timestamp() + interval '30 minutes')")
    agent_data = work / 'agent'
    agent_data.mkdir()
    (agent_data / 'config.json').write_text(json.dumps({
        'server': '127.0.0.1', 'port': relay.port, 'enrollment_port': web,
        'server_ca': (work / 'tls.pem').read_text(encoding='utf-8'), 'kind': 'workstation', 'name': name,
    }), encoding='utf-8')
    (agent_data / 'enrollment-token').write_text(token, encoding='utf-8')
    for _ in range(120):
        try:
            with socket.create_connection(('127.0.0.1', web), timeout=1):
                break
        except OSError:
            time.sleep(1)

    agent = subprocess.Popen([release / 'midas-agent.exe', 'run', '--data', agent_data],
                             stdout=open(work / 'agent.log', 'ab'), stderr=subprocess.STDOUT, creationflags=flags,
                             env={**os.environ, 'RUST_LOG': 'info'})
    tray = subprocess.Popen([release / 'midas-tray.exe'])
    log(f'agent {agent.pid}, tray {tray.pid}; sampling every {args.sample} s for {args.hours} h')

    def device():
        row = sql(f"SELECT online, coalesce(array_to_string(capabilities, ' '), ''), "
                  f"(SELECT count(*) FROM midas.device_metrics m WHERE m.device_id = d.id), "
                  f"(SELECT count(*) FROM midas.device_inventory i WHERE i.device_id = d.id), "
                  f"(SELECT count(*) FROM midas.device_health h WHERE h.device_id = d.id) "
                  f"FROM midas.devices d WHERE d.name = '{name}' AND d.enrollment = 'active'")
        if not row:
            return None
        online, capabilities, metrics, inventory, health = row.split('|')
        return {'online': online == 't', 'capabilities': capabilities.split(), 'metric_rows': int(metrics),
                'inventory_rows': int(inventory), 'health_rows': int(health)}

    def webviews(owners):
        found = []
        for process in psutil.process_iter(['name']):
            if (process.info['name'] or '').lower() != 'msedgewebview2.exe':
                continue
            try:
                if any(parent.pid in owners for parent in process.parents()):
                    found.append(process)
            except psutil.Error:
                pass
        return found

    def measure(process):
        try:
            with process.oneshot():
                times = process.cpu_times()
                memory = process.memory_info()
                io = process.io_counters()
                return {'cpu_seconds': times.user + times.system, 'working_set': memory.rss,
                        'private': memory.private, 'peak_working_set': memory.peak_wset,
                        'handles': process.num_handles(), 'threads': process.num_threads(),
                        'io_read': io.read_bytes, 'io_write': io.write_bytes, 'io_other': io.other_bytes}
        except psutil.Error:
            return None

    def total(processes):
        parts = [part for part in map(measure, processes) if part]
        if not parts:
            return None
        return {key: sum(part[key] for part in parts) for key in parts[0]} | {'processes': len(parts)}

    def view(open_it):
        window = user32.FindWindowW('MidasAgentTray', 'MidasAgentTray')
        if not window:
            return False
        if open_it:
            return bool(user32.PostMessageW(window, TRAY_CALLBACK, 0, WM_LBUTTONUP))
        for caption in ('MIDAS agent állapota', 'MIDAS agent status'):
            box = user32.FindWindowW('#32770', caption)
            if box:
                user32.PostMessageW(box, WM_CLOSE, 0, 0)
                return True
        return False

    started = time.time()
    count, last_device = 0, None
    views, view_open_until = [], None
    next_view = started + min(3600, args.view_every * 3600)
    while time.time() - started < args.hours * 3600:
        time.sleep(args.sample)
        now = time.time()
        if agent.poll() is not None:
            log(f'the agent exited with {agent.returncode}')
            break
        if view_open_until is None and now >= next_view:
            opened = view(True)
            views.append({'opened_at_hour': round((now - started) / 3600, 3), 'opened': opened})
            view_open_until = now + args.view_minutes * 60
            log(f'tray status view opened: {opened}')
        elif view_open_until is not None and now >= view_open_until:
            views[-1]['closed'] = view(False)
            view_open_until = None
            next_view = now + args.view_every * 3600
        owners = {agent.pid, tray.pid}
        battery = psutil.sensors_battery()
        up, down, connections = relay.totals()
        sample = {
            'at': round(now, 1), 'hour': round((now - started) / 3600, 4), 'view_open': view_open_until is not None,
            'agent': total([psutil.Process(agent.pid)]),
            'tray': total([psutil.Process(tray.pid)]) if tray.poll() is None else None,
            'webview': total(webviews(owners)),
            'traffic': {'up': up, 'down': down, 'connections': connections},
            'mains': None if battery is None else battery.power_plugged,
            'battery_percent': None if battery is None else battery.percent,
            'system_cpu_percent': psutil.cpu_percent(interval=None),
            'cpu_mhz': round(psutil.cpu_freq().current),
        }
        if count % 5 == 0:
            last_device = device()
        sample['device'] = last_device
        count += 1
        with samples_file.open('a', encoding='utf-8') as out:
            out.write(json.dumps(sample) + '\n')
        if int((now - started) / args.sample) % 60 == 0:
            log(json.dumps({key: sample[key] for key in ('hour', 'agent', 'tray', 'traffic', 'mains')}))

    (work / 'views.json').write_text(json.dumps(views), encoding='utf-8')
    summarize()
finally:
    if tray and tray.poll() is None:
        window = user32.FindWindowW('MidasAgentTray', 'MidasAgentTray')
        if window:
            user32.PostMessageW(window, WM_CLOSE, 0, 0)
        try:
            tray.wait(timeout=10)
        except subprocess.TimeoutExpired:
            tray.kill()
    for process in (agent, server):
        if process and process.poll() is None:
            process.kill()
    pg('pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
