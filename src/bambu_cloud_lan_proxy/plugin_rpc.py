"""Private, language-neutral JSON-lines boundary to the isolated native shim.

No printer commands are retried here: timeout/disconnect means the outcome is
unknown, not that a write is safe to repeat. Native diagnostics are discarded
because a proprietary library can print credentials to stderr.
"""

from __future__ import annotations

import asyncio
import json
import os
from contextlib import suppress
from typing import Any


class PluginError(RuntimeError):
    pass


class PluginRPC:
    def __init__(self, executable: list[str], env: dict[str, str] | None = None) -> None:
        self.executable = executable
        self.env = env or {}
        self.process: asyncio.subprocess.Process | None = None
        self.reader: asyncio.Task | None = None
        self.pending: dict[int, asyncio.Future] = {}
        self.events: asyncio.Queue[dict] = asyncio.Queue(maxsize=2048)
        self.lock = asyncio.Lock()
        self.sequence = 0
        self.failure: str | None = None

    async def start(self) -> None:
        if self.process is not None:
            raise PluginError("plugin already started")
        self.process = await asyncio.create_subprocess_exec(
            *self.executable,
            env={**os.environ, **self.env},
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
            limit=16 * 1024 * 1024,
        )
        self.reader = asyncio.create_task(self._receive())

    async def _receive(self) -> None:
        assert self.process and self.process.stdout
        try:
            async for line in self.process.stdout:
                frame = json.loads(line)
                if not isinstance(frame, dict):
                    raise PluginError("invalid native frame")
                if "event" in frame:
                    # Losing print/security events is not safe. Fail closed on overflow.
                    self.events.put_nowait(frame["event"])
                    continue
                future = self.pending.pop(frame.get("id"), None)
                if future is None or future.done():
                    continue  # late reply after timeout/cancellation
                if "error" in frame:
                    future.set_exception(PluginError("native operation failed"))
                else:
                    future.set_result(frame.get("result"))
        except (ValueError, OSError, asyncio.QueueFull):
            self.failure = "native transport failed"
        finally:
            self.failure = self.failure or "native transport closed"
            for future in self.pending.values():
                if not future.done():
                    future.set_exception(PluginError(self.failure))
            self.pending.clear()

    async def call(self, method: str, params: dict, *, timeout: float = 60) -> Any:
        if self.failure or not self.process or self.process.returncode is not None:
            raise PluginError(self.failure or "plugin unavailable")
        if len(self.pending) >= 64:
            raise PluginError("native request limit reached")
        self.sequence += 1
        request_id = self.sequence
        future = asyncio.get_running_loop().create_future()
        self.pending[request_id] = future
        try:
            async with asyncio.timeout(timeout):
                async with self.lock:
                    assert self.process.stdin
                    self.process.stdin.write(
                        (
                            json.dumps(
                                {
                                    "id": request_id,
                                    "method": method,
                                    "params": params,
                                }
                            )
                            + "\n"
                        ).encode()
                    )
                    await self.process.stdin.drain()
                return await future
        except TimeoutError:
            raise PluginError("native operation timed out; outcome unknown") from None
        except (BrokenPipeError, ConnectionResetError):
            raise PluginError("native transport lost; outcome unknown") from None
        finally:
            self.pending.pop(request_id, None)
            if not future.done():
                future.cancel()

    async def stop(self) -> None:
        if self.process and self.process.returncode is None:
            self.process.terminate()
            try:
                await asyncio.wait_for(self.process.wait(), 5)
            except TimeoutError:
                self.process.kill()
                await self.process.wait()
        if self.reader:
            self.reader.cancel()
            with suppress(asyncio.CancelledError):
                await self.reader
