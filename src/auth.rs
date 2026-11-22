use crate::config::{AuthMode, Config};
use argon2::password_hash::PasswordVerifier;
use argon2::Argon2;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::state::Persisted;

pub const COOKIE_NAME: &str = "SID";
const TTL_SECS: u64 = 7 * 24 * 3600;

/// Всё, что нужно для аутентификации: секрет кук, режим, параметры AD.
#[derive(Clone)]
pub struct AuthState {
    pub secret: Vec<u8>,
    pub mode: AuthMode,
    pub ad_url: String,
    pub ad_domain: String,
}

/// Имя пользователя из куки. Вставляется middleware, хендлеры берут экстрактором.
#[derive(Clone)]
pub struct CurrentUser(pub String);

impl<S> axum::extract::FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = StatusCode;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Имя: буквы/цифры/._- , до 64 символов — иначе сломается разбор "user|expiry" в куке.
pub fn valid_username(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 64
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && user.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

pub fn make_token(secret: &[u8], user: &str) -> String {
    let expiry = now() + TTL_SECS;
    let payload = format!("{user}|{expiry}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
    mac.update(payload.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{payload}.{sig}")
}

/// -> Some(user), если подпись валидна и не протухла
pub fn verify_token(secret: &[u8], cookie: &str) -> Option<String> {
    let (payload, sig) = cookie.rsplit_once('.')?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac key");
    mac.update(payload.as_bytes());
    mac.verify_slice(&hex::decode(sig).ok()?).ok()?;
    let (user, expiry) = payload.split_once('|')?;
    if expiry.parse::<u64>().ok()? < now() {
        return None;
    }
    Some(user.to_string())
}

fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// middleware: кладёт CurrentUser в extensions или отдаёт 401.
pub async fn require_auth(
    State(auth): State<AuthState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let user = cookie_value(req.headers(), COOKIE_NAME).and_then(|v| verify_token(&auth.secret, &v));
    match user {
        Some(u) => {
            let mut req = req;
            req.extensions_mut().insert(CurrentUser(u));
            next.run(req).await
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginReq {
    pub user: String,
    pub pass: String,
}

/// Общая проверка для /login и /api/v2/auth -> токен куки.
pub async fn login_check(
    auth: &AuthState,
    persist: &Mutex<Persisted>,
    user: &str,
    pass: &str,
) -> Result<String, StatusCode> {
    if !valid_username(user) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let verified = match auth.mode {
        AuthMode::Ad => ldap_bind(&auth.ad_url, &auth.ad_domain, user, pass).await,
        AuthMode::Local => {
            let hash = persist.lock().unwrap().users.get(user).cloned();
            hash.map(|h| local_verify(&h, pass)).unwrap_or(false)
        }
    };
    if !verified {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(make_token(&auth.secret, user))
}

fn auth_cookie(token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; Max-Age={TTL_SECS}; Path=/; HttpOnly; SameSite=Lax"
    )
}

/// POST /login — работает в обоих режимах, источник проверяет по AUTH_MODE.
#[utoipa::path(post, path = "/login", request_body = LoginReq,
    responses((status = 200, description = "кука выставлена"), (status = 401)))]
pub async fn login(
    State(app): State<crate::api::App>,
    Json(req): Json<LoginReq>,
) -> Response {
    match login_check(&app.auth, &app.persist, &req.user, &req.pass).await {
        Ok(token) => {
            let mut resp = (StatusCode::OK, "Ok.").into_response();
            resp.headers_mut()
                .insert(header::SET_COOKIE, HeaderValue::from_str(&auth_cookie(&token)).unwrap());
            resp
        }
        Err(code) => code.into_response(),
    }
}

/// POST /logout
#[utoipa::path(post, path = "/logout", responses((status = 200)))]
pub async fn logout() -> Response {
    let cookie = format!("{COOKIE_NAME}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax");
    let mut resp = (StatusCode::OK, "Ok.").into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    resp
}

fn local_verify(phc_hash: &str, pass: &str) -> bool {
    argon2::PasswordHash::new(phc_hash)
        .map(|ph| Argon2::default().verify_password(pass.as_bytes(), &ph).is_ok())
        .unwrap_or(false)
}

/// LDAP bind-as-user против Samba AD: bind удался = пользователь настоящий.
async fn ldap_bind(url: &str, domain: &str, user: &str, pass: &str) -> bool {
    let (conn, mut ldap) = match ldap3::LdapConnAsync::new(url).await {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!("ldap connect {url}: {e}");
            return false;
        }
    };
    ldap3::drive!(conn);
    let bind = ldap
        .simple_bind(&format!("{user}@{domain}"), pass)
        .await
        .map(|r| r.success().is_ok())
        .unwrap_or(false);
    let _ = ldap.unbind().await;
    bind
}

pub fn secret_from(cfg: &Config, p: &Persisted) -> Vec<u8> {
    cfg.secret
        .clone()
        .or_else(|| p.secret_hex.as_ref().and_then(|s| hex::decode(s).ok()))
        .unwrap_or_else(|| b"insecure-default-secret".to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip_and_expiry() {
        let s = b"secret";
        let t = make_token(s, "ilnar");
        assert_eq!(verify_token(s, &t).as_deref(), Some("ilnar"));
        assert_eq!(verify_token(b"other", &t), None);
        assert_eq!(verify_token(s, &format!("{t}x")), None);
        let old = format!("ilnar|0.{}", {
            let mut mac = Hmac::<Sha256>::new_from_slice(s).unwrap();
            mac.update(b"ilnar|0");
            hex::encode(mac.finalize().into_bytes())
        });
        assert_eq!(verify_token(s, &old), None);
    }

    #[test]
    fn usernames() {
        assert!(valid_username("ilnar"));
        assert!(valid_username("a-b_c2"));
        assert!(!valid_username("a|b"));
        assert!(!valid_username(""));
        assert!(!valid_username("u".repeat(65).as_str()));
        assert!(!valid_username(".dot"));
    }
}
