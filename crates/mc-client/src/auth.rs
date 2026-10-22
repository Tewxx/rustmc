//! Online-mode accounts: Microsoft device-code sign-in → Xbox Live → XSTS → Minecraft token and
//! profile, then the session-server `join` that `NetHandlerLoginClient` performs
//! (`MinecraftSessionService.joinServer`) when a server asks for encryption.
//!
//! The Microsoft refresh token is cached in `rustmc_account.json` next to `options.txt`, so the
//! browser step is only needed once.
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

/// The client id vanilla-compatible launchers use for the Live device-code flow.
const CLIENT_ID: &str = "00000000402b5328";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const CACHE: &str = "rustmc_account.json";

#[derive(Clone, Debug)]
pub struct Session {
    pub name: String,
    /// Profile UUID without dashes.
    pub uuid: String,
    pub access_token: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build()
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => format!("HTTP {code}: {}", r.into_string().unwrap_or_default()),
        e => e.to_string(),
    }
}

fn form(a: &ureq::Agent, url: &str, fields: &[(&str, &str)]) -> Result<Value, ureq::Error> {
    a.post(url).send_form(fields)?.into_json().map_err(|e| e.into())
}

/// Microsoft token from a cached refresh token, or the device-code flow printed to the console.
fn microsoft_token(a: &ureq::Agent) -> Result<(String, String), String> {
    if let Some(refresh) = std::fs::read_to_string(CACHE).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v["refresh_token"].as_str().map(String::from)) {
        let r = form(
            a,
            "https://login.live.com/oauth20_token.srf",
            &[("client_id", CLIENT_ID), ("refresh_token", &refresh), ("grant_type", "refresh_token"), ("scope", SCOPE)],
        );
        if let Ok(v) = r {
            if let (Some(t), Some(rt)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) {
                return Ok((t.into(), rt.into()));
            }
        }
    }
    let dc = form(a, "https://login.live.com/oauth20_connect.srf", &[("client_id", CLIENT_ID), ("scope", SCOPE), ("response_type", "device_code")]).map_err(err)?;
    let device_code = dc["device_code"].as_str().ok_or("no device code")?.to_string();
    let interval = dc["interval"].as_u64().unwrap_or(5);
    let expires = dc["expires_in"].as_u64().unwrap_or(900);
    println!(
        "To sign in, open {} and enter the code {}",
        dc["verification_uri"].as_str().unwrap_or("https://www.microsoft.com/link"),
        dc["user_code"].as_str().unwrap_or("?")
    );
    let start = Instant::now();
    while start.elapsed().as_secs() < expires {
        std::thread::sleep(Duration::from_secs(interval));
        match form(
            a,
            "https://login.live.com/oauth20_token.srf",
            &[("client_id", CLIENT_ID), ("device_code", &device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
        ) {
            Ok(v) => {
                if let (Some(t), Some(rt)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) {
                    return Ok((t.into(), rt.into()));
                }
            }
            Err(ureq::Error::Status(400, r)) => {
                let v: Value = r.into_json().unwrap_or_default();
                match v["error"].as_str() {
                    Some("authorization_pending") | Some("slow_down") => continue,
                    _ => return Err(format!("sign-in failed: {v}")),
                }
            }
            Err(e) => return Err(err(e)),
        }
    }
    Err("sign-in timed out".into())
}

/// Signs in (cached when possible) and returns the Minecraft session.
pub fn login() -> Result<Session, String> {
    let a = agent();
    let (ms, refresh) = microsoft_token(&a)?;
    let _ = std::fs::write(CACHE, json!({ "refresh_token": refresh }).to_string());
    let xbl: Value = a
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .send_json(json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": ms },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .map_err(err)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let xbl_token = xbl["Token"].as_str().ok_or("no Xbox Live token")?;
    let xsts: Value = a
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .send_json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => "This Microsoft account has no Xbox profile (or is a child account)".to_string(),
            e => err(e),
        })?
        .into_json()
        .map_err(|e| e.to_string())?;
    let xsts_token = xsts["Token"].as_str().ok_or("no XSTS token")?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"].as_str().ok_or("no user hash")?;
    let mc: Value = a
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .send_json(json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") }))
        .map_err(err)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let access_token = mc["access_token"].as_str().ok_or("no Minecraft token")?.to_string();
    let profile: Value = a
        .get("https://api.minecraftservices.com/minecraft/profile")
        .set("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "This account does not own Minecraft".to_string(),
            e => err(e),
        })?
        .into_json()
        .map_err(|e| e.to_string())?;
    Ok(Session {
        name: profile["name"].as_str().ok_or("no profile name")?.into(),
        uuid: profile["id"].as_str().ok_or("no profile id")?.into(),
        access_token,
    })
}

/// `YggdrasilMinecraftSessionService.joinServer`
pub fn join_server(s: &Session, server_hash: &str) -> Result<(), String> {
    agent()
        .post("https://sessionserver.mojang.com/session/minecraft/join")
        .send_json(json!({ "accessToken": s.access_token, "selectedProfile": s.uuid, "serverId": server_hash }))
        .map(|_| ())
        .map_err(|e| match e {
            ureq::Error::Status(403, _) => "Invalid session (Try restarting your game)".to_string(),
            e => err(e),
        })
}

#[allow(dead_code)]
pub fn forget() {
    let _ = std::fs::remove_file(Path::new(CACHE));
}
