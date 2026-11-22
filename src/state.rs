use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const USER_RUNNING: &str = "running";
pub const USER_PAUSED: &str = "paused";

/// Всё наше сохранённое состояние. librqbit сам восстанавливает торренты
/// (SessionPersistenceConfig::Json), здесь только то, чего он не знает:
/// владельцы (подписчики), категории, локальные пользователи, секрет.
#[derive(Serialize, Deserialize, Default)]
pub struct Persisted {
    /// учётки: argon2-хеш + роль ("admin"|"user"). Для AD-юзеров hash = None.
    #[serde(default)]
    pub users: HashMap<String, UserRec>,
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

#[derive(Serialize, Clone)]
pub struct UserRec {
    /// argon2 PHC-хеш; None = доменный пользователь (пароль в АД)
    pub hash: Option<String>,
    pub role: String, // "admin" | "user"
}

// Старые state.json хранили users как {name: "hash"} — принимаем оба формата,
// чтобы не терять торренты при обновлении.
impl<'de> Deserialize<'de> for UserRec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct New {
            hash: Option<String>,
            #[serde(default = "default_role")]
            role: String,
        }
        fn default_role() -> String {
            "user".into()
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Any {
            New(New),
            OldHash(String),
        }
        Ok(match Any::deserialize(d)? {
            Any::New(n) => UserRec { hash: n.hash, role: n.role },
            Any::OldHash(h) => UserRec { hash: Some(h), role: "user".into() },
        })
    }
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
    /// per-user пауза: юзер -> "running"|"paused". Движок ставится на паузу,
    /// только когда паузу хотят ВСЕ подписчики.
    #[serde(default)]
    pub user_states: HashMap<String, String>,
    /// сид-лимит сработал: причина ("ratio"|"time"). Снимается resume'ом.
    #[serde(default)]
    pub seeding_paused: Option<String>,
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
                // новый подписчик по умолчанию хочет качать
                existing
                    .user_states
                    .insert(user.to_string(), USER_RUNNING.to_string());
            }
            false
        }
        None => {
            p.torrents.insert(hash.to_string(), rec);
            true
        }
    }
}

/// Per-user пауза. true = движку нужно поставить паузу (паузу хотят все).
pub fn user_pause(p: &mut Persisted, hash: &str, user: &str) -> bool {
    if let Some(rec) = p.torrents.get_mut(hash) {
        rec.user_states.insert(user.to_string(), USER_PAUSED.to_string());
        desired_running(rec)
    } else {
        true
    }
}

/// Per-user resume. true = движку надо снять паузу (есть хоть один желающий).
pub fn user_resume(p: &mut Persisted, hash: &str, user: &str) -> bool {
    if let Some(rec) = p.torrents.get_mut(hash) {
        rec.user_states.insert(user.to_string(), USER_RUNNING.to_string());
        rec.seeding_paused = None; // ручной resume перекрывает сработавший лимит
        desired_running(rec)
    } else {
        true
    }
}

/// Общее желаемое состояние: работает, если ХОТЬ ОДИН подписчик хочет.
fn desired_running(rec: &TorrentRec) -> bool {
    rec.subscribers
        .iter()
        .any(|s| rec.user_states.get(s).map(|v| v == USER_RUNNING).unwrap_or(true))
}

/// Отписка. true = ещё есть подписчики (торрент живёт).
pub fn unsubscribe(p: &mut Persisted, hash: &str, user: &str) -> bool {
    let empty = if let Some(rec) = p.torrents.get_mut(hash) {
        rec.subscribers.retain(|s| s != user);
        rec.user_states.remove(user);
        rec.subscribers.is_empty()
    } else {
        return false;
    };
    if empty {
        p.torrents.remove(hash);
    }
    !empty
}

// --- управление пользователями (админка) ---

pub fn is_admin(p: &Persisted, user: &str) -> bool {
    p.users.get(user).map(|u| u.role == "admin").unwrap_or(false)
}

/// Создать/обновить пользователя. Пароль хешируется argon2.
pub fn user_add(p: &mut Persisted, user: &str, pass: &str, role: &str) -> Result<(), &'static str> {
    if !crate::auth::valid_username(user) {
        return Err("bad username");
    }
    if pass.len() < 4 {
        return Err("password too short (min 4)");
    }
    let salt = argon2::password_hash::SaltString::generate(&mut rand_core::OsRng);
    let hash = argon2::Argon2::default()
        .hash_password(pass.as_bytes(), &salt)
        .map_err(|_| "hash error")?
        .to_string();
    p.users.insert(
        user.to_string(),
        UserRec { hash: Some(hash), role: role.to_string() },
    );
    Ok(())
}

pub fn user_set_password(p: &mut Persisted, user: &str, pass: &str) -> Result<(), &'static str> {
    if pass.len() < 4 {
        return Err("password too short (min 4)");
    }
    let salt = argon2::password_hash::SaltString::generate(&mut rand_core::OsRng);
    let hash = argon2::Argon2::default()
        .hash_password(pass.as_bytes(), &salt)
        .map_err(|_| "hash error")?
        .to_string();
    match p.users.get_mut(user) {
        Some(rec) => {
            rec.hash = Some(hash);
            Ok(())
        }
        None => Err("no such user"),
    }
}

/// Удалить пользователя: отписка от всех его торрентов (опустевшие удаляются
/// с файлами), его категории стираются. Ошибки:
/// "self" — нельзя удалить себя, "last admin" — нельзя оставить систему без админа.
pub fn user_delete(p: &mut Persisted, actor: &str, user: &str) -> Result<Vec<usize>, &'static str> {
    if user == actor {
        return Err("self");
    }
    if is_admin(p, user) && p.users.values().filter(|u| u.role == "admin").count() <= 1 {
        return Err("last admin");
    }
    let mut orphaned: Vec<usize> = Vec::new();
    let hashes: Vec<String> = p
        .torrents
        .iter()
        .filter(|(_, r)| r.subscribers.iter().any(|s| s == user))
        .map(|(h, _)| h.clone())
        .collect();
    for h in hashes {
        let still_used = unsubscribe(p, &h, user);
        if !still_used {
            if let Some(rec) = p.torrents.get(&h) {
                orphaned.push(rec.id);
            }
            p.torrents.remove(&h);
        }
    }
    p.users.remove(user);
    p.categories.remove(user);
    Ok(orphaned)
}

/// Роль при входе: local — из записи; ad — admin если в списке ADMINS, иначе user.
pub fn role_on_login(p: &mut Persisted, user: &str, admins: &[String]) -> String {
    match p.users.get_mut(user) {
        Some(rec) if rec.role == "admin" => "admin".into(),
        Some(rec) => {
            if admins.iter().any(|a| a == user) {
                rec.role = "admin".into();
                "admin".into()
            } else {
                "user".into()
            }
        }
        // первый вход доменного пользователя
        None => {
            let role = if admins.iter().any(|a| a == user) { "admin" } else { "user" };
            p.users.insert(
                user.to_string(),
                UserRec { hash: None, role: role.into() },
            );
            role.into()
        }
    }
}

use argon2::PasswordHasher;
use rand_core::RngCore;

pub fn init(config: &Config) -> anyhow::Result<Persisted> {
    fs::create_dir_all(&config.downloads_dir)?;
    fs::create_dir_all(&config.persist_dir)?;
    let mut p = load(&config.persist_dir);
    // local-режим: сидируем из env ТОЛЬКО если пользователей ещё нет —
    // дальше правды в админке (env перезаписал бы созданных в UI).
    // Первый в LOCAL_USERS становится админом.
    if config.auth_mode == crate::config::AuthMode::Local && p.users.is_empty() {
        for (i, (user, pass)) in config.local_users.iter().enumerate() {
            let _ = user_add(
                &mut p,
                user,
                pass,
                if i == 0 { "admin" } else { "user" },
            );
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
                user_states: HashMap::new(),
                seeding_paused: None,
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
            user_states: HashMap::new(),
            seeding_paused: None,
        };
        assert!(subscribe(&mut s, "h", rec(vec!["a"]), "a"));
        assert!(!subscribe(&mut s, "h", rec(vec![]), "b")); // дедуп: второй юзер
        assert_eq!(s.torrents["h"].subscribers, vec!["a", "b"]);
        assert!(unsubscribe(&mut s, "h", "a")); // b ещё пользуется
        assert!(s.torrents.contains_key("h"));
        assert!(!unsubscribe(&mut s, "h", "b")); // последний — можно удалять
        assert!(!s.torrents.contains_key("h"));
    }

    #[test]
    fn per_user_pause_engine_only_when_all_paused() {
        let mut s = p();
        let rec = |subs: Vec<&str>| TorrentRec {
            id: 1,
            category: None,
            subscribers: subs.iter().map(|s| s.to_string()).collect(),
            added_at: 0,
            completed_at: None,
            user_states: HashMap::new(),
            seeding_paused: None,
        };
        subscribe(&mut s, "h", rec(vec!["a"]), "a");
        subscribe(&mut s, "h", rec(vec![]), "b");
        // a ставит паузу — b хочет работать: движок НЕ на паузе
        assert!(user_pause(&mut s, "h", "a"));
        // b тоже — теперь все на паузе: движку пауза
        assert!(!user_pause(&mut s, "h", "b"));
        // a снял — движок снова работает
        assert!(user_resume(&mut s, "h", "a"));
        // новый подписчик по умолчанию running -> тоже работает
        subscribe(&mut s, "h", rec(vec![]), "c");
        assert_eq!(s.torrents["h"].user_states["c"], USER_RUNNING);
        // resume снимает сработавший сид-лимит
        s.torrents.get_mut("h").unwrap().seeding_paused = Some("ratio".into());
        user_resume(&mut s, "h", "b");
        assert_eq!(s.torrents["h"].seeding_paused, None);
    }
}
