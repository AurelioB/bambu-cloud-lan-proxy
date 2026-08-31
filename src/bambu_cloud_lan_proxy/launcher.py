"""Fail-fast launcher for the derived Bambu Gateway image."""

from __future__ import annotations

import runpy
import sys

from .integration import install
from .storage_api import install_storage_routes


def main() -> None:
    args = sys.argv[1:]
    if args[:2] != ["-m", "app"]:
        raise SystemExit("expected Gateway command beginning with: -m app")
    install()
    from app.main import app  # type: ignore[import-not-found]

    install_storage_routes(app)
    sys.argv = ["app", *args[2:]]
    runpy.run_module("app", run_name="__main__", alter_sys=True)


if __name__ == "__main__":
    main()
