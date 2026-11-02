import hashlib
import os
import shutil
import subprocess
import tempfile

scanner = shutil.which("gitleaks")
if scanner is None:
    raise SystemExit("gitleaks is required")
environment = {key: value for key, value in os.environ.items() if not key.startswith("GITLEAKS_")}
synthetic = "ghp_" + hashlib.sha256(b"midas-scanner-negative-control").hexdigest()[:36]
with tempfile.TemporaryDirectory() as directory:
    command = [scanner, "stdin", "--redact", "--no-banner", "--no-color"]
    clean = subprocess.run(command, input="service_name = MIDAS\n", text=True, capture_output=True, cwd=directory, env=environment, timeout=30)
    detected = subprocess.run(command, input=f"token = {synthetic}\n", text=True, capture_output=True, cwd=directory, env=environment, timeout=30)
if clean.returncode != 0 or detected.returncode != 1:
    raise SystemExit("Secret scanner did not distinguish clean and synthetic input")
if synthetic in detected.stdout + detected.stderr:
    raise SystemExit("Secret scanner output was not redacted")
print("Clean input accepted; synthetic credential rejected with redacted output")
