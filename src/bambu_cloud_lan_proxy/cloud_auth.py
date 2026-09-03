"""Bambu browser-ticket sign-in, independent of any dashboard framework."""

from __future__ import annotations

import json
from urllib.parse import parse_qs, urlencode, urlsplit

import httpx

from .plugin_install import api_base, studio_headers
from .plugin_rpc import PluginError, PluginRPC


def signin_url(region: str) -> str:
    base = {"US": "https://bambulab.com", "CN": "https://bambulab.cn"}[region]
    target = (
        base
        + "/sign-in/callback?"
        + urlencode(
            {
                "source": "portal",
                "locale": "en",
                "redirect_url": "http://localhost:13618",
                "openBy": "suite",
                "from": "studio",
                "slicerLoginType": "ticket",
            }
        )
    )
    return base + "/sign-in?" + urlencode({"from": "studio", "source": "portal", "to": target})


def ticket_from_url(value: str) -> str:
    url = urlsplit(value)
    if (
        url.scheme != "http"
        or url.hostname not in {"localhost", "127.0.0.1"}
        or url.port != 13618
        or url.username
        or url.password
    ):
        raise ValueError("expected the localhost:13618 browser callback")
    tickets = parse_qs(url.query).get("ticket", []) + parse_qs(url.fragment).get("ticket", [])
    if len(tickets) != 1 or not tickets[0] or len(tickets[0]) > 16384:
        raise ValueError("callback must contain exactly one ticket")
    return tickets[0]


def login_payload(tokens: dict, profile: dict) -> str:
    def field(source: dict, *keys: str) -> str:
        return next((str(source[k]) for k in keys if source.get(k) is not None), "")

    access = field(tokens, "access_token", "accessToken", "token")
    refresh = field(tokens, "refresh_token", "refreshToken")
    user_id = field(profile, "uidStr", "uid", "id")
    if not access or not refresh or not user_id:
        raise PluginError("incomplete Bambu login response")
    user = {key: user_id for key in ("id", "uid", "uidStr")}
    user.update({key: profile.get(key, "") for key in ("name", "account", "avatar")})
    return json.dumps(
        {
            "command": "user_login",
            "data": {
                "token": access,
                "access_token": access,
                "refresh_token": refresh,
                "expires_in": field(tokens, "expires_in", "expiresIn"),
                "refresh_expires_in": field(tokens, "refresh_expires_in", "refreshExpiresIn"),
                "user_id": user_id,
                "uidStr": user_id,
                "user": user,
            },
        }
    )


async def login(host: PluginRPC, region: str, callback: str) -> None:
    tokens = await host.call("get_my_token", {"ticket": ticket_from_url(callback)})
    token = tokens.get("access_token") or tokens.get("accessToken") or tokens.get("token")
    if not token:
        raise PluginError("Bambu ticket exchange returned no access token")
    async with httpx.AsyncClient(timeout=20) as client:
        response = await client.get(
            api_base(region) + "/v1/user-service/my/profile",
            headers={
                **studio_headers(),
                "Authorization": f"Bearer {token}",
            },
        )
        response.raise_for_status()
        payload = login_payload(tokens, response.json())
    result = await host.call("change_user", {"canonical_login": payload})
    if result.get("rc") != 0 or not (await host.call("is_user_login", {})).get("is_login"):
        raise PluginError("Bambu sign-in was not accepted")
