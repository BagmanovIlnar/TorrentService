// Kerberos SSO (SPNEGO): Authorization: Negotiate <base64> -> имя пользователя.
// Собирается только с cargo feature "sso" (нужны системные libgssapi_krb5 + заголовки);
// без фичи модуль существует, но accept() всегда None — работает обычный логин.

#[cfg(feature = "sso")]
pub mod enabled {
    use libgssapi::context::{SecurityContext, ServerCtx};
    use libgssapi::credential::{Cred, CredUsage};
    use libgssapi::name::Name;

    /// Проверить Negotiate-токен. -> Some("user") при успехе.
    /// spn: "HTTP/host@REALM" (None = default principal из keytab).
    /// Кука выдаётся в middleware — браузер не ходит по SPNEGO на каждый запрос.
    pub fn accept(token_b64: &str, spn: Option<&str>) -> Option<String> {
        use base64::Engine;
        use libgssapi::oid;
        let token = base64::engine::general_purpose::STANDARD.decode(token_b64).ok()?;
        let cred = match spn {
            Some(s) => {
                let name = Name::new(s.as_bytes(), Some(&oid::GSS_NT_HOSTBASED_SERVICE)).ok()?;
                Cred::acquire(Some(&name), None, CredUsage::Accept, None).ok()?
            }
            None => Cred::acquire(None, None, CredUsage::Accept, None).ok()?, // default keytab (KRB5_KTNAME)
        };
        let mut ctx = ServerCtx::new(Some(cred));
        ctx.step(&token).ok()?;
        let name = ctx.source_name().ok()?;
        let display = String::from_utf8(name.display_name().ok()?.to_vec()).ok()?;
        // "user@REALM" -> "user"
        let user = display.split('@').next()?.to_string();
        if !crate::auth::valid_username(&user) {
            return None;
        }
        Some(user)
    }

    /// Сгенерировать krb5.conf (если нет) и выставить KRB5_CONFIG.
    /// KDC берём из хоста AD_URL — в docker-сети это имя контейнера DC.
    pub fn setup_krb5(ad_domain: &str, ad_url: &str, persist_dir: &std::path::Path) {
        let kdc = ad_url
            .trim_start_matches("ldap://")
            .trim_start_matches("ldaps://")
            .split(':')
            .next()
            .unwrap_or("service_samba_addc")
            .to_string();
        let conf = format!(
            "[libdefaults]\n\
             default_realm = {realm}\n\
             dns_lookup_realm = false\n\
             dns_lookup_kdc = false\n\
             udp_preference_limit = 1\n\
             [realms]\n\
             {realm} = {{ kdc = {kdc}, admin_server = {kdc} }}\n\
             [domain_realm]\n\
             .{realm_lower} = {realm}\n\
             {realm_lower} = {realm}\n",
            realm = ad_domain,
            realm_lower = ad_domain.to_lowercase(),
            kdc = kdc,
        );
        let path = persist_dir.join("krb5.conf");
        if !path.exists() {
            std::fs::write(&path, conf).ok();
        }
        std::env::set_var("KRB5_CONFIG", &path);
        if std::env::var("KRB5_KTNAME").is_err() {
            let kt = persist_dir.join("torrent.keytab");
            std::env::set_var("KRB5_KTNAME", format!("FILE:{}", kt.display()));
        }
        tracing::info!("SSO: krb5.conf = {}, keytab = {}", path.display(), std::env::var("KRB5_KTNAME").unwrap_or_default());
    }
}

#[cfg(not(feature = "sso"))]
pub mod enabled {
    pub fn accept(_token_b64: &str, _spn: Option<&str>) -> Option<String> {
        tracing::debug!("SSO: собрано без feature \"sso\", Negotiate недоступен");
        None
    }

    pub fn setup_krb5(_d: &str, _u: &str, _p: &std::path::Path) {}
}

pub use enabled::{accept, setup_krb5};
