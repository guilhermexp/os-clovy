//! The app's only identity: the bearer token the local `clovy-api` accepts.
//! There is no sign-in, so the app is always connected.

const BEARER_TOKEN_ENV: &str = "OS_CLOVY_LOCAL_DEV_BEARER_TOKEN";
const DEFAULT_BEARER_TOKEN: &str = "local-dev-token";

/// Bearer token sent to `clovy-api`.
pub(crate) fn access_token() -> String {
    crate::local_env::load_local_env();
    let token = crate::local_env::trimmed(BEARER_TOKEN_ENV);
    if token.is_empty() {
        DEFAULT_BEARER_TOKEN.to_string()
    } else {
        token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const VARS: [&str; 1] = ["OS_CLOVY_LOCAL_DEV_BEARER_TOKEN"];

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
}
