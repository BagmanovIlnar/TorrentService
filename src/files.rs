use crate::auth::CurrentUser;
use crate::api::App;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Разрешить путь только внутри DOWNLOADS_DIR: canonicalize закрывает и "../",
/// и симлинки. Это граница доверия — DELETE сюда без проверки нельзя.
pub fn resolve_safe(root_canonical: &Path, root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.trim_start_matches('/');
    anyhow::ensure!(!rel.contains('\0'), "bad path");
    let candidate = root.join(rel);
    let canonical = candidate.canonicalize()?;
    anyhow::ensure!(canonical.starts_with(root_canonical), "path escapes root");
    Ok(canonical)
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct FilesQuery {
    /// относительный путь внутри downloads/; пусто = корень
    pub path: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct EntryInfo {
    pub name: String,
    /// относительный путь — передавать обратно в ?path=
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// unix epoch
    pub modified: u64,
}

/// GET /files — содержимое каталога в downloads/
#[utoipa::path(get, path = "/files", params(FilesQuery), responses((status = 200, body = [EntryInfo]), (status = 403)))]
pub async fn get_files(
    _user: CurrentUser,
    State(app): State<App>,
    Query(q): Query<FilesQuery>,
) -> Response {
    let root = match app.downloads_dir.canonicalize() {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let dir = match resolve_safe(&root, &app.downloads_dir, q.path.as_deref().unwrap_or("")) {
        Ok(p) => p,
        Err(_) => return StatusCode::FORBIDDEN.into_response(),
    };
    let mut entries = Vec::new();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = match entry.path().strip_prefix(&root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        entries.push(EntryInfo {
            name,
            path: rel,
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Json(entries).into_response()
}

/// DELETE /files — удалить файл или каталог (рекурсивно)
#[utoipa::path(delete, path = "/files", params(FilesQuery), responses((status = 200), (status = 403), (status = 404)))]
pub async fn delete_file(
    _user: CurrentUser,
    State(app): State<App>,
    Query(q): Query<FilesQuery>,
) -> Response {
    let Some(rel) = q.path.filter(|p| !p.trim().is_empty()) else {
        return (StatusCode::BAD_REQUEST, "path required").into_response();
    };
    let root = match app.downloads_dir.canonicalize() {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let target = match resolve_safe(&root, &app.downloads_dir, &rel) {
        Ok(p) => p,
        Err(_) => return StatusCode::FORBIDDEN.into_response(),
    };
    if target == root {
        return StatusCode::FORBIDDEN.into_response();
    }
    let res = if target.is_dir() {
        fs::remove_dir_all(&target)
    } else {
        fs::remove_file(&target)
    };
    match res {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_traversal_blocked() {
        let tmp = std::env::temp_dir().join(format!("ts_test_{}", std::process::id()));
        fs::create_dir_all(tmp.join("sub")).unwrap();
        let root = tmp.canonicalize().unwrap();
        // существующий путь внутри — ок
        assert!(resolve_safe(&root, &tmp, "sub").is_ok());
        assert!(resolve_safe(&root, &tmp, "").is_ok());
        // .. наружу — нет
        assert!(resolve_safe(&root, &tmp, "../").is_err());
        assert!(resolve_safe(&root, &tmp, "sub/../../etc").is_err());
        // абсолютный путь снаружи — нет
        assert!(resolve_safe(&root, &tmp, "/etc/passwd").is_err());
        // несуществующий — ошибка (canonicalize), не паника
        assert!(resolve_safe(&root, &tmp, "nope").is_err());
        // симлинк наружу — нет
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", tmp.join("evil")).unwrap();
            assert!(resolve_safe(&root, &tmp, "evil/passwd").is_err());
        }
        fs::remove_dir_all(&tmp).ok();
    }
}
