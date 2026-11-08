"""Builds the Windows agent MSI with WiX v3 and, on a disposable CI runner, installs it with a
canary token to show the token never reaches the MSI log or a process command line.

Run on CI only (`--install`); a development machine should only build the package.
"""
import argparse
import os
import pathlib
import secrets
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument('--install', action='store_true', help='install, inspect and remove the package (CI runner only)')
args = parser.parse_args()

root = pathlib.Path(__file__).resolve().parents[2]
work = root / 'target' / 'msi'
work.mkdir(parents=True, exist_ok=True)
wix = pathlib.Path(os.environ.get('WIX', r'C:\Program Files (x86)\WiX Toolset v3.14')) / 'bin'
if not (wix / 'candle.exe').is_file():
    raise SystemExit(f'WiX Toolset v3 not found at {wix}')
agent = root / 'target' / 'release' / 'midas-agent.exe'
if not agent.is_file():
    raise SystemExit('build the release agent first (just release)')
version = next(line.split('"')[1] for line in (root / 'Cargo.toml').read_text(encoding='utf-8').splitlines()
               if line.startswith('version = '))
source = root / 'agent' / 'agent-windows' / 'installer' / 'midas-agent.wxs'
package = work / 'midas-agent.msi'
subprocess.run([wix / 'candle.exe', '-nologo', '-arch', 'x64', '-ext', 'WixUtilExtension', f'-dVersion={version}',
                f'-dAgentPath={agent}', '-out', work / 'midas-agent.wixobj', source], check=True)
subprocess.run([wix / 'light.exe', '-nologo', '-ext', 'WixUtilExtension', '-out', package,
                work / 'midas-agent.wixobj'], check=True)
print(f'built {package} ({package.stat().st_size // 1024} KiB)')
if not args.install:
    sys.exit(0)
if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('--install changes the machine; it runs on CI runners only')

canary = 'midas-enroll-' + secrets.token_hex(32)
token_file = work / 'token.txt'
token_file.write_text(canary + '\n', encoding='ascii')
ca = work / 'server-ca.pem'
ca.write_text('-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ0FOQVJZ\n-----END CERTIFICATE-----\n', encoding='ascii')
log = work / 'install.log'
result = subprocess.run(['msiexec', '/i', str(package), '/qn', '/l*v', str(log), 'MIDAS_SERVER=127.0.0.1',
                         'MIDAS_PORT=1', 'MIDAS_ENROLLMENT_PORT=1', f'MIDAS_SERVER_CA={ca}',
                         f'MIDAS_TOKEN_FILE={token_file}'])
text = log.read_bytes()
decoded = text.decode('utf-16', errors='ignore') if text[:2] == b'\xff\xfe' else text.decode('utf-8', errors='ignore')
if result.returncode != 0:
    print(decoded[-6000:])
    raise SystemExit(f'msiexec failed: {result.returncode}')
assert canary not in decoded and canary[13:] not in decoded, 'the token reached the MSI log'
assert str(token_file) in decoded, 'the log should name the token file, which is not secret'
data = pathlib.Path(os.environ['ProgramData']) / 'MIDAS' / 'agent'
config = (data / 'config.json').read_text(encoding='utf-8')
assert '127.0.0.1' in config and canary not in config, 'config written without the token'
staged = (data / 'enrollment-token').read_text(encoding='ascii').strip()
assert staged == canary, 'the token is staged in the protected data directory'
acl = subprocess.run(['icacls', str(data / 'enrollment-token')], capture_output=True, text=True, check=True).stdout
assert 'Users' not in acl and 'Everyone' not in acl, acl
for _ in range(30):
    state = subprocess.run(['sc', 'query', 'MidasAgent'], capture_output=True, text=True).stdout
    if 'RUNNING' in state:
        break
    time.sleep(1)
assert 'RUNNING' in state, state
lines = subprocess.run(['powershell', '-NoProfile', '-Command',
                        "Get-CimInstance Win32_Process | ForEach-Object { $_.CommandLine }"],
                       capture_output=True, text=True, check=True).stdout
assert canary not in lines and canary[13:] not in lines, 'the token appeared in a process command line'
removal = subprocess.run(['msiexec', '/x', str(package), '/qn', '/l*v', str(work / 'remove.log')])
assert removal.returncode == 0, removal.returncode
# 1060: the service does not exist any more; the data directory is kept on purpose.
assert subprocess.run(['sc', 'query', 'MidasAgent'], capture_output=True).returncode == 1060
print('MSI installed with a staged token that stayed out of the log and command lines, then removed')
