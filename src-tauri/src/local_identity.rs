//! The app's only identity: a fixed local user and the bearer token the local
//! `clovy-api` accepts. There is no sign-in, so the app is always connected.

/// Bearer token sent to `clovy-api`.
pub(crate) fn access_token() -> String {
    String::new()
}

/// Id of the local user, as stored on notes and sessions.
pub(crate) fn user_id() -> String {
    String::new()
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
