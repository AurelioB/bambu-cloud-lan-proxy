"""Interactive, local-only sign-in CLI; tokens never enter shell history."""

import getpass
import json
import os
import urllib.request


def request(path: str, body: dict | None = None):
    req = urllib.request.Request(
        "http://127.0.0.1:4844/api/cloud/auth/" + path,
        data=json.dumps(body).encode() if body else None,
        headers={
            "Content-Type": "application/json",
            "X-Cloud-Lan-Proxy-Key": os.environ["CLOUD_LAN_PROXY_ACCESS_CODE"],
        },
    )
    with urllib.request.urlopen(req, timeout=90) as response:
        return json.load(response)


def main() -> None:
    print("Open this Bambu sign-in URL in your browser:")
    print(request("url")["url"])
    print("After login, copy the failed localhost:13618 redirect URL.")
    callback = getpass.getpass("Paste callback URL (hidden): ")
    request("paste", {"pasted_url": callback})
    print("Login accepted. Printer discovery will run automatically.")


if __name__ == "__main__":
    main()
