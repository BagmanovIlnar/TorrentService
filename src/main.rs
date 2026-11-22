mod api;
mod auth;
mod compat;
mod config;
mod files;
mod state;
mod torrent;

use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use std::sync::Arc;
use tower_http::services::ServeDir;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        auth::login,
        api::post_torrent_file,
        auth::logout,
        api::get_torrents,
        api::post_torrents,
        api::delete_torrent,
        api::post_pause,
        api::post_resume,
        api::post_category,
        api::get_categories,
        api::post_categories,
        api::delete_categories,
        files::get_files,
        files::delete_file,
    ),
    components(schemas(
        auth::LoginReq,
        torrent::TorrentInfo,
        api::AddTorrentReq,
        api::SetCategoryReq,
        api::NameReq,
        files::EntryInfo,
    )),
    info(title = "TorrentService", description = "Мультипользовательский веб-клиент торрентов (librqbit). Категории с вложенностью через '/', дедуп торрентов между пользователями, изоляция, qBittorrent-совместимый слой для *arr на /api/v2.")
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cfg = config::Config::from_env()?;
    let persist = state::init(&cfg)?;
    let session = torrent::init(&cfg).await?;
    let auth_state = auth::AuthState {
        secret: auth::secret_from(&cfg, &persist),
        mode: cfg.auth_mode.clone(),
        ad_url: cfg.ad_url.clone(),
        ad_domain: cfg.ad_domain.clone(),
    };
    let app_state = api::App {
        session,
        persist: Arc::new(std::sync::Mutex::new(persist)),
        downloads_dir: cfg.downloads_dir.clone(),
        persist_dir: cfg.persist_dir.clone(),
        auth: auth_state.clone(),
    };

    let app = Router::new()
        .route("/torrents", get(api::get_torrents).post(api::post_torrents))
        .route("/torrents/file", post(api::post_torrent_file))
        .route("/torrents/{hash}", delete(api::delete_torrent))
        .route("/torrents/{hash}/pause", post(api::post_pause))
        .route("/torrents/{hash}/resume", post(api::post_resume))
        .route("/torrents/{hash}/category", post(api::post_category))
        .route("/categories", get(api::get_categories).post(api::post_categories))
        .route("/categories/{name}", delete(api::delete_categories))
        .route("/files", get(files::get_files).delete(files::delete_file))
        .route(
            "/api/v2/app/version",
            get(compat::app_version),
        )
        .route("/api/v2/app/preferences", get(compat::app_preferences))
        .route("/api/v2/torrents/info", get(compat::torrents_info))
        .route("/api/v2/torrents/add", post(compat::torrents_add))
        .route("/api/v2/torrents/delete", post(compat::torrents_delete))
        .route("/api/v2/torrents/pause", post(compat::torrents_pause))
        .route("/api/v2/torrents/resume", post(compat::torrents_resume))
        // route_layer действует только на маршруты выше: /torrents, /categories, /files, /api/v2/*
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth::require_auth,
        ))
        // дальше — публичные
        .route("/health", get(|| async { "ok" }))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/api/v2/auth", post(compat::v2_auth))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .fallback_service(ServeDir::new("static").append_index_html_on_directories(true))
        .with_state(app_state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!(
        "listening on http://{addr} (auth={:?}, downloads={}, persist={})",
        cfg.auth_mode,
        cfg.downloads_dir.display(),
        cfg.persist_dir.display()
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
