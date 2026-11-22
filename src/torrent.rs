use crate::config::Config;
use crate::state::{Persisted, TorrentRec};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session,
    SessionOptions, SessionPersistenceConfig,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

pub type SharedSession = Arc<Session>;

pub async fn init(cfg: &Config) -> anyhow::Result<SharedSession> {
    let opts = SessionOptions {
        // торренты восстанавливаются при старте сами (json в persist_dir/session)
        persistence: Some(SessionPersistenceConfig::Json {
            folder: Some(cfg.persist_dir.join("session")),
        }),
        listen: Some(librqbit::ListenerOptions {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], cfg.bt_port)),
            ..Default::default()
        }),
        ..Default::default()
    };
    let session = Session::new_with_opts(cfg.downloads_dir.clone(), opts).await?;
    Ok(session)
}

#[derive(Serialize, Clone, utoipa::ToSchema)]
pub struct TorrentInfo {
    pub hash: String,
    pub id: usize,
    pub name: String,
    pub category: Option<String>,
    pub subscribers: Vec<String>,
    /// 0..1
    pub progress: f64,
    pub size: u64,
    pub downloaded: u64,
    pub uploaded: u64,
    pub ratio: f64,
    /// байт/сек
    pub down_speed: u64,
    pub up_speed: u64,
    /// сек, 0 = неизвестно/готово
    pub eta_sec: u64,
    /// подключённые пиры
    pub peers: u32,
    pub seeders: u32,
    pub status: String, // downloading|seeding|paused|error
    pub error: Option<String>,
    pub added_at: u64,
    pub completed_at: Option<u64>,
}

/// Добавить magnet или .torrent-байты. Дедуп внутри librqbit: AlreadyManaged
/// означает, что закачка уже есть — юзер просто подписывается на неё
/// (подписка — в state::subscribe, её делает вызывающий без await под локом).
pub async fn add(
    session: &SharedSession,
    source: AddTorrent<'_>,
    downloads_dir: &Path,
    category: Option<String>,
) -> anyhow::Result<(String, usize)> {
    let output_folder = category
        .as_ref()
        .map(|c| downloads_dir.join(c).display().to_string());
    let opts = AddTorrentOptions {
        output_folder,
        overwrite: true, // нужно для докачки/сидирования существующих файлов
        ..Default::default()
    };
    let resp = session.add_torrent(source, Some(opts)).await?;
    let (id, torrent) = match resp {
        AddTorrentResponse::AlreadyManaged(id, t) | AddTorrentResponse::Added(id, t) => (id, t),
        AddTorrentResponse::ListOnly(r) => {
            anyhow::bail!("list-only response, unexpected: {}", hex::encode(r.info_hash.0))
        }
    };
    let hash = hex::encode(torrent.info_hash().0);
    Ok((hash, id))
}

pub async fn pause(session: &SharedSession, id: usize) -> anyhow::Result<()> {
    let t = get_by_id(session, id)?;
    session.pause(&t).await
}

pub async fn resume(session: &SharedSession, id: usize) -> anyhow::Result<()> {
    let t = get_by_id(session, id)?;
    session.unpause(&t).await
}

/// Удалить из сессии; files=true — стереть файлы с диска.
pub async fn remove(session: &SharedSession, id: usize, files: bool) -> anyhow::Result<()> {
    session.delete(id.into(), files).await
}

fn get_by_id(session: &SharedSession, id: usize) -> anyhow::Result<Arc<ManagedTorrent>> {
    session
        .get(id.into())
        .ok_or_else(|| anyhow::anyhow!("torrent {id} not in session"))
}

/// Список торрентов пользователя (там, где он в subscribers).
/// Если торрент закончился, а completed_at ещё пуст — проставляем (возвращает хеши к обновлению).
pub fn list_user(
    session: &SharedSession,
    persist: &Persisted,
    user: &str,
    category_filter: Option<&str>,
) -> (Vec<TorrentInfo>, Vec<String>) {
    let mut out = Vec::new();
    let mut newly_finished = Vec::new();
    for (hash, rec) in &persist.torrents {
        if !rec.subscribers.iter().any(|s| s == user) {
            continue;
        }
        if let Some(f) = category_filter {
            if !crate::state::category_matches(rec.category.as_deref(), f) {
                continue;
            }
        }
        let Some(t) = session.get(rec.id.into()) else {
            continue; // в state есть, в сессии нет: торрент добавили в этом старте? нет — пропускаем
        };
        let info = build_info(hash, rec, &t);
        if info.status == "seeding" && rec.completed_at.is_none() {
            newly_finished.push(hash.clone());
        }
        out.push(info);
    }
    out.sort_by(|a, b| b.added_at.cmp(&a.added_at));
    (out, newly_finished)
}

pub fn build_info(hash: &str, rec: &TorrentRec, t: &ManagedTorrent) -> TorrentInfo {
    let stats = t.stats();
    let name = t.name().unwrap_or_else(|| hash[..12].to_string());
    let progress = if stats.total_bytes > 0 {
        stats.progress_bytes as f64 / stats.total_bytes as f64
    } else {
        0.0
    };
    let (down, up) = stats
        .live
        .as_ref()
        .map(|l| (l.download_speed.as_bytes(), l.upload_speed.as_bytes()))
        .unwrap_or((0, 0));
    let (peers, seeders) = stats
        .live
        .as_ref()
        .map(|l| (l.snapshot.peer_stats.live, l.snapshot.peer_stats.live_seeders))
        .unwrap_or((0, 0));
    let eta_sec = if progress < 1.0 && down > 0 {
        let remaining = stats.total_bytes.saturating_sub(stats.progress_bytes);
        remaining / down
    } else {
        0
    };
    let status = match stats.state {
        librqbit::TorrentStatsState::Error => "error",
        librqbit::TorrentStatsState::Paused => "paused",
        _ if stats.finished || progress >= 1.0 => "seeding",
        _ => "downloading",
    };
    TorrentInfo {
        hash: hash.to_string(),
        id: rec.id,
        name,
        category: rec.category.clone(),
        subscribers: rec.subscribers.clone(),
        progress,
        size: stats.total_bytes,
        downloaded: stats.progress_bytes,
        uploaded: stats.uploaded_bytes,
        ratio: stats.uploaded_bytes as f64 / stats.progress_bytes.max(1) as f64,
        down_speed: down,
        up_speed: up,
        eta_sec,
        peers,
        seeders,
        status: status.to_string(),
        error: stats.error,
        added_at: rec.added_at,
        completed_at: rec.completed_at,
    }
}
