"""Downloads with retries; release hosts sometimes time out or answer 5xx for minutes."""

import shutil
import time
import urllib.error
import urllib.request

ATTEMPTS = 5


def fetch(url, target=None):
    """Returns the body, or copies it into the open binary file `target`."""
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                if target is None:
                    return response.read()
                target.seek(0)
                target.truncate()
                shutil.copyfileobj(response, target)
                return None
        except OSError as error:
            permanent = isinstance(error, urllib.error.HTTPError) and error.code < 500 and error.code != 429
            if permanent or attempt == ATTEMPTS:
                raise
            delay = 10 * 2 ** (attempt - 1)
            print(f"download failed ({error}); retrying in {delay} s", flush=True)
            time.sleep(delay)
    return None
