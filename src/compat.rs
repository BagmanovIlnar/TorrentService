use crate::api::App;
use crate::auth::{login_check, CurrentUser, COOKIE_NAME};
use crate::torrent::TorrentInfo;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

/// Подмножество qBittorrent WebUI API v2, которого достаточно radarr/sonarr/lidarr.
/// В swagger не документируем — это чужой фиксированный протокол.

const QBT_VERSION: &str = "v5.0.2";

fn qbt_state(t: &TorrentInfo) -> &'static str {
    match t.status.as_str() {
        "paused" => "stoppedUP",
        "seeding" => "uploading",
        "error" => "errored",
        _ => "downloading",
    }
}

fn save_path_of(app: &App, category: &Option<String>) -> String {
    let mut s = match category {
        Some(c) => app.downloads_dir.join(c).display().to_string(),
        None => app.downloads_dir.display().to_string(),
    };
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

fn to_qbt(t: &TorrentInfo, app: &App) -> serde_json::Value {
    let save_path = save_path_of(app, &t.category);
    json!({
        "hash": t.hash,
        "name": t.name,
        "size": t.size,
        "progress": t.progress,
        "dlspeed": t.down_speed,
        "upspeed": t.up_speed,
        "eta": t.eta_sec as i64,
        "ratio": t.ratio,
        "uploaded": t.uploaded,
        "downloaded": t.downloaded,
        "state": qbt_state(t),
        "category": t.category.clone().unwrap_or_default(),
        "num_seeds": t.seeders,
        "num_leechs": t.peers,
        "added_on": t.added_at as i64,
        "completion_on": t.completed_at.map(|c| c as i64).unwrap_or(-1),
        "save_path": save_path,
        "content_path": format!("{}{}", save_path, t.name),
    })
}

/// GET /api/v2/app/version
pub async fn app_version() -> &'static str {
    QBT_VERSION
}

/// GET /api/v2/app/preferences — что *arr читают при настройке клиента
pub async fn app_preferences() -> Response {
    axum::Json(json!({
        "save_path": "",
        "temp_path_enabled": false,
        "category_in_path": true,
        "auto_tmm_enabled": false,
        "max_ratio_enabled": false,
    }))
    .into_response()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// POST /api/v2/auth — form username/password (как у настоящего qBittorrent).
pub async fn v2_auth(
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let fields = form_from(&headers, &body);
    let user = fields.get_first("username").unwrap_or_default();
    let pass = fields.get_first("password").unwrap_or_default();
    match login_check(&app.auth, &app.persist, &user, &pass).await {
        Ok(token) => {
            let cookie = format!(
                "{COOKIE_NAME}={token}; Max-Age=604800; Path=/; HttpOnly; SameSite=Lax"
            );
            let mut resp = "Ok.".into_response();
            resp.headers_mut()
                .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
            resp
        }
        Err(code) => code.into_response(),
    }
}

/// POST /api/v2/torrents/add — multipart (urls|torrents, category) или urlencoded
pub async fn torrents_add(
    user: CurrentUser,
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let form = form_from(&headers, &body);
    let urls: Vec<String> = form
        .get_all("urls")
        .into_iter()
        .flat_map(|u| u.lines().map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .filter(|u| !u.is_empty())
        .collect();
    let category = form.get_first("category").filter(|c| !c.is_empty());
    let file = form.file.clone();

    if let Some(c) = &category {
        let mut p = app.persist.lock().unwrap();
        crate::state::category_add(&mut p, &user.0, c);
        crate::state::save(&app.persist_dir(), &p).ok();
    }

    let mut added = 0;
    let mut sources: Vec<librqbit::AddTorrent> = urls
        .iter()
        .map(|u| librqbit::AddTorrent::from_url(u.clone()))
        .collect();
    if let Some(bytes) = file {
        sources.push(librqbit::AddTorrent::from_bytes(bytes));
    }
    for source in sources {
        match crate::torrent::add(&app.session, source, &app.downloads_dir, category.clone()).await {
            Ok((hash, id)) => {
                let mut p = app.persist.lock().unwrap();
                crate::state::subscribe(
                    &mut p,
                    &hash,
                    crate::state::TorrentRec {
                        id,
                        category: category.clone(),
                        subscribers: vec![user.0.clone()],
                        added_at: now(),
                        completed_at: None,
                        user_states: std::collections::HashMap::new(),
                        seeding_paused: None,
                    },
                    &user.0,
                );
                crate::state::save(&app.persist_dir(), &p).ok();
                added += 1;
            }
            Err(e) => tracing::warn!("compat add: {e}"),
        }
    }
    if added == 0 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    "Ok.".into_response()
}

#[derive(serde::Deserialize)]
pub struct InfoQuery {
    pub category: Option<String>,
}

/// GET /api/v2/torrents/info
pub async fn torrents_info(
    user: CurrentUser,
    State(app): State<App>,
    axum::extract::Query(q): axum::extract::Query<InfoQuery>,
) -> Response {
    let p = app.persist.lock().unwrap();
    let (infos, _) = crate::torrent::list_user(&app.session, &p, &user.0, q.category.as_deref());
    drop(p);
    axum::Json(infos.iter().map(|t| to_qbt(t, &app)).collect::<Vec<_>>()).into_response()
}

/// POST /api/v2/torrents/delete — для *arr это отписка: файлы переживут,
/// пока не ушёл последний подписчик (deleteFiles=true учитывается).
pub async fn torrents_delete(
    user: CurrentUser,
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let form = form_from(&headers, &body);
    let hashes: Vec<String> = form
        .get_first("hashes")
        .unwrap_or_default()
        .split('|')
        .map(|s| s.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();
    let delete_files = form.get_first("deleteFiles").map(|v| v == "true").unwrap_or(false);
    for hash in hashes {
        // id берём ДО отписки — после неё записи может не остаться
        let (id, still_used) = {
            let mut p = app.persist.lock().unwrap();
            let id = p.torrents.get(&hash).map(|r| r.id);
            let still = id.map(|_| crate::state::unsubscribe(&mut p, &hash, &user.0));
            (id, still == Some(false))
        };
        if !still_used {
            if let Some(id) = id {
                crate::torrent::remove(&app.session, id, delete_files).await.ok();
            }
        }
    }
    "Ok.".into_response()
}

/// POST /api/v2/torrents/pause и /resume
pub async fn torrents_pause(
    user: CurrentUser,
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    pause_resume(&app, &user.0, &form_from(&headers, &body), true).await;
    "Ok.".into_response()
}

pub async fn torrents_resume(
    user: CurrentUser,
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    pause_resume(&app, &user.0, &form_from(&headers, &body), false).await;
    "Ok.".into_response()
}

async fn pause_resume(app: &App, username: &str, form: &Form, pause: bool) {
    let hashes: Vec<String> = form
        .get_first("hashes")
        .unwrap_or_default()
        .split('|')
        .map(|s| s.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();
    for hash in hashes {
        // только свои (подписчик) — иначе *arr может трогать чужое
        let (id, desired_running) = {
            let mut p = app.persist.lock().unwrap();
            let is_subscriber = p
                .torrents
                .get(&hash)
                .map(|rec| rec.subscribers.iter().any(|s| s == username))
                .unwrap_or(false);
            let d = if is_subscriber {
                Some(if pause {
                    crate::state::user_pause(&mut p, &hash, username)
                } else {
                    crate::state::user_resume(&mut p, &hash, username)
                })
            } else {
                None
            };
            let id = p.torrents.get(&hash).map(|rec| rec.id);
            crate::state::save(&app.persist_dir(), &p).ok();
            (id, d)
        };
        if let (Some(id), Some(desired)) = (id, desired_running) {
            let r = if desired {
                crate::torrent::resume(&app.session, id).await
            } else {
                crate::torrent::pause(&app.session, id).await
            };
            if let Err(e) = r {
                tracing::warn!("compat pause/resume {hash}: {e}");
            }
        }
    }
}

fn ct_multipart(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("multipart"))
        .unwrap_or(false)
}

// --- парсеры форм: urlencoded + бинарно-безопасный multipart, без новых крейтов ---

#[derive(Default)]
pub(crate) struct Form {
    fields: Vec<(String, String)>,
    /// первый файл (filename=) сырыми байтами
    file: Option<Bytes>,
}

impl Form {
    fn get_first(&self, name: &str) -> Option<String> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
    fn get_all(&self, name: &str) -> Vec<String> {
        self.fields
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

fn parse_form(body: &Bytes) -> Form {
    let mut f = Form::default();
    for pair in std::str::from_utf8(body).unwrap_or("").split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        f.fields.push((percent_decode(k), percent_decode(v)));
    }
    f
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() || from > haystack.len() - needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

fn boundary_of(content_type: &str) -> Option<Vec<u8>> {
    content_type
        .split(';')
        .map(|s| s.trim())
        .find_map(|s| s.strip_prefix("boundary="))
        .map(|b| format!("--{}", b.trim_matches('"')).into_bytes())
}

fn parse_multipart_with_boundary(body: &Bytes, boundary: &[u8]) -> Form {
    let mut form = Form::default();
    let mut pos = find(body, boundary, 0).unwrap_or(body.len());
    while pos < body.len() {
        // после boundary: либо "--" (конец), либо "\r\n" + part
        let after = pos + boundary.len();
        if body.get(after..after + 2) == Some(b"--") {
            break;
        }
        let head_start = after + 2; // \r\n
        let Some(head_end) = find(body, b"\r\n\r\n", head_start) else { break };
        let head = String::from_utf8_lossy(&body[head_start..head_end]).to_string();
        let data_start = head_end + 4;
        let data_end = find(body, boundary, data_start).unwrap_or(body.len());
        // перед следующим boundary стоит \r\n — он не часть данных
        let data_end = data_end.saturating_sub(2);
        let name = head
            .split(';')
            .map(|s| s.trim())
            .find_map(|s| s.strip_prefix("name=\""))
            .map(|n| n.trim_end_matches('"').to_string())
            .unwrap_or_default();
        let is_file = head.contains("filename=");
        if is_file {
            if form.file.is_none() {
                form.file = Some(Bytes::copy_from_slice(&body[data_start..data_end]));
            }
        } else if !name.is_empty() {
            let value = String::from_utf8_lossy(&body[data_start..data_end]).into_owned();
            form.fields.push((name, value));
        }
        pos = find(body, boundary, data_end + 2).unwrap_or(body.len());
    }
    form
}

/// Единая точка: multipart (binary-safe) или urlencoded.
pub fn form_from(headers: &HeaderMap, body: &Bytes) -> Form {
    if ct_multipart(headers) {
        let ct = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        match boundary_of(ct) {
            Some(b) => parse_multipart_with_boundary(body, &b),
            None => Form::default(),
        }
    } else {
        parse_form(body)
    }
}
