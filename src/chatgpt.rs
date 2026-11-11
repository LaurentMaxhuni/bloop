//! Native Sign in with ChatGPT for Bloop's Responses provider.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, oneshot},
    time::{sleep, timeout},
};

pub(crate) const RESOURCE: &str = "https://api.openai.com/v1";
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
const CALLBACK: &str = "/auth/callback";
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const DIRECT_SCOPE: &str = "chatgpt.tokens.use.direct";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const MAX_AUTH_BODY: usize = 128 * 1024;

// Bloop is single-instance on Windows. Serialize credential changes within that instance.
static SESSION_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Default)]
pub struct AccountStatus {
    pub account: Option<String>,
    pub registered: bool,
    pub signed_in: bool,
    pub plan_enabled: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    host_id: String,
    pending_client: Option<String>,
    profile: Option<Profile>,
}

#[derive(Serialize, Deserialize)]
struct Profile {
    client_id: String,
    subject: String,
    email: Option<String>,
    tokens: Option<Session>,
}

#[derive(Serialize, Deserialize)]
struct Session {
    access_token: String,
    refresh_token: Option<String>,
    id_token: String,
    expires_at: u64,
    scopes: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: u64,
    token_type: String,
    scope: Option<String>,
}

impl TokenResponse {
    fn into_session(self, id_token: String, scopes: String) -> Result<Session, String> {
        if !self.token_type.eq_ignore_ascii_case("bearer")
            || self.access_token.trim().is_empty()
            || self.expires_in == 0
        {
            return Err("ChatGPT returned unusable credentials.".into());
        }
        Ok(Session {
            access_token: self.access_token,
            refresh_token: self.refresh_token.filter(|token| !token.is_empty()),
            id_token,
            expires_at: now().saturating_add(self.expires_in),
            scopes,
        })
    }
}

impl Session {
    fn plan_enabled(&self) -> bool {
        self.scopes
            .split_ascii_whitespace()
            .any(|s| s == DIRECT_SCOPE)
    }
}

impl Store {
    fn load() -> Result<Self, String> {
        let Some(mut protected) = crate::storage::load_chatgpt()? else {
            return Ok(Self::default());
        };
        let mut data = crate::secrets::chatgpt_blob(&mut protected, false)?;
        let result = serde_json::from_slice(&data)
            .map_err(|_| "Could not read the saved ChatGPT account.".into());
        data.fill(0);
        result
    }

    fn save(&self) -> Result<(), String> {
        let mut data = serde_json::to_vec(self)
            .map_err(|_| "Could not encode the ChatGPT account.".to_owned())?;
        let protected = crate::secrets::chatgpt_blob(&mut data, true);
        data.fill(0);
        crate::storage::save_chatgpt(&protected?)
    }

    fn status(&self) -> AccountStatus {
        let tokens = self.profile.as_ref().and_then(|p| p.tokens.as_ref());
        AccountStatus {
            account: self.profile.as_ref().and_then(|p| p.email.clone()),
            registered: self.profile.is_some() || self.pending_client.is_some(),
            signed_in: tokens.is_some(),
            plan_enabled: tokens.is_some_and(Session::plan_enabled),
        }
    }
}

pub async fn status() -> Result<AccountStatus, String> {
    let _guard = SESSION_LOCK.lock().await;
    Ok(Store::load()?.status())
}

pub async fn sign_in(
    open_browser: fn(&str) -> Result<(), String>,
    new_account: bool,
    cancel: oneshot::Receiver<()>,
) -> Result<AccountStatus, String> {
    if !cfg!(windows) {
        return Err("ChatGPT sign-in is available on Windows.".into());
    }
    let _guard = SESSION_LOCK.lock().await;
    let mut store = Store::load()?;
    if new_account && store.status().signed_in {
        return Err("Sign out of your current ChatGPT account first.".into());
    }
    if store.host_id.is_empty() {
        store.host_id = host_id()?;
        store.save()?;
    }
    let existing = if new_account {
        None
    } else {
        store.profile.as_ref()
    };
    let client_id = existing
        .map(|p| p.client_id.as_str())
        .or(store.pending_client.as_deref());

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "Could not start the ChatGPT sign-in listener.")?;
    let address = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    let redirect = format!("http://{address}{CALLBACK}");
    let state = URL_SAFE_NO_PAD.encode(random::<32>()?);
    let nonce = URL_SAFE_NO_PAD.encode(random::<32>()?);
    let verifier = URL_SAFE_NO_PAD.encode(random::<32>()?);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut url = Url::parse(AUTHORIZE).expect("valid authorization endpoint");
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            ("client_id", client_id.unwrap_or(DYNAMIC_CLIENT)),
            ("ext_agent_host_id", &store.host_id),
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
        ]);
        if client_id.is_none() {
            query.append_pair("agent_name_hint", "Bloop");
        }
        if let Some(profile) = existing {
            if let Some(email) = &profile.email {
                query.append_pair("login_hint", email);
            }
            if let Some(tokens) = &profile.tokens {
                query.append_pair("id_token_hint", &tokens.id_token);
                if !tokens.plan_enabled() {
                    query.append_pair("prompt", "consent");
                }
            }
        }
    }
    open_browser(url.as_str()).map_err(|_| "Could not open ChatGPT sign-in in your browser.")?;
    let callback = tokio::select! {
        biased;
        _ = cancel => return Err("ChatGPT sign-in canceled.".into()),
        result = timeout(Duration::from_secs(300), receive_callback(listener, &address, &state, client_id)) => {
            result.map_err(|_| "ChatGPT sign-in timed out. Please try again.")??
        }
    };
    let expected_subject = existing.map(|p| p.subject.clone());
    if existing.is_none() {
        store.pending_client = Some(callback.client_id.clone());
        store.save()?;
    }
    let client = auth_client()?;
    let response: TokenResponse = read_json(form(
        &client,
        TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &callback.client_id),
            ("code", &callback.code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("resource", RESOURCE),
        ],
    ))
    .await
    .map_err(|e| e.message("ChatGPT sign-in"))?;
    let id_token = response.id_token.clone().ok_or_else(invalid_identity)?;
    let identity = verify_identity(&client, &id_token, &callback.client_id, &nonce).await?;
    if expected_subject.is_some_and(|subject| subject != identity.sub) {
        return Err("Choose the ChatGPT account originally connected to Bloop.".into());
    }
    let scopes = response.scope.clone().unwrap_or_default();
    store.profile = Some(Profile {
        client_id: callback.client_id,
        subject: identity.sub,
        email: identity.email,
        tokens: Some(response.into_session(id_token, scopes)?),
    });
    store.pending_client = None;
    store.save()?;
    Ok(store.status())
}

pub async fn sign_out() -> Result<(AccountStatus, bool), String> {
    let _guard = SESSION_LOCK.lock().await;
    let mut store = Store::load()?;
    let mut revoked = true;
    if let Some(profile) = store.profile.as_mut()
        && let Some(session) = profile.tokens.take()
    {
        if let Some(token) = session.refresh_token {
            revoked = revoke(&profile.client_id, &token).await;
        }
        store.save()?;
    }
    Ok((store.status(), revoked))
}

async fn revoke(client_id: &str, token: &str) -> bool {
    #[derive(Deserialize)]
    struct Discovery {
        revocation_endpoint: String,
    }
    // Keep the credential on the documented auth origin even if metadata changes.
    async fn attempt(client: &Client, client_id: &str, token: &str) -> Result<(), AuthError> {
        let discovery: Discovery = read_json(client.get(DISCOVERY)).await?;
        let url = Url::parse(&discovery.revocation_endpoint).map_err(|_| AuthError::Invalid)?;
        if url.origin() != Url::parse(ISSUER).expect("valid issuer").origin()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AuthError::Invalid);
        }
        checked_response(form(
            client,
            url.as_str(),
            &[
                ("client_id", client_id),
                ("token", token),
                ("token_type_hint", "refresh_token"),
            ],
        ))
        .await?;
        Ok(())
    }

    let Ok(client) = auth_client() else {
        return false;
    };
    match attempt(&client, client_id, token).await {
        Ok(()) => true,
        Err(error) if error.is_temporary() => {
            sleep(Duration::from_secs(1)).await;
            attempt(&client, client_id, token).await.is_ok()
        }
        Err(_) => false,
    }
}

pub(crate) async fn access_token() -> Result<String, String> {
    // A canceled chat must not interrupt a rotating refresh between exchange and save.
    tokio::spawn(renew_if_needed())
        .await
        .map_err(|_| "Could not prepare ChatGPT credentials.")?
}

async fn renew_if_needed() -> Result<String, String> {
    let _guard = SESSION_LOCK.lock().await;
    let mut store = Store::load()?;
    let reconnect = || "Sign in to ChatGPT under Settings → Model.".to_owned();
    let profile = store.profile.as_mut().ok_or_else(reconnect)?;
    let tokens = profile.tokens.as_ref().ok_or_else(reconnect)?;
    if !tokens.plan_enabled() {
        return Err("Enable ChatGPT plan usage under Settings → Model.".into());
    }
    if tokens.expires_at > now().saturating_add(60) {
        return Ok(tokens.access_token.clone());
    }
    let refresh_token = tokens.refresh_token.as_deref().ok_or_else(reconnect)?;
    let client = auth_client()?;
    let result: Result<TokenResponse, _> = read_json(form(
        &client,
        TOKEN,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &profile.client_id),
            ("refresh_token", refresh_token),
            ("resource", RESOURCE),
        ],
    ))
    .await;
    let response = match result {
        Ok(response) => response,
        Err(error) if error.requires_sign_in() => {
            profile.tokens = None;
            store.save()?;
            return Err(reconnect());
        }
        Err(error) => return Err(error.message("ChatGPT credential refresh")),
    };
    let scopes = response
        .scope
        .clone()
        .unwrap_or_else(|| tokens.scopes.clone());
    // Keep the original, verified ID token as a hint for this registration.
    let renewed = response.into_session(tokens.id_token.clone(), scopes)?;
    let has_plan = renewed.plan_enabled();
    let access_token = renewed.access_token.clone();
    profile.tokens = Some(renewed);
    store.save()?;
    if !has_plan {
        return Err("Enable ChatGPT plan usage under Settings → Model.".into());
    }
    Ok(access_token)
}

struct Callback {
    code: String,
    client_id: String,
}

async fn receive_callback(
    listener: TcpListener,
    address: &str,
    state: &str,
    client_id: Option<&str>,
) -> Result<Callback, String> {
    loop {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|_| "ChatGPT callback failed.")?;
        let Ok(Ok(target)) = timeout(
            Duration::from_secs(2),
            callback_target(&mut socket, address),
        )
        .await
        else {
            continue;
        };
        let result = parse_callback(&target, state, client_id);
        let (status, body) = match &result {
            Some(Ok(_)) => ("200 OK", "Return to Bloop to finish signing in."),
            Some(Err(_)) => (
                "400 Bad Request",
                "ChatGPT sign-in stopped. Return to Bloop for details.",
            ),
            None => ("404 Not Found", "No matching Bloop sign-in request."),
        };
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = timeout(Duration::from_secs(1), socket.write_all(reply.as_bytes())).await;
        if let Some(result) = result {
            return result;
        }
    }
}

async fn callback_target(socket: &mut TcpStream, address: &str) -> Result<String, ()> {
    let mut bytes = [0; 8192];
    let mut end = 0;
    while end < bytes.len() {
        let n = socket.read(&mut bytes[end..]).await.map_err(|_| ())?;
        if n == 0 {
            break;
        }
        end += n;
        let mut headers = [httparse::EMPTY_HEADER; 48];
        let mut request = httparse::Request::new(&mut headers);
        if !request.parse(&bytes[..end]).map_err(|_| ())?.is_complete() {
            continue;
        }
        let mut hosts = request
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("host"));
        if request.method == Some("GET")
            && hosts
                .next()
                .is_some_and(|host| host.value == address.as_bytes())
            && hosts.next().is_none()
        {
            return request.path.map(str::to_owned).ok_or(());
        }
        break;
    }
    Err(())
}

fn parse_callback(
    target: &str,
    state: &str,
    client_id: Option<&str>,
) -> Option<Result<Callback, String>> {
    let (path, query) = target.split_once('?')?;
    if path != CALLBACK {
        return None;
    }
    let mut params = std::collections::HashMap::new();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        if params.insert(key, value).is_some() {
            return None;
        }
    }
    if params.get("state")?.as_ref() != state {
        return None;
    }
    if let Some(error) = params.get("error") {
        return Some(Err(if error == "access_denied" {
            "ChatGPT authorization was declined."
        } else {
            "ChatGPT authorization failed. Please try again."
        }
        .into()));
    }
    let issued = params.get("client_id").map(|value| value.as_ref());
    let client_id = match (client_id, issued) {
        (Some(expected), None) => expected,
        (Some(expected), Some(actual)) if expected == actual => expected,
        (None, Some(actual)) if actual.starts_with("oaiapp_") && actual.len() > 7 => actual,
        _ => {
            return Some(Err(
                "ChatGPT returned an invalid client registration.".into()
            ));
        }
    };
    Some(
        params
            .get("code")
            .filter(|code| !code.is_empty())
            .map(|code| Callback {
                client_id: client_id.to_owned(),
                code: code.to_string(),
            })
            .ok_or_else(|| "ChatGPT did not return an authorization code.".into()),
    )
}

#[derive(Deserialize)]
struct Identity {
    iss: String,
    sub: String,
    aud: Audience,
    azp: Option<String>,
    nonce: String,
    exp: u64,
    nbf: Option<u64>,
    email: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Identity {
    fn matches(&self, client_id: &str, nonce: &str, now: u64) -> bool {
        let audience = match &self.aud {
            Audience::One(value) => value == client_id,
            Audience::Many(values) => {
                values.iter().any(|value| value == client_id)
                    && (values.len() == 1 || self.azp.as_deref() == Some(client_id))
            }
        };
        self.iss == ISSUER
            && !self.sub.is_empty()
            && audience
            && self.azp.as_deref().is_none_or(|party| party == client_id)
            && self.nonce == nonce
            && self.exp > now
            && self.nbf.is_none_or(|nbf| nbf <= now)
    }
}

async fn verify_identity(
    client: &Client,
    token: &str,
    client_id: &str,
    nonce: &str,
) -> Result<Identity, String> {
    #[derive(Deserialize)]
    struct Header {
        alg: String,
        kid: String,
    }
    #[derive(Deserialize)]
    struct Keys {
        keys: Vec<Key>,
    }
    #[derive(Deserialize)]
    struct Key {
        kid: String,
        kty: String,
        alg: Option<String>,
        #[serde(rename = "use")]
        purpose: Option<String>,
        n: Option<String>,
        e: Option<String>,
    }

    let (signed, signature) = token.rsplit_once('.').ok_or_else(invalid_identity)?;
    let (header, claims) = signed.split_once('.').ok_or_else(invalid_identity)?;
    let header: Header =
        serde_json::from_slice(&decode(header)?).map_err(|_| invalid_identity())?;
    if header.alg != "RS256" || header.kid.is_empty() {
        return Err(invalid_identity());
    }
    let keys: Keys = read_json(client.get(JWKS))
        .await
        .map_err(|e| e.message("ChatGPT identity verification"))?;
    let key = keys
        .keys
        .iter()
        .find(|key| {
            key.kid == header.kid
                && key.kty == "RSA"
                && key.alg.as_deref().is_none_or(|alg| alg == "RS256")
                && key
                    .purpose
                    .as_deref()
                    .is_none_or(|purpose| purpose == "sig")
        })
        .ok_or_else(invalid_identity)?;
    RsaPublicKeyComponents {
        n: decode(key.n.as_deref().ok_or_else(invalid_identity)?)?,
        e: decode(key.e.as_deref().ok_or_else(invalid_identity)?)?,
    }
    .verify(
        &RSA_PKCS1_2048_8192_SHA256,
        signed.as_bytes(),
        &decode(signature)?,
    )
    .map_err(|_| invalid_identity())?;
    let identity: Identity =
        serde_json::from_slice(&decode(claims)?).map_err(|_| invalid_identity())?;
    if !identity.matches(client_id, nonce, now()) {
        return Err(invalid_identity());
    }
    Ok(identity)
}

fn decode(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| invalid_identity())
}

fn invalid_identity() -> String {
    "ChatGPT returned an identity Bloop could not verify.".into()
}

fn auth_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| "Could not initialize the ChatGPT connection.".into())
}

fn form(client: &Client, url: &str, params: &[(&str, &str)]) -> RequestBuilder {
    let body = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().copied())
        .finish();
    client
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
}

enum AuthError {
    Network,
    Invalid,
    Http(StatusCode, Option<String>),
}

impl AuthError {
    fn message(&self, action: &str) -> String {
        match self {
            Self::Network => format!("{action} could not reach OpenAI. Try again."),
            Self::Invalid => format!("{action} received an unexpected reply from OpenAI."),
            Self::Http(status, code) => {
                let code = code
                    .as_ref()
                    .map(|code| format!(" ({code})"))
                    .unwrap_or_default();
                format!("{action} failed with HTTP {}{code}.", status.as_u16())
            }
        }
    }

    fn is_temporary(&self) -> bool {
        matches!(self, Self::Network)
            || matches!(self, Self::Http(status, _) if status.is_server_error())
    }

    fn requires_sign_in(&self) -> bool {
        matches!(self, Self::Http(_, Some(code)) if matches!(code.as_str(),
            "invalid_grant" | "invalid_refresh_token" | "token_expired" | "refresh_token_expired"
            | "refresh_token_invalidated" | "refresh_token_reused"))
    }
}

async fn checked_response(request: RequestBuilder) -> Result<reqwest::Response, AuthError> {
    let response = request.send().await.map_err(|_| AuthError::Network)?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let code = read_body(response)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|body| {
            body.get("error")
                .and_then(|error| error.as_str().or_else(|| error.get("code")?.as_str()))
                .filter(|code| {
                    code.len() <= 64 && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                })
                .map(str::to_owned)
        });
    Err(AuthError::Http(status, code))
}

async fn read_json<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, AuthError> {
    let body = read_body(checked_response(request).await?).await?;
    serde_json::from_slice(&body).map_err(|_| AuthError::Invalid)
}

async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, AuthError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Network)? {
        if body.len() + chunk.len() > MAX_AUTH_BODY {
            return Err(AuthError::Invalid);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "Could not generate secure ChatGPT sign-in values.")?;
    Ok(bytes)
}

fn host_id() -> Result<String, String> {
    let mut bytes = random::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = format!("{:032x}", u128::from_be_bytes(bytes));
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_checks_path_state_and_client_registration() {
        let query = |query: &str, client_id| {
            parse_callback(&format!("{CALLBACK}?{query}"), "fresh", client_id)
        };
        assert!(parse_callback("/wrong?state=fresh&code=test", "fresh", None).is_none());
        assert!(query("state=old&code=test", None).is_none());
        assert!(query("state=old&state=fresh&code=test", None).is_none());
        assert!(query("state=fresh&code=test", None).unwrap().is_err());
        assert!(
            query(
                "state=fresh&code=test&client_id=oaiapp_other",
                Some("oaiapp_saved")
            )
            .unwrap()
            .is_err()
        );
        assert!(
            query(
                "state=fresh&code=test&error=access_denied",
                Some("oaiapp_saved")
            )
            .unwrap()
            .is_err()
        );
        let initial = query("state=fresh&code=test%2B123&client_id=oaiapp_new", None)
            .unwrap()
            .unwrap();
        assert_eq!(initial.code, "test+123");
        assert_eq!(initial.client_id, "oaiapp_new");
        let returning = query("state=fresh&code=test", Some("oaiapp_saved"))
            .unwrap()
            .unwrap();
        assert_eq!(returning.client_id, "oaiapp_saved");
    }

    #[tokio::test]
    async fn loopback_listener_ignores_unrelated_requests() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let host = address.to_string();
        let browser = async {
            for (path, expected) in [
                ("/favicon.ico", "404 Not Found"),
                ("/auth/callback?state=wrong&code=discard", "404 Not Found"),
                ("/auth/callback?state=fresh&code=example", "200 OK"),
            ] {
                let mut socket = TcpStream::connect(address).await.unwrap();
                let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n");
                socket.write_all(request.as_bytes()).await.unwrap();
                let mut response = String::new();
                socket.read_to_string(&mut response).await.unwrap();
                assert!(response.starts_with(&format!("HTTP/1.1 {expected}")));
                assert!(!response.contains("code=example"));
            }
        };
        let (callback, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(
                receive_callback(listener, &host, "fresh", Some("oaiapp_saved")),
                browser
            )
        })
        .await
        .unwrap();
        assert_eq!(callback.unwrap().code, "example");
    }

    #[test]
    fn identity_is_bound_to_issuer_audience_nonce_and_expiry() {
        let mut identity: Identity = serde_json::from_value(serde_json::json!({
            "iss": ISSUER, "aud": ["oaiapp_bloop", "another"], "azp": "oaiapp_bloop",
            "sub": "subject", "nonce": "random", "exp": 100, "nbf": 30,
        }))
        .unwrap();
        assert!(identity.matches("oaiapp_bloop", "random", 80));
        assert!(!identity.matches("another", "random", 80));
        assert!(!identity.matches("oaiapp_bloop", "wrong", 80));
        assert!(!identity.matches("oaiapp_bloop", "random", 100));
        assert!(!identity.matches("oaiapp_bloop", "random", 29));
        identity.iss = "https://example.test".into();
        assert!(!identity.matches("oaiapp_bloop", "random", 80));
    }
}
