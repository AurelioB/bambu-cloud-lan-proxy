use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde_json::{Map, Value, json};
use url::Url;

use crate::{
    plugin_install::{api_base, studio_headers},
    plugin_rpc::PluginRpc,
};

pub fn signin_url(region: &str) -> Result<String> {
    let base = match region {
        "US" => "https://bambulab.com",
        "CN" => "https://bambulab.cn",
        _ => bail!("invalid cloud region"),
    };
    let mut callback = Url::parse(&format!("{base}/sign-in/callback"))?;
    callback
        .query_pairs_mut()
        .append_pair("source", "portal")
        .append_pair("locale", "en")
        .append_pair("redirect_url", "http://localhost:13618")
        .append_pair("openBy", "suite")
        .append_pair("from", "studio")
        .append_pair("slicerLoginType", "ticket");
    let mut login = Url::parse(&format!("{base}/sign-in"))?;
    login
        .query_pairs_mut()
        .append_pair("from", "studio")
        .append_pair("source", "portal")
        .append_pair("to", callback.as_str());
    Ok(login.into())
}

pub fn ticket_from_url(value: &str) -> Result<String> {
    let url = Url::parse(value)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || url.port() != Some(13618)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("expected the localhost:13618 browser callback")
    }
    let mut tickets: Vec<String> = url
        .query_pairs()
        .chain(
            url.fragment()
                .into_iter()
                .flat_map(|fragment| url::form_urlencoded::parse(fragment.as_bytes())),
        )
        .filter(|(key, _)| key == "ticket")
        .map(|(_, value)| value.into_owned())
        .collect();
    if tickets.len() != 1 || tickets[0].is_empty() || tickets[0].len() > 16_384 {
        bail!("callback must contain exactly one ticket")
    }
    Ok(tickets.remove(0))
}

fn field(object: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| object.get(*key))
        .filter(|v| !v.is_null())
        .map(|value| match value {
            Value::String(value) => value.clone(),
            value => value.to_string(),
        })
        .unwrap_or_default()
}

pub fn login_payload(tokens: &Value, profile: &Value) -> Result<String> {
    let tokens = tokens.as_object().context("invalid token response")?;
    let profile = profile.as_object().context("invalid profile response")?;
    let access = field(tokens, &["access_token", "accessToken", "token"]);
    let refresh = field(tokens, &["refresh_token", "refreshToken"]);
    let user_id = field(profile, &["uidStr", "uid", "id"]);
    if access.is_empty() || refresh.is_empty() || user_id.is_empty() {
        bail!("incomplete Bambu login response")
    }
    let user = json!({
        "id": user_id, "uid": user_id, "uidStr": user_id,
        "name": field(profile, &["name"]), "account": field(profile, &["account"]),
        "avatar": field(profile, &["avatar"]),
    });
    Ok(json!({"command":"user_login","data":{
        "token":access,"access_token":access,"refresh_token":refresh,
        "expires_in":field(tokens,&["expires_in","expiresIn"]),
        "refresh_expires_in":field(tokens,&["refresh_expires_in","refreshExpiresIn"]),
        "user_id":user_id,"uidStr":user_id,"user":user
    }})
    .to_string())
}

pub async fn login(host: &PluginRpc, region: &str, callback: &str) -> Result<()> {
    let ticket = ticket_from_url(callback)?;
    let tokens = host
        .call(
            "get_my_token",
            json!({"ticket":ticket}),
            Duration::from_secs(60),
        )
        .await?;
    let token = tokens
        .as_object()
        .map(|object| field(object, &["access_token", "accessToken", "token"]))
        .unwrap_or_default();
    if token.is_empty() {
        bail!("Bambu ticket exchange returned no access token")
    }
    let mut headers = studio_headers()?;
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    let profile: Value = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?
        .get(format!("{}/v1/user-service/my/profile", api_base(region)?))
        .headers(headers)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let canonical = login_payload(&tokens, &profile)?;
    let result = host
        .call(
            "change_user",
            json!({"canonical_login":canonical}),
            Duration::from_secs(60),
        )
        .await?;
    if result.get("rc").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        bail!("Bambu sign-in was not accepted")
    }
    let status = host
        .call("is_user_login", json!({}), Duration::from_secs(30))
        .await?;
    if !status
        .get("is_login")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        bail!("Bambu sign-in was not accepted")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_is_strictly_loopback() {
        assert_eq!(
            ticket_from_url("http://localhost:13618/?ticket=one").unwrap(),
            "one"
        );
        assert!(ticket_from_url("https://evil.example/?ticket=x").is_err());
        assert!(ticket_from_url("http://localhost:13618/?ticket=a&ticket=b").is_err());
    }
}
