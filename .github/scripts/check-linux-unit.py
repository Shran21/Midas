"""Fails when the Linux agent unit's systemd exposure score rises above the limit."""

import pathlib
import re
import subprocess
import sys

LIMIT = 3.0
UNIT = pathlib.Path("agent/agent-linux/packaging/midas-agent.service")

result = subprocess.run(
    ["systemd-analyze", "security", "--offline=yes", "--no-pager", str(UNIT)],
    capture_output=True,
    text=True,
)
print(result.stdout)
print(result.stderr, file=sys.stderr)
match = re.search(r"Overall exposure level for \S+: (\d+(?:\.\d+)?)", result.stdout + result.stderr)
if not match:
    raise SystemExit("systemd-analyze did not report an exposure level")
score = float(match.group(1))
print(f"exposure {score} (limit {LIMIT})")
if score > LIMIT:
    raise SystemExit(f"unit exposure {score} exceeds {LIMIT}")
binary = pathlib.Path("target/release/midas-agent-linux")
started = subprocess.run([str(binary), "--help"], capture_output=True, text=True)
if started.returncode != 0 or "enroll" not in started.stdout:
    raise SystemExit("the release agent binary does not start")
