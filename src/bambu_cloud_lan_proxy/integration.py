"""Runtime compatibility hook for the pinned Bambu Gateway release."""

from __future__ import annotations

import logging
from typing import Any

from .bridge import CloudLanFacade
from .config import ProxyConfig

logger = logging.getLogger("bambu.cloud_lan_proxy.integration")
_facade: CloudLanFacade | None = None


def install() -> CloudLanFacade | None:
    """Patch only the cloud-client event boundary, leaving Gateway otherwise intact."""

    global _facade
    config = ProxyConfig.from_env()
    if not config.enabled:
        logger.info("cloud LAN facade disabled")
        return None

    from app.cloud.cloud_printer import CloudPrinterClient  # type: ignore[import-not-found]

    if getattr(CloudPrinterClient, "_cloud_lan_proxy_installed", False):
        return _facade
    if not hasattr(CloudPrinterClient, "handle_event") or not hasattr(
        CloudPrinterClient, "send_command"
    ):
        raise RuntimeError("unsupported Bambu Gateway CloudPrinterClient API")

    facade = CloudLanFacade(config)
    original_init = CloudPrinterClient.__init__
    original_handle_event = CloudPrinterClient.handle_event

    def patched_init(self: Any, *args: Any, **kwargs: Any) -> None:
        original_init(self, *args, **kwargs)
        facade.register(self)

    async def patched_handle_event(self: Any, event: dict[str, Any]) -> None:
        if event.get("kind") == "OnMessage" and event.get("dev_id") == self.serial:
            facade.publish_report(self.serial, event.get("payload", ""))
        await original_handle_event(self, event)

    CloudPrinterClient.__init__ = patched_init
    CloudPrinterClient.handle_event = patched_handle_event
    CloudPrinterClient._cloud_lan_proxy_installed = True
    _facade = facade
    logger.info("installed Bambu Gateway cloud LAN facade compatibility hook")
    return facade
