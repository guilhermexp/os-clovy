import { IconShuffle } from "central-icons/IconShuffle";
import { useEffect, useRef, useState } from "react";
import { t as translate, useT } from "../../i18n";
import { toast } from "../ui/Toaster";
import { errorCode } from "../../lib/errors";
import { osAccountsCancelLogin, osAccountsLogin, osAccountsLogout } from "../../lib/tauri";
import type { AccountStatus } from "../../lib/tauri";
import { AccountAvatar, useAccountAvatar } from "./AccountAvatar";

type Props = {
  account: AccountStatus;
  loading: boolean;
  onAccountChanged: (next: AccountStatus) => void;
  onRefresh: () => Promise<AccountStatus | undefined>;
};

export function AccountSettings({ account, loading, onAccountChanged, onRefresh }: Props) {
  const t = useT();
  return (
    <div className="settings-page">
      <header className="settings-header">
        <h1 className="settings-title">{t("account.title")}</h1>
        <p className="settings-description">
          {account.localDev
            ? t("account.settings.localDescription")
            : t("account.settings.description")}
        </p>
      </header>

      <AccountSettingsSection
        account={account}
        loading={loading}
        onAccountChanged={onAccountChanged}
        onRefresh={onRefresh}
      />
    </div>
  );
}

export function AccountSettingsSection({ account, loading, onAccountChanged }: Props) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [avatarBusy, setAvatarBusy] = useState(false);
  const [accountStatus, setAccountStatus] = useState<string>();
  const accountRef = useRef(account);
  const mountedRef = useRef(true);
  accountRef.current = account;
  const { localOnly: avatarLocalOnly, refresh: refreshAvatar } = useAccountAvatar(account);
  const canManageAvatar = account.localDev || Boolean(account.signedIn && account.user?.id?.trim());

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  async function handleRefreshAvatar() {
    setAvatarBusy(true);
    try {
      const user = await refreshAvatar();
      if (user) {
        // Propagate to App-level account state before the mounted guard: the
        // callback is safe post-unmount and skipping it would strand the
        // local-only copy until the next focus refresh.
        const currentAccount = accountRef.current;
        if (currentAccount.signedIn && currentAccount.user?.id === user.id) {
          onAccountChanged({ ...currentAccount, user });
        }
      }
      if (!mountedRef.current) return;
      toast.success(
        accountRef.current.localDev
          ? t("account.settings.avatarUpdatedDevice")
          : t("account.settings.avatarUpdatedEverywhere"),
      );
    } catch (error) {
      if (mountedRef.current && (accountRef.current.signedIn || accountRef.current.localDev)) {
        const code = errorCode(error);
        if (code === "account_permission_required") {
          toast.warning(t("account.settings.avatarPermission"));
        } else if (code === "avatar_sync_unavailable") {
          toast.warning(t("account.settings.avatarSyncUnavailable"));
        } else {
          toast.error(messageFromError(error));
        }
      }
    } finally {
      if (mountedRef.current) setAvatarBusy(false);
    }
  }

  async function handleSignIn() {
    setBusy(true);
    setAccountStatus(t("account.settings.openingBrowser"));
    try {
      const next = await osAccountsLogin();
      onAccountChanged(next);
      setAccountStatus(
        next.signedIn ? t("account.settings.signedInAs", { name: displayName(next) }) : undefined,
      );
    } catch (error) {
      setAccountStatus(messageFromError(error));
    } finally {
      setBusy(false);
    }
  }

  async function handleCancel() {
    // Aborts the waiting os_accounts_login; that promise then rejects with
    // "login_canceled", and handleSignIn's catch/finally resets status + busy.
    try {
      await osAccountsCancelLogin();
    } catch (error) {
      setAccountStatus(messageFromError(error));
    }
  }

  async function handleSignOut() {
    setBusy(true);
    try {
      await osAccountsLogout({ clearBrowserSession: true });
      onAccountChanged({ signedIn: false, configured: account.configured });
      setAccountStatus(t("account.settings.signedOut"));
    } catch (error) {
      setAccountStatus(messageFromError(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="settings-group" aria-labelledby="account-heading">
      <h2 id="account-heading" className="settings-group-heading">
        {t("account.title")}
      </h2>
      {accountStatus ? <p className="settings-status">{accountStatus}</p> : null}
      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 className="settings-row-title">
                {account.localDev
                  ? t("account.settings.localMode")
                  : loading
                    ? t("account.settings.checkingSignIn")
                    : account.signedIn
                      ? displayName(account)
                      : t("account.settings.notSignedIn")}
              </h3>
              <p className="settings-row-description">
                {account.localDev
                  ? t("account.settings.localRequests")
                  : account.signedIn
                    ? (account.user?.email ?? `@${account.user?.handle ?? "account"}`)
                    : account.configured
                      ? t("account.settings.managedBy")
                      : t("account.signInNotConfigured")}
              </p>
            </div>
            <div className="settings-row-control">
              {account.localDev ? null : account.signedIn ? (
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={busy || avatarBusy}
                  onClick={() => void handleSignOut()}
                >
                  {t("account.settings.signOut")}
                </button>
              ) : busy ? (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => void handleCancel()}
                >
                  {t("common.cancel")}
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={loading || !account.configured}
                  onClick={() => void handleSignIn()}
                >
                  {t("account.settings.signIn")}
                </button>
              )}
            </div>
          </div>
          {canManageAvatar ? (
            <div className="settings-row">
              <div className="settings-row-info">
                <h3 className="settings-row-title">{t("account.settings.avatar")}</h3>
                <p className="settings-row-description">
                  {account.localDev
                    ? t("account.settings.avatarLocal")
                    : avatarLocalOnly
                      ? t("account.settings.avatarLocalOnly")
                      : t("account.settings.avatarSynced")}
                </p>
              </div>
              <div className="settings-row-control">
                <AccountAvatar account={account} className="account-avatar-preview" />
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={avatarBusy || busy}
                  onClick={() => void handleRefreshAvatar()}
                >
                  <IconShuffle size={14} />
                  {t("account.settings.refresh")}
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function displayName(account: AccountStatus) {
  return (
    account.user?.displayName ??
    (account.user?.handle ? `@${account.user.handle}` : translate("account.settings.signedIn"))
  );
}

function messageFromError(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}
