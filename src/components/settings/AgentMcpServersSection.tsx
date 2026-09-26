import { IconArrowRotateClockwise } from "central-icons/IconArrowRotateClockwise";
import { IconPlusMedium } from "central-icons/IconPlusMedium";
import { IconSettingsGear4 } from "central-icons/IconSettingsGear4";
import { IconTrashCan } from "central-icons/IconTrashCan";
import { useCallback, useEffect, useState } from "react";
import {
  createAgentMcpServer,
  connectAgentMcpOauth,
  DEFAULT_AGENT_MCP_SAFETY,
  deleteAgentMcpServer,
  listAgentMcpServers,
  testAgentMcpServer,
  updateAgentMcpServer,
  type AgentMcpServerDto,
  type AgentMcpTransport,
} from "../../lib/agent-mcp";
import { type TFunction, useT } from "../../i18n";
import { messageFromError } from "../../lib/errors";
import { Dialog } from "../ui/Dialog";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { InlineNotice } from "../ui/InlineNotice";
import { Switch } from "../ui/Switch";

type Draft = {
  name: string;
  transport: AgentMcpTransport;
  command: string;
  args: string;
  url: string;
  env: string;
  headers: string;
  includeTools: string;
  excludeTools: string;
  approvalTools: string;
  requiresApproval: boolean;
  allowSandboxed: boolean;
  oauth: boolean;
};

const EMPTY_DRAFT: Draft = {
  name: "",
  transport: "stdio",
  command: "",
  args: "",
  url: "",
  env: "",
  headers: "",
  includeTools: "",
  excludeTools: "",
  approvalTools: "",
  requiresApproval: true,
  allowSandboxed: true,
  oauth: false,
};

function parseSecretMap(raw: string, label: string, t: TFunction): Record<string, string> {
  if (!raw.trim()) return {};
  const value: unknown = JSON.parse(raw);
  if (
    !value ||
    Array.isArray(value) ||
    typeof value !== "object" ||
    Object.values(value).some((entry) => typeof entry !== "string")
  ) {
    throw new Error(t("settingsPanels.mcp.secretMapInvalid", { label }));
  }
  return value as Record<string, string>;
}

export function AgentMcpServersSection() {
  const t = useT();
  const [servers, setServers] = useState<AgentMcpServerDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [busyId, setBusyId] = useState<string>();
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [addOpen, setAddOpen] = useState(false);
  const [editing, setEditing] = useState<AgentMcpServerDto>();
  const [toDelete, setToDelete] = useState<AgentMcpServerDto>();
  const [saveError, setSaveError] = useState<string>();
  const [testResults, setTestResults] = useState<Record<string, string>>({});

  const load = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      setServers(await listAgentMcpServers());
    } catch (loadError) {
      setError(messageFromError(loadError));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function toggle(server: AgentMcpServerDto, enabled: boolean) {
    setBusyId(server.id);
    setError(undefined);
    try {
      const updated = await updateAgentMcpServer({
        ...server,
        enabled,
      });
      setServers((current) => current.map((item) => (item.id === server.id ? updated : item)));
    } catch (updateError) {
      setError(messageFromError(updateError));
    } finally {
      setBusyId(undefined);
    }
  }

  async function remove(server: AgentMcpServerDto) {
    setBusyId(server.id);
    setError(undefined);
    try {
      await deleteAgentMcpServer(server.id);
      setServers((current) => current.filter((item) => item.id !== server.id));
    } catch (deleteError) {
      setError(messageFromError(deleteError));
    } finally {
      setBusyId(undefined);
    }
  }

  async function test(server: AgentMcpServerDto) {
    setBusyId(server.id);
    setError(undefined);
    try {
      const tools = await testAgentMcpServer(server.id);
      setTestResults((current) => ({
        ...current,
        [server.id]: t("settingsPanels.mcp.toolsAvailable", { count: tools.length }),
      }));
    } catch (testError) {
      setTestResults((current) => ({
        ...current,
        [server.id]: messageFromError(testError),
      }));
    } finally {
      setBusyId(undefined);
    }
  }

  async function connectOauth(server: AgentMcpServerDto) {
    setBusyId(server.id);
    setError(undefined);
    setTestResults((current) => ({
      ...current,
      [server.id]: t("settingsPanels.mcp.waitingForBrowser"),
    }));
    try {
      const connected = await connectAgentMcpOauth(server.id);
      setServers((current) => current.map((item) => (item.id === connected.id ? connected : item)));
      setTestResults((current) => ({
        ...current,
        [server.id]: t("settingsPanels.mcp.oauthConnected"),
      }));
    } catch (connectError) {
      setTestResults((current) => ({
        ...current,
        [server.id]: messageFromError(connectError),
      }));
    } finally {
      setBusyId(undefined);
    }
  }

  function openCreate() {
    setEditing(undefined);
    setDraft(EMPTY_DRAFT);
    setSaveError(undefined);
    setAddOpen(true);
  }

  function openEdit(server: AgentMcpServerDto) {
    setEditing(server);
    setDraft({
      name: server.name,
      transport: server.transport,
      command: server.command ?? "",
      args: server.args.join("\n"),
      url: server.url ?? "",
      env: "",
      headers: "",
      includeTools: server.toolVisibility.include.join("\n"),
      excludeTools: server.toolVisibility.exclude.join("\n"),
      approvalTools: server.safety.approvalTools.join("\n"),
      requiresApproval: server.safety.requiresApproval,
      allowSandboxed: server.safety.allowSandboxed,
      oauth: server.metadata.auth === "oauth" || server.metadata.legacyAuth === "oauth",
    });
    setSaveError(undefined);
    setAddOpen(true);
  }

  async function save() {
    setSaveError(undefined);
    try {
      const secretBundle = {
        env: parseSecretMap(draft.env, t("settingsPanels.mcp.environment"), t),
        headers: parseSecretMap(draft.headers, t("settingsPanels.mcp.headers"), t),
      };
      const metadata = { ...(editing?.metadata ?? {}) };
      if (draft.oauth) {
        metadata.auth = "oauth";
      } else {
        delete metadata.auth;
        delete metadata.oauthConnected;
        delete metadata.legacyAuth;
        delete metadata.needsReview;
        delete metadata.migrationWarning;
      }
      const input = {
        id: editing?.id,
        name: draft.name.trim(),
        enabled: draft.oauth ? (editing?.enabled ?? false) : (editing?.enabled ?? true),
        transport: draft.transport,
        command: draft.transport === "stdio" ? draft.command.trim() : undefined,
        args:
          draft.transport === "stdio"
            ? draft.args
                .split("\n")
                .map((value) => value.trim())
                .filter(Boolean)
            : [],
        url: draft.transport === "streamable_http" ? draft.url.trim() : undefined,
        metadata,
        toolVisibility: {
          include: splitLines(draft.includeTools),
          exclude: splitLines(draft.excludeTools),
        },
        safety: {
          ...(editing?.safety ?? DEFAULT_AGENT_MCP_SAFETY),
          requiresApproval: draft.requiresApproval,
          allowSandboxed: draft.allowSandboxed,
          approvalTools: splitLines(draft.approvalTools),
        },
        ...(!editing ||
        Object.keys(secretBundle.env).length ||
        Object.keys(secretBundle.headers).length
          ? { secrets: secretBundle }
          : {}),
      };
      const saved = editing
        ? await updateAgentMcpServer({ ...input, id: editing.id })
        : await createAgentMcpServer(input);
      setServers((current) =>
        (editing
          ? current.map((server) => (server.id === editing.id ? saved : server))
          : [...current, saved]
        ).sort((a, b) => a.name.localeCompare(b.name)),
      );
      setDraft(EMPTY_DRAFT);
      setEditing(undefined);
      setAddOpen(false);
    } catch (createError) {
      setSaveError(messageFromError(createError));
    }
  }

  function splitLines(value: string) {
    return value
      .split("\n")
      .map((item) => item.trim())
      .filter(Boolean);
  }

  return (
    <section className="settings-group" aria-labelledby="mcp-servers-heading">
      <div className="settings-group-header">
        <h3 id="mcp-servers-heading" className="settings-group-heading">
          {t("settingsPanels.mcp.heading")}
        </h3>
        <button type="button" className="btn btn-secondary" onClick={openCreate}>
          <IconPlusMedium size={14} />
          {t("settingsPanels.mcp.addServer")}
        </button>
      </div>
      <p className="settings-group-description">{t("settingsPanels.mcp.description")}</p>

      {error ? (
        <InlineNotice
          tone="warning"
          body={error}
          actions={
            <button type="button" className="btn btn-secondary" onClick={() => void load()}>
              {t("common.tryAgain")}
            </button>
          }
        />
      ) : null}

      <div className="settings-card">
        <div className="settings-rows">
          {loading ? (
            <div className="settings-row">
              <p className="settings-row-description">{t("settingsPanels.mcp.loading")}</p>
            </div>
          ) : servers.length === 0 ? (
            <div className="settings-row">
              <div className="settings-row-info">
                <h4 className="settings-row-title">{t("settingsPanels.mcp.emptyTitle")}</h4>
                <p className="settings-row-description">{t("settingsPanels.mcp.emptyBody")}</p>
              </div>
            </div>
          ) : (
            servers.map((server) => (
              <div className="settings-row" key={server.id}>
                <div className="settings-row-info">
                  <h4 className="settings-row-title">{server.name}</h4>
                  <p className="settings-row-description">
                    {server.transport === "stdio" ? server.command : server.url}
                    {server.metadata.needsReview === true
                      ? t("settingsPanels.mcp.needsReviewSuffix")
                      : ""}
                    {testResults[server.id] ? ` · ${testResults[server.id]}` : ""}
                  </p>
                </div>
                <div className="settings-row-control">
                  {server.metadata.legacyAuth === "oauth" || server.metadata.auth === "oauth" ? (
                    <button
                      type="button"
                      className="btn btn-secondary"
                      disabled={busyId === server.id}
                      onClick={() => void connectOauth(server)}
                    >
                      {server.metadata.oauthConnected === true
                        ? t("settingsPanels.mcp.reconnect")
                        : t("settingsPanels.mcp.connect")}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("settingsPanels.mcp.configureName", { name: server.name })}
                    disabled={busyId === server.id}
                    onClick={() => openEdit(server)}
                  >
                    <IconSettingsGear4 size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("settingsPanels.mcp.testName", { name: server.name })}
                    disabled={busyId === server.id}
                    onClick={() => void test(server)}
                  >
                    <IconArrowRotateClockwise size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("settingsPanels.mcp.deleteName", { name: server.name })}
                    disabled={busyId === server.id}
                    onClick={() => setToDelete(server)}
                  >
                    <IconTrashCan size={14} />
                  </button>
                  <Switch
                    checked={server.enabled}
                    disabled={busyId === server.id}
                    aria-label={t("settingsPanels.mcp.enabledName", { name: server.name })}
                    onCheckedChange={(enabled) => void toggle(server, enabled)}
                  />
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      <Dialog
        open={addOpen}
        onClose={() => {
          setAddOpen(false);
          setEditing(undefined);
        }}
        title={
          editing
            ? t("settingsPanels.mcp.configureName", { name: editing.name })
            : t("settingsPanels.mcp.addTitle")
        }
        description={
          editing ? t("settingsPanels.mcp.editDescription") : t("settingsPanels.mcp.addDescription")
        }
        footer={
          <>
            <button
              type="button"
              className="primary-action"
              onClick={() => {
                setAddOpen(false);
                setEditing(undefined);
              }}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              className="primary-action primary-solid"
              disabled={!draft.name.trim()}
              onClick={() => void save()}
            >
              {editing ? t("settingsPanels.mcp.saveChanges") : t("settingsPanels.mcp.addServer")}
            </button>
          </>
        }
      >
        <div className="dialog-body">
          {saveError ? <InlineNotice tone="warning" body={saveError} /> : null}
          {editing?.metadata.legacyAuth === "oauth" ? (
            <InlineNotice tone="warning" body={t("settingsPanels.mcp.legacyOauth")} />
          ) : null}
          <label className="dialog-field">
            {t("settingsPanels.mcp.name")}
            <input
              className="dialog-input"
              value={draft.name}
              onChange={(event) =>
                setDraft((current) => ({ ...current, name: event.target.value }))
              }
            />
          </label>
          <label className="dialog-field">
            {t("settingsPanels.mcp.transport")}
            <select
              className="dialog-input"
              value={draft.transport}
              onChange={(event) =>
                setDraft((current) => ({
                  ...current,
                  transport: event.target.value as AgentMcpTransport,
                }))
              }
            >
              <option value="stdio">{t("settingsPanels.mcp.transportStdio")}</option>
              <option value="streamable_http">Streamable HTTP</option>
            </select>
          </label>
          {draft.transport === "stdio" ? (
            <>
              <label className="dialog-field">
                {t("settingsPanels.mcp.command")}
                <input
                  className="dialog-input"
                  value={draft.command}
                  onChange={(event) =>
                    setDraft((current) => ({ ...current, command: event.target.value }))
                  }
                />
              </label>
              <label className="dialog-field">
                {t("settingsPanels.mcp.arguments")}
                <textarea
                  className="dialog-textarea"
                  value={draft.args}
                  onChange={(event) =>
                    setDraft((current) => ({ ...current, args: event.target.value }))
                  }
                />
              </label>
            </>
          ) : (
            <>
              <label className="dialog-field">
                URL
                <input
                  className="dialog-input"
                  value={draft.url}
                  onChange={(event) =>
                    setDraft((current) => ({ ...current, url: event.target.value }))
                  }
                />
              </label>
              <label className="dialog-checkbox">
                <input
                  type="checkbox"
                  checked={draft.oauth}
                  onChange={(event) =>
                    setDraft((current) => ({ ...current, oauth: event.target.checked }))
                  }
                />
                {t("settingsPanels.mcp.authenticateOauth")}
              </label>
            </>
          )}
          <label className="dialog-field">
            {t("settingsPanels.mcp.envVars")}
            <textarea
              className="dialog-textarea"
              placeholder={'{"TOKEN":"..."}'}
              value={draft.env}
              onChange={(event) => setDraft((current) => ({ ...current, env: event.target.value }))}
            />
          </label>
          <label className="dialog-field">
            {t("settingsPanels.mcp.httpHeaders")}
            <textarea
              className="dialog-textarea"
              placeholder={'{"Authorization":"Bearer ..."}'}
              value={draft.headers}
              onChange={(event) =>
                setDraft((current) => ({ ...current, headers: event.target.value }))
              }
            />
          </label>
          <label className="dialog-field">
            {t("settingsPanels.mcp.allowedTools")}
            <textarea
              className="dialog-textarea"
              placeholder={t("settingsPanels.mcp.allowedToolsPlaceholder")}
              value={draft.includeTools}
              onChange={(event) =>
                setDraft((current) => ({ ...current, includeTools: event.target.value }))
              }
            />
          </label>
          <label className="dialog-field">
            {t("settingsPanels.mcp.blockedTools")}
            <textarea
              className="dialog-textarea"
              value={draft.excludeTools}
              onChange={(event) =>
                setDraft((current) => ({ ...current, excludeTools: event.target.value }))
              }
            />
          </label>
          <label className="dialog-field">
            {t("settingsPanels.mcp.approvalTools")}
            <textarea
              className="dialog-textarea"
              value={draft.approvalTools}
              disabled={draft.requiresApproval}
              onChange={(event) =>
                setDraft((current) => ({ ...current, approvalTools: event.target.value }))
              }
            />
          </label>
          <div className="settings-card">
            <div className="settings-rows">
              <div className="settings-row">
                <span className="settings-row-title">
                  {t("settingsPanels.mcp.requireApprovalAll")}
                </span>
                <Switch
                  checked={draft.requiresApproval}
                  aria-label={t("settingsPanels.mcp.requireApprovalAll")}
                  onCheckedChange={(value) =>
                    setDraft((current) => ({ ...current, requiresApproval: value }))
                  }
                />
              </div>
              <div className="settings-row">
                <span className="settings-row-title">
                  {draft.transport === "stdio"
                    ? t("settingsPanels.mcp.allowSandboxedMacos")
                    : t("settingsPanels.mcp.allowSandboxed")}
                </span>
                <Switch
                  checked={draft.allowSandboxed}
                  aria-label={t("settingsPanels.mcp.allowSandboxed")}
                  onCheckedChange={(value) =>
                    setDraft((current) => ({ ...current, allowSandboxed: value }))
                  }
                />
              </div>
            </div>
          </div>
        </div>
      </Dialog>
      <ConfirmDialog
        open={Boolean(toDelete)}
        onClose={() => setToDelete(undefined)}
        onConfirm={() => (toDelete ? remove(toDelete) : undefined)}
        title={
          toDelete
            ? t("settingsPanels.mcp.deleteTitleName", { name: toDelete.name })
            : t("settingsPanels.mcp.deleteTitle")
        }
        description={t("settingsPanels.mcp.deleteDescription")}
        confirmLabel={t("settingsPanels.mcp.deleteServer")}
        confirmBusyLabel={t("settingsPanels.mcp.deleting")}
        destructive
      />
    </section>
  );
}
