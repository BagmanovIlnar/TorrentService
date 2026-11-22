use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Всё наше сохранённое состояние. librqbit сам восстанавливает торренты
/// (SessionPersistenceConfig::Json), здесь только то, чего он не знает:
/// владельцы (подписчики), категории, локальные пользователи, секрет.
#[derive(Serialize, Deserialize, Default)]
pub struct Persisted {
    /// local-режим: user -> argon2 PHC-хеш (из LOCAL_USERS при старте)
    #[serde(default)]
    pub users: HashMap<String, String>,
    /// HMAC-секрет, если не задан через SECRET env (сессии переживают рестарт)
    #[serde(default)]
    pub secret_hex: Option<String>,
    /// per-user категории
    #[serde(default)]
    pub categories: HashMap<String, Vec<String>>,
    /// hash -> запись о торренте
    #[serde(default)]
    pub torrents: HashMap<String, TorrentRec>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TorrentRec {
    /// внутренний id librqbit (в API наружу не торчит)
    pub id: usize,
    pub category: Option<String>,
    /// один экземпляр закачки на всех: дедуп по хешу
    pub subscribers: Vec<String>,
    pub added_at: u64,
    pub completed_at: Option<u64>,
}

/// ponytail: нет fsync — write-to-temp + rename; потеря state.json при пропадании
/// питания = торренты останутся (их помнит librqbit), подписчики слетят. Ок для дома.
pub fn save(persist_dir: &PathBuf, data: &Persisted) -> anyhow::Result<()> {
    let path = persist_dir.join("state.json");
    let tmp = persist_dir.join(".state.json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(data)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load(persist_dir: &PathBuf) -> Persisted {
    let path = persist_dir.join("state.json");
    fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

// --- чистые функции над Persisted (тестируются без движка) ---

/// Категории пользователя; ancestors создаются автоматически ("a/b" -> "a", "a/b")
pub fn category_add(p: &mut Persisted, user: &str, name: &str) {
    let list = p.categories.entry(user.to_string()).or_default();
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split('/') {
        if part.is_empty() {
            continue;
        }
        parts.push(part);
        let path = parts.join("/");
        if !list.contains(&path) {
            list.push(path);
        }
    }
}

/// Удалить категорию; торренты, на неё ссылающиеся, отвязываются.
pub fn category_delete(p: &mut Persisted, user: &str, name: &str) -> bool {
    let mut removed = false;
    if let Some(list) = p.categories.get_mut(user) {
        let before = list.len();
        list.retain(|c| c != name);
        removed = list.len() != before;
    }
    for rec in p.torrents.values_mut() {
        if rec.subscribers.contains(&user.to_string()) && rec.category.as_deref() == Some(name) {
            rec.category = None;
        }
    }
    removed
}

/// Фильтр категории: сам X + все подкатегории X ("x" ловит "x/y")
pub fn category_matches(rec_category: Option<&str>, filter: &str) -> bool {
    match rec_category {
        Some(c) => c == filter || c.starts_with(&format!("{filter}/")),
        None => false,
    }
}

/// Подписка юзера на торрент (дедуп: один экземпляр файлов на всех).
/// true = добавлен впервые в state (новый торрент).
pub fn subscribe(p: &mut Persisted, hash: &str, rec: TorrentRec, user: &str) -> bool {
    match p.torrents.get_mut(hash) {
        Some(existing) => {
            if !existing.subscribers.iter().any(|s| s == user) {
                existing.subscribers.push(user.to_string());
            }
            false
        }
        None => {
            p.torrents.insert(hash.to_string(), rec);
            true
        }
    }
}

/// Отписка. true = последний подписчик ушёл, торрент можно удалять физически.
pub fn unsubscribe(p: &mut Persisted, hash: &str, user: &str) -> bool {
    let empty = if let Some(rec) = p.torrents.get_mut(hash) {
        rec.subscribers.retain(|s| s != user);
        rec.subscribers.is_empty()
    } else {
        return true;
    };
    if empty {
        p.torrents.remove(hash);
    }
    !empty
}

use rand_core::RngCore;

pub fn init(config: &Config) -> anyhow::Result<Persisted> {
    fs::create_dir_all(&config.downloads_dir)?;
    fs::create_dir_all(&config.persist_dir)?;
    let mut p = load(&config.persist_dir);
    // local-режим: env — источник правды по пользователям, перезаписываем хеши
    if config.auth_mode == crate::config::AuthMode::Local {
        for (user, pass) in &config.local_users {
            let salt = argon2::password_hash::SaltString::generate(&mut rand_core::OsRng);
            use argon2::PasswordHasher;
            let hash = argon2::Argon2::default()
                .hash_password(pass.as_bytes(), &salt)?
                .to_string();
            p.users.insert(user.clone(), hash);
        }
    }
    if p.secret_hex.is_none() {
        let mut buf = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut buf);
        p.secret_hex = Some(hex::encode(buf));
        save(&config.persist_dir, &p)?;
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Persisted {
        Persisted::default()
    }

    #[test]
    fn categories_nested_and_delete_detaches() {
        let mut s = p();
        category_add(&mut s, "u", "movies/action");
        assert_eq!(
            s.categories["u"],
            vec!["movies".to_string(), "movies/action".to_string()]
        );
        s.torrents.insert(
            "h1".into(),
            TorrentRec {
                id: 1,
                category: Some("movies/action".into()),
                subscribers: vec!["u".into()],
                added_at: 0,
                completed_at: None,
            },
        );
        assert!(category_delete(&mut s, "u", "movies/action"));
        assert_eq!(s.torrents["h1"].category, None);
        assert!(!category_delete(&mut s, "u", "movies/action"));
    }

    #[test]
    fn category_filter_includes_subcategories() {
        assert!(category_matches(Some("movies/action/sci"), "movies"));
        assert!(category_matches(Some("movies"), "movies"));
        assert!(!category_matches(Some("movies2"), "movies"));
        assert!(!category_matches(None, "movies"));
    }

    #[test]
    fn subscribe_dedups_and_unsubscribe_refcounts() {
        let mut s = p();
        let rec = |subs: Vec<&str>| TorrentRec {
            id: 1,
            category: None,
            subscribers: subs.iter().map(|s| s.to_string()).collect(),
            added_at: 0,
            completed_at: None,
        };
        assert!(subscribe(&mut s, "h", rec(vec!["a"]), "a"));
        assert!(!subscribe(&mut s, "h", rec(vec![]), "b")); // дедуп: второй юзер
        assert_eq!(s.torrents["h"].subscribers, vec!["a", "b"]);
        assert!(unsubscribe(&mut s, "h", "a")); // b ещё пользуется
        assert!(s.torrents.contains_key("h"));
        assert!(!unsubscribe(&mut s, "h", "b")); // последний — можно удалять
        assert!(!s.torrents.contains_key("h"));
    }
}
