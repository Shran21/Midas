"""Runs the relay trial against a real eturnal: the production template with a random secret,
the runner's own address as the only whitelisted peer, and the agent's TURN client in front of it."""

import os
import pathlib
import re
import secrets
import socket
import subprocess
import sys
import tempfile
import time

IMAGE = (
    "ghcr.io/processone/eturnal:1.12.3"
    "@sha256:88258657ba5e82bd52e58ad3735899ee1b4bddcec08a33aa73dee1671eb2c949"
)
TEMPLATE = pathlib.Path("server/turn/eturnal.yml")
NAME = "midas-turn-trial"


def runner_address() -> str:
    # A UDP connect only picks the outgoing interface; nothing is sent.
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    probe.connect(("192.0.2.1", 9))
    address = probe.getsockname()[0]
    probe.close()
    return address


address = runner_address()
secret = secrets.token_urlsafe(32)
config = TEMPLATE.read_text(encoding="utf-8")
config = re.sub(r'secret: ".*"', f'secret: "{secret}"', config, count=1)
config = re.sub(r'relay_ipv4_addr: ".*"', f'relay_ipv4_addr: "{address}"', config, count=1)
config = re.sub(r'  # relay_ipv6_addr: ".*"', '  relay_ipv6_addr: "::1"', config, count=1)
config = config.replace("  whitelist_peers: []", f'  whitelist_peers:\n    - "{address}/32"')
if config.count(secret) != 1 or address not in config or "::1" not in config:
    raise SystemExit("the template no longer has the expected fields")

directory = tempfile.mkdtemp()
path = pathlib.Path(directory, "eturnal.yml")
path.write_text(config, encoding="utf-8")
os.chmod(directory, 0o755)
os.chmod(path, 0o644)

subprocess.run(["docker", "rm", "-f", NAME], capture_output=True)
subprocess.run(
    [
        "docker", "run", "-d", "--name", NAME, "--network", "host",
        "-e", "STUN_SERVICE=false",
        "-v", f"{path}:/etc/eturnal.yml:ro",
        "--read-only", "--cap-drop=ALL", "--security-opt", "no-new-privileges",
        IMAGE,
    ],
    check=True,
)
try:
    time.sleep(3)
    environment = dict(
        os.environ,
        MIDAS_TURN_TRIAL_SERVER=f"{address}:3478",
        MIDAS_TURN_TRIAL_SERVER_V6="[::1]:3478",
        MIDAS_TURN_TRIAL_SECRET=secret,
        MIDAS_TURN_TRIAL_PEER=address,
    )
    trial = subprocess.run(
        [
            "cargo", "test", "--locked", "-p", "midas-agent-core", "--features", "webrtc",
            "--lib", "media::turn::tests::relay_trial", "--", "--ignored", "--exact", "--nocapture",
        ],
        env=environment,
    )
finally:
    logs = subprocess.run(["docker", "logs", NAME], capture_output=True, text=True)
    subprocess.run(["docker", "rm", "-f", NAME], capture_output=True)
if trial.returncode != 0:
    print(logs.stdout[-8000:], file=sys.stderr)
    print(logs.stderr[-8000:], file=sys.stderr)
    raise SystemExit("the relay trial failed")
