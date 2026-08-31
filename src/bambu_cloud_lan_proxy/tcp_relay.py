"""Small unprivileged TCP relay used to publish RTSPS on a standard port."""

from __future__ import annotations

import asyncio
import os


async def copy_stream(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while data := await reader.read(64 * 1024):
            writer.write(data)
            await writer.drain()
    finally:
        writer.close()


async def handle_client(
    client_reader: asyncio.StreamReader,
    client_writer: asyncio.StreamWriter,
    upstream_host: str,
    upstream_port: int,
) -> None:
    try:
        upstream_reader, upstream_writer = await asyncio.open_connection(
            upstream_host, upstream_port
        )
    except OSError:
        client_writer.close()
        await client_writer.wait_closed()
        return
    await asyncio.gather(
        copy_stream(client_reader, upstream_writer),
        copy_stream(upstream_reader, client_writer),
        return_exceptions=True,
    )


async def run() -> None:
    listen_host = os.getenv("TCP_RELAY_LISTEN_HOST", "0.0.0.0")
    listen_port = int(os.getenv("TCP_RELAY_LISTEN_PORT", "8322"))
    upstream_host = os.getenv("TCP_RELAY_UPSTREAM_HOST", "host.docker.internal")
    upstream_port = int(os.getenv("TCP_RELAY_UPSTREAM_PORT", "8322"))
    server = await asyncio.start_server(
        lambda reader, writer: handle_client(
            reader, writer, upstream_host, upstream_port
        ),
        listen_host,
        listen_port,
    )
    async with server:
        await server.serve_forever()


def main() -> None:
    asyncio.run(run())


if __name__ == "__main__":
    main()
