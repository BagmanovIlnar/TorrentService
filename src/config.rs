use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum AuthMode {
    Local,
    Ad,
}

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub bt_port: u16,
    pub downloads_dir: PathBuf,
    pub persist_dir: PathBuf,
    pub auth_mode: AuthMode,
    pub ad_url: String,
    pub ad_domain: String,
    pub local_users: Vec<(String, String)>,
    /// Секрет для HMAC-кук. None = сгенерировать при первом старте и сохранить.
    pub secret: Option<Vec<u8>>,
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let mode = env_or("AUTH_MODE", "local");
        let auth_mode = match mode.as_str() {
            "local" => AuthMode::Local,
            "ad" => AuthMode::Ad,
            other => anyhow::bail!("AUTH_MODE must be local|ad, got: {other}"),
        };
        let ad_url = env_or("AD_URL", "");
        if auth_mode == AuthMode::Ad && ad_url.is_empty() {
            anyhow::bail!("AUTH_MODE=ad requires AD_URL (e.g. ldap://service_samba_addc:389)");
        }
        let secret = env::var("SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| hex::decode(s.replace("0x", "")))
            .transpose()
            .map_err(|e| anyhow::anyhow!("SECRET must be hex (openssl rand -hex 32): {e}"))?;
        Ok(Self {
            port: env_or("PORT", "8280").parse()?,
            bt_port: env_or("BT_PORT", "6882").parse()?,
            downloads_dir: PathBuf::from(env_or("DOWNLOADS_DIR", "./downloads")),
            persist_dir: PathBuf::from(env_or("PERSIST_DIR", "./state")),
            auth_mode,
            ad_url,
            ad_domain: env_or("AD_DOMAIN", "HOME.ARPA"),
            local_users: parse_local_users(&env_or("LOCAL_USERS", "")),
            secret,
        })
    }
}

/// "user:pass,user2:pass2" -> [(user, pass)]
fn parse_local_users(s: &str) -> Vec<(String, String)> {
    s.split(',')
        .filter_map(|pair| pair.split_once(':'))
        .map(|(u, p)| (u.trim().to_string(), p.trim().to_string()))
        .filter(|(u, p)| !u.is_empty() && !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_users_parsing() {
        let got = parse_local_users("ilnar:pa:ss,wife: abc , ,x:");
        assert_eq!(
            got,
            vec![
                ("ilnar".into(), "pa:ss".into()),
                ("wife".into(), "abc".into())
            ]
        );
    }
}
