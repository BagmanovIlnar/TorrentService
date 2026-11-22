use crate::auth::CurrentUser;
use crate::state::{self, TorrentRec};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::torrent::{TorrentInfo, SharedSession};
use librqbit::ManagedTorrent;

#[derive(Clone)]
pub struct App {
    pub session: SharedSession,
    pub persist: Arc<Mutex<state::Persisted>>,
    pub downloads_dir: PathBuf,
    pub persist_dir: PathBuf,
    pub auth: crate::auth::AuthState,
}

impl App {
    pub fn persist_dir(&self) -> PathBuf {
        self.persist_dir.clone()
    }
}

/// Свой торрент по хешу: (запись, хендлер сессии). Чужой -> None (=> 404, не светим).
fn own_torrent(
    app: &App,
    user: &str,
    hash: &str,
) -> Option<(TorrentRec, Arc<ManagedTorrent>)> {
    let p = app.persist.lock().unwrap();
    let rec = p.torrents.get(hash)?;
    if !rec.subscribers.iter().any(|s| s == user) {
        return None;
    }
    let t = app.session.get(rec.id.into())?;
    Some((rec.clone(), t))
}

// --- торренты ---

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    /// фильтр по категории (включая подкатегории)
    pub category: Option<String>,
}

/// GET /torrents — только те, на которые подписан текущий юзер.
#[utoipa::path(get, path = "/torrents", params(ListQuery),
    responses((status = 200, body = [TorrentInfo])))]
pub async fn get_torrents(
    user: CurrentUser,
    State(app): State<App>,
    Query(q): Query<ListQuery>,
) -> Json<Vec<TorrentInfo>> {
    let mut p = app.persist.lock().unwrap();
    let (mut infos, finished) =
        crate::torrent::list_user(&app.session, &p, &user.0, q.category.as_deref());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut dirty = false;
    for h in finished {
        if let Some(rec) = p.torrents.get_mut(&h) {
            if rec.completed_at.is_none() {
                rec.completed_at = Some(now);
                dirty = true;
            }
        }
    }
    if dirty {
        state::save(&app.persist_dir(), &p).ok();
    }
    infos.sort_by(|a, b| b.added_at.cmp(&a.added_at));
    Json(infos)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AddTorrentReq {
    /// magnet-ссылка или infohash
    pub magnet: String,
    /// своя категория: файлы лягут в downloads/<category>/
    pub category: Option<String>,
}

/// POST /torrents — дедуп по хешу: если торрент уже качает другой юзер,
/// новый подписывается на тот же экземпляр, повторной загрузки нет.
#[utoipa::path(post, path = "/torrents", request_body = AddTorrentReq,
    responses((status = 200, body = TorrentInfo), (status = 400), (status = 403)))]
pub async fn post_torrents(
    user: CurrentUser,
    State(app): State<App>,
    Json(req): Json<AddTorrentReq>,
) -> Response {
    let category = match req.category {
        Some(c) => {
            let owned = app
                .persist
                .lock()
                .unwrap()
                .categories
                .get(&user.0)
                .map(|list| list.contains(&c))
                .unwrap_or(false);
            if !owned {
                return (StatusCode::FORBIDDEN, "no such category").into_response();
            }
            Some(c)
        }
        None => None,
    };
    let source = librqbit::AddTorrent::from_url(req.magnet);
    let (hash, id) =
        match crate::torrent::add(&app.session, source, &app.downloads_dir, category.clone()).await {
            Ok(x) => x,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        };
    let info = {
        let mut p = app.persist.lock().unwrap();
        state::subscribe(
            &mut p,
            &hash,
            TorrentRec {
                id,
                category: category.clone(),
                subscribers: vec![user.0.clone()],
                added_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                completed_at: None,
                user_states: std::collections::HashMap::new(),
                seeding_paused: None,
            },
            &user.0,
        );
        let rec = p.torrents.get(&hash).unwrap().clone();
        state::save(&app.persist_dir(), &p).ok();
        app.session
            .get(id.into())
            .map(|t| crate::torrent::build_info(&hash, &rec, &t, Some(&user.0)))
    };
    match info {
        Some(i) => Json(i).into_response(),
        None => (StatusCode::ACCEPTED, hash).into_response(),
    }
}

/// POST /torrents/file — загрузка .torrent-файла (multipart: file, category)
#[utoipa::path(post, path = "/torrents/file", request_body = [String],
    responses((status = 200, body = TorrentInfo), (status = 400), (status = 403)))]
pub async fn post_torrent_file(
    user: CurrentUser,
    State(app): State<App>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let mut category: Option<String> = None;
    let mut bytes: Option<axum::body::Bytes> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().unwrap_or_default() {
            "category" => category = { let t = field.text().await.unwrap_or_default(); Some(t).filter(|s| !s.is_empty()) },
            "file" => bytes = field.bytes().await.ok(),
            _ => {}
        }
    }
    let Some(bytes) = bytes else {
        return (StatusCode::BAD_REQUEST, "no file").into_response();
    };
    if let Some(c) = &category {
        let owned = app
            .persist
            .lock()
            .unwrap()
            .categories
            .get(&user.0)
            .map(|list| list.contains(c))
            .unwrap_or(false);
        if !owned {
            return (StatusCode::FORBIDDEN, "no such category").into_response();
        }
    }
    let source = librqbit::AddTorrent::from_bytes(bytes);
    let (hash, id) =
        match crate::torrent::add(&app.session, source, &app.downloads_dir, category.clone()).await {
            Ok(x) => x,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        };
    let info = {
        let mut p = app.persist.lock().unwrap();
        state::subscribe(
            &mut p,
            &hash,
            TorrentRec {
                id,
                category: category.clone(),
                subscribers: vec![user.0.clone()],
                added_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                completed_at: None,
                user_states: std::collections::HashMap::new(),
                seeding_paused: None,
            },
            &user.0,
        );
        let rec = p.torrents.get(&hash).unwrap().clone();
        state::save(&app.persist_dir(), &p).ok();
        app.session
            .get(id.into())
            .map(|t| crate::torrent::build_info(&hash, &rec, &t, Some(&user.0)))
    };
    match info {
        Some(i) => Json(i).into_response(),
        None => (StatusCode::ACCEPTED, hash).into_response(),
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct DeleteQuery {
    /// true — стереть файлы с диска (только если подписчик последний)
    #[serde(default)]
    pub delete_files: bool,
}

/// DELETE /torrents/{hash} — отписка; файлы и закачка сносятся,
/// только если подписчиков не осталось.
#[utoipa::path(delete, path = "/torrents/{hash}", params(("hash" = String, Path, description = "infohash"), DeleteQuery),
    responses((status = 200), (status = 404)))]
#[axum::debug_handler]
pub async fn delete_torrent(
    user: CurrentUser,
    State(app): State<App>,
    Path(hash): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Response {
    let Some((rec, _)) = own_torrent(&app, &user.0, &hash) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let still_used = {
        let mut p = app.persist.lock().unwrap();
        let used = state::unsubscribe(&mut p, &hash, &user.0);
        state::save(&app.persist_dir(), &p).ok();
        used
    };
    if !still_used {
        crate::torrent::remove(&app.session, rec.id, q.delete_files).await.ok();
    }
    StatusCode::OK.into_response()
}

/// POST /torrents/{hash}/pause — персональная пауза: движок встаёт на паузу,
/// только если паузу захотели ВСЕ подписчики.
#[utoipa::path(post, path = "/torrents/{hash}/pause", responses((status = 200), (status = 404)))]
pub async fn post_pause(
    user: CurrentUser,
    State(app): State<App>,
    Path(hash): Path<String>,
) -> Response {
    let Some((rec, _)) = own_torrent(&app, &user.0, &hash) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let desired_running = {
        let mut p = app.persist.lock().unwrap();
        let d = state::user_pause(&mut p, &hash, &user.0);
        state::save(&app.persist_dir(), &p).ok();
        d
    };
    if !desired_running {
        crate::torrent::pause(&app.session, rec.id).await.ok();
    }
    StatusCode::OK.into_response()
}

/// POST /torrents/{hash}/resume — снимает вашу паузу и сработавший сид-лимит;
/// движок оживает, если есть хоть один желающий качать.
#[utoipa::path(post, path = "/torrents/{hash}/resume", responses((status = 200), (status = 404)))]
pub async fn post_resume(
    user: CurrentUser,
    State(app): State<App>,
    Path(hash): Path<String>,
) -> Response {
    let Some((rec, _)) = own_torrent(&app, &user.0, &hash) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let desired_running = {
        let mut p = app.persist.lock().unwrap();
        let d = state::user_resume(&mut p, &hash, &user.0);
        state::save(&app.persist_dir(), &p).ok();
        d
    };
    if desired_running {
        crate::torrent::resume(&app.session, rec.id).await.ok();
    }
    StatusCode::OK.into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetCategoryReq {
    /// null = убрать метку. Файлы не перемещаются.
    pub category: Option<String>,
}

/// POST /torrents/{hash}/category — смена категории с перемещением файлов.
/// Завершённые: файлы переносятся физически. Качающиеся: 409.
#[utoipa::path(post, path = "/torrents/{hash}/category", request_body = SetCategoryReq,
    responses((status = 200), (status = 403), (status = 404), (status = 409)))]
pub async fn post_category(
    user: CurrentUser,
    State(app): State<App>,
    Path(hash): Path<String>,
    Json(req): Json<SetCategoryReq>,
) -> Response {
    let Some((rec, t)) = own_torrent(&app, &user.0, &hash) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some(c) = &req.category {
        let owned = app
            .persist
            .lock()
            .unwrap()
            .categories
            .get(&user.0)
            .map(|list| list.contains(c))
            .unwrap_or(false);
        if !owned {
            return (StatusCode::FORBIDDEN, "no such category").into_response();
        }
    }
    // перенос файлов — только для завершённых: качающийся пишет в старые пути
    let stats = t.stats();
    if !stats.finished && stats.progress_bytes < stats.total_bytes {
        return (
            StatusCode::CONFLICT,
            "торрент ещё качается: перенос файлов возможен после завершения",
        )
            .into_response();
    }
    let old_dir = match &rec.category {
        Some(c) => app.downloads_dir.join(c),
        None => app.downloads_dir.clone(),
    };
    let new_dir = match &req.category {
        Some(c) => app.downloads_dir.join(c),
        None => app.downloads_dir.clone(),
    };
    if old_dir != new_dir {
        if let Err(e) = crate::torrent::move_torrent_files(&t, &old_dir, &new_dir) {
            return (StatusCode::CONFLICT, e.to_string()).into_response();
        }
    }
    let mut p = app.persist.lock().unwrap();
    if let Some(r) = p.torrents.get_mut(&hash) {
        r.category = req.category;
    }
    state::save(&app.persist_dir(), &p).ok();
    StatusCode::OK.into_response()
}

// --- категории ---

/// GET /categories — свои категории
#[utoipa::path(get, path = "/categories", responses((status = 200, body = [String])))]
pub async fn get_categories(user: CurrentUser, State(app): State<App>) -> Json<Vec<String>> {
    let p = app.persist.lock().unwrap();
    Json(p.categories.get(&user.0).cloned().unwrap_or_default())
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct NameReq {
    /// вложенность = "/" в имени ("movies/action")
    pub name: String,
}

/// POST /categories — ancestors создаются автоматически
#[utoipa::path(post, path = "/categories", request_body = NameReq, responses((status = 200)))]
pub async fn post_categories(
    user: CurrentUser,
    State(app): State<App>,
    Json(req): Json<NameReq>,
) -> Response {
    if req.name.trim().is_empty() || req.name.len() > 256 {
        return (StatusCode::BAD_REQUEST, "bad name").into_response();
    }
    let mut p = app.persist.lock().unwrap();
    state::category_add(&mut p, &user.0, req.name.trim());
    state::save(&app.persist_dir(), &p).ok();
    StatusCode::OK.into_response()
}

/// DELETE /categories/{name} — торренты отвязываются, файлы не трогаются
#[utoipa::path(delete, path = "/categories/{name}", responses((status = 200)))]
pub async fn delete_categories(
    user: CurrentUser,
    State(app): State<App>,
    Path(name): Path<String>,
) -> Response {
    let mut p = app.persist.lock().unwrap();
    state::category_delete(&mut p, &user.0, &name);
    state::save(&app.persist_dir(), &p).ok();
    StatusCode::OK.into_response()
}
