//! The app's only identity: a fixed local user and the bearer token the local
//! `clovy-api` accepts. There is no sign-in, so the app is always connected.

const BEARER_TOKEN_ENV: &str = "OS_CLOVY_LOCAL_DEV_BEARER_TOKEN";
const LEGACY_BEARER_TOKEN_ENV: &str = "OS_JUNE_LOCAL_DEV_BEARER_TOKEN";
const USER_ID_ENV: &str = "OS_CLOVY_LOCAL_DEV_USER_ID";
const LEGACY_USER_ID_ENV: &str = "OS_JUNE_LOCAL_DEV_USER_ID";
const DEFAULT_BEARER_TOKEN: &str = "local-dev-token";
const DEFAULT_USER_ID: &str = "usr_local_dev";

/// Bearer token sent to `clovy-api`.
pub(crate) fn access_token() -> String {
    crate::env_compat::load_local_env();
    let token = crate::env_compat::trimmed(BEARER_TOKEN_ENV, LEGACY_BEARER_TOKEN_ENV);
    if token.is_empty() {
        DEFAULT_BEARER_TOKEN.to_string()
    } else {
        token
    }
}

/// Id of the local user, as stored on notes and sessions.
pub(crate) fn user_id() -> String {
    crate::env_compat::load_local_env();
    let user_id = crate::env_compat::trimmed(USER_ID_ENV, LEGACY_USER_ID_ENV);
    if user_id.starts_with("usr_") {
        user_id
    } else {
        DEFAULT_USER_ID.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const VARS: [&str; 4] = [
        "OS_CLOVY_LOCAL_DEV_BEARER_TOKEN",
        "OS_JUNE_LOCAL_DEV_BEARER_TOKEN",
        "OS_CLOVY_LOCAL_DEV_USER_ID",
        "OS_JUNE_LOCAL_DEV_USER_ID",
    ];

    fn clean_env() -> MutexGuard<'static, ()> {
        let guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        for var in VARS {
            std::env::remove_var(var);
        }
        guard
    }

    #[test]
    fn uses_the_default_local_token_without_configuration() {
        let _env = clean_env();

        assert_eq!(access_token(), "local-dev-token");
    }

    #[test]
    fn uses_the_configured_local_token() {
        let _env = clean_env();
        std::env::set_var("OS_CLOVY_LOCAL_DEV_BEARER_TOKEN", "  owner-token  ");

        assert_eq!(access_token(), "owner-token");
        std::env::remove_var("OS_CLOVY_LOCAL_DEV_BEARER_TOKEN");
    }

    #[test]
    fn uses_the_default_local_user_without_configuration() {
        let _env = clean_env();

        assert_eq!(user_id(), "usr_local_dev");
    }

    #[test]
    fn uses_a_configured_local_user_only_when_it_is_a_user_id() {
        let _env = clean_env();
        std::env::set_var("OS_CLOVY_LOCAL_DEV_USER_ID", "usr_owner");
        assert_eq!(user_id(), "usr_owner");

        std::env::set_var("OS_CLOVY_LOCAL_DEV_USER_ID", "owner");
        assert_eq!(user_id(), "usr_local_dev");
        std::env::remove_var("OS_CLOVY_LOCAL_DEV_USER_ID");
    }
}
