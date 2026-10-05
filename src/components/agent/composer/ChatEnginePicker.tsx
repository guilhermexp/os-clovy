import { IconCheckmark2Small } from "central-icons/IconCheckmark2Small";
import { IconChevronDownSmall } from "central-icons/IconChevronDownSmall";
import { type RefObject, useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { useT } from "../../../i18n";
import {
  type ChatEngineCatalog,
  type ChatEngineCliStatus,
  type ChatEngineEndpoint,
  chatEngineCatalog,
  classifyChatEngineModel,
} from "../../../lib/chat-engine";
import { rawLocalGenerationModelId } from "../../../lib/local-generation";
import { useComposerModelPopoverPosition } from "./ModelPicker";

const DEFAULT_CLIS: ChatEngineCliStatus[] = [
  {
    id: "claude",
    name: "Claude Code",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:claude",
    clovyTools: "available",
  },
  {
    id: "codex",
    name: "Codex",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:codex",
    clovyTools: "available",
  },
  {
    id: "pi",
    name: "Pi",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:pi",
    clovyTools: "unsupported",
  },
  {
    id: "agy",
    name: "Agy",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:agy",
    clovyTools: "unsupported",
  },
  {
    id: "cursor-agent",
    name: "Cursor Agent",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:cursor-agent",
    clovyTools: "unsupported",
  },
  {
    id: "copilot",
    name: "GitHub Copilot",
    installed: false,
    reason: null,
    modelId: "__clovy_cli_engine__:copilot",
    clovyTools: "available",
  },
];

function fallbackCliName(id: string): string {
  switch (id) {
    case "claude":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "pi":
      return "Pi";
    case "agy":
      return "Agy";
    case "cursor-agent":
      return "Cursor Agent";
    case "copilot":
      return "GitHub Copilot";
    default:
      return id;
  }
}

export function ChatEngineNotice({
  catalog,
  model,
}: {
  catalog?: ChatEngineCatalog | null;
  model?: string;
}) {
  const t = useT();
  const classification = classifyChatEngineModel(model, catalog);
  if (classification.kind !== "cli") return null;

  const cli =
    catalog?.clis.find((item) => item.id === classification.cli) ??
    DEFAULT_CLIS.find((item) => item.id === classification.cli);

  const cliName = cli?.name ?? fallbackCliName(classification.cli);
  const toolsStatus = cli?.clovyTools ?? "unsupported";

  let messageKey:
    | "chat.engine.notice.available"
    | "chat.engine.notice.serverOff"
    | "chat.engine.notice.unsupported";

  switch (toolsStatus) {
    case "available":
      messageKey = "chat.engine.notice.available";
      break;
    case "server_off":
      messageKey = "chat.engine.notice.serverOff";
      break;
    case "unsupported":
      messageKey = "chat.engine.notice.unsupported";
      break;
  }

  return (
    <div className="agent-composer-notice" role="status">
      {t(messageKey, { name: cliName })}
    </div>
  );
}

export type ChatEnginePickerProps = {
  model?: string;
  setModel: (modelId: string) => void;
  catalog?: ChatEngineCatalog | null;
  onRefreshCatalog?: () => Promise<ChatEngineCatalog | null>;
  anchorRef?: RefObject<HTMLElement | null>;
  clovyModels?: ReadonlyArray<{ id: string }> | null;
  readOnly?: boolean;
  showNotice?: boolean;
};

export function ChatEnginePicker({
  model,
  setModel,
  catalog: propCatalog,
  onRefreshCatalog,
  anchorRef,
  clovyModels,
  readOnly = false,
  showNotice = true,
}: ChatEnginePickerProps) {
  const t = useT();
  const menuTitleId = useId();
  const [open, setOpen] = useState(false);
  const [internalCatalog, setInternalCatalog] = useState<ChatEngineCatalog | null>(
    propCatalog ?? null,
  );
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const lastClovyModelRef = useRef<string>("auto");

  const classification = classifyChatEngineModel(
    model,
    propCatalog ?? internalCatalog,
    clovyModels,
  );
  if (classification.kind === "clovy" && model) {
    lastClovyModelRef.current = model;
  }

  const loadCatalog = useCallback(async () => {
    try {
      if (onRefreshCatalog) {
        const refreshed = await onRefreshCatalog();
        if (refreshed) {
          setInternalCatalog(refreshed);
          return;
        }
      }
      const fetched = await chatEngineCatalog();
      setInternalCatalog(fetched);
    } catch {
      // Fallback stays in place if IPC is unavailable.
    }
  }, [onRefreshCatalog]);

  useEffect(() => {
    if (propCatalog) {
      setInternalCatalog(propCatalog);
      return;
    }
    void loadCatalog();
  }, [loadCatalog, propCatalog]);

  useComposerModelPopoverPosition({
    open,
    triggerRef: triggerRef as RefObject<HTMLElement>,
    popoverRef: popoverRef as RefObject<HTMLElement>,
    anchorRef: (anchorRef ?? { current: null }) as RefObject<HTMLElement>,
  });

  useEffect(() => {
    if (!open) return;
    const handleMousedown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (popoverRef.current?.contains(target) || triggerRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    };
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      }
    };
    window.addEventListener("mousedown", handleMousedown);
    window.addEventListener("keydown", handleKeydown);
    return () => {
      window.removeEventListener("mousedown", handleMousedown);
      window.removeEventListener("keydown", handleKeydown);
    };
  }, [open]);

  const activeCatalog = propCatalog ?? internalCatalog;
  const clis = activeCatalog?.clis.length ? activeCatalog.clis : DEFAULT_CLIS;
  const endpoints = activeCatalog?.endpoints ?? [];

  let activeEngineLabel = t("chat.engine.clovy");
  if (classification.kind === "cli") {
    const activeCli = clis.find((item) => item.id === classification.cli);
    activeEngineLabel = activeCli?.name ?? fallbackCliName(classification.cli);
  } else if (classification.kind === "endpoint") {
    const rawLocal = rawLocalGenerationModelId(model ?? "");
    const activeEndpoint = endpoints.find(
      (ep) =>
        ep.optionId === model ||
        ep.modelId === classification.modelId ||
        (rawLocal !== null && ep.modelId === rawLocal),
    );
    activeEngineLabel = activeEndpoint?.name ?? classification.modelId;
  }

  const handleSelectClovy = () => {
    setOpen(false);
    if (classification.kind !== "clovy") {
      setModel(lastClovyModelRef.current || "auto");
    }
  };

  const handleSelectEndpoint = (ep: ChatEngineEndpoint) => {
    setOpen(false);
    setModel(ep.optionId);
  };

  const handleSelectCli = (cli: ChatEngineCliStatus) => {
    if (!cli.installed) return;
    setOpen(false);
    setModel(cli.modelId);
  };

  const popoverContent = open ? (
    <div
      ref={popoverRef}
      className="agent-composer-model-popover agent-engine-popover"
      role="menu"
      aria-labelledby={menuTitleId}
    >
      <p id={menuTitleId} className="agent-composer-model-title">
        {t("chat.engine.menuTitle")}
      </p>

      <p className="agent-composer-model-title">{t("chat.engine.sectionClovy")}</p>
      <div className="agent-composer-model-menu">
        <button
          type="button"
          className="agent-composer-model-row"
          role="menuitemradio"
          aria-checked={classification.kind === "clovy"}
          onClick={handleSelectClovy}
        >
          <span className="agent-composer-model-choice-copy">
            <span className="agent-composer-model-row-name">{t("chat.engine.clovy")}</span>
          </span>
          {classification.kind === "clovy" ? (
            <IconCheckmark2Small size={14} aria-hidden className="agent-composer-model-row-check" />
          ) : null}
        </button>
      </div>

      {endpoints.length > 0 ? (
        <>
          <p className="agent-composer-model-title">{t("chat.engine.sectionEndpoints")}</p>
          <div className="agent-composer-model-menu">
            {endpoints.map((ep) => {
              const rawLocal = rawLocalGenerationModelId(model ?? "");
              const isSelected =
                classification.kind === "endpoint" &&
                (model === ep.optionId ||
                  classification.modelId === ep.modelId ||
                  rawLocal === ep.modelId);
              return (
                <button
                  key={ep.id}
                  type="button"
                  className="agent-composer-model-row"
                  role="menuitemradio"
                  aria-checked={isSelected}
                  onClick={() => handleSelectEndpoint(ep)}
                >
                  <span className="agent-composer-model-choice-copy">
                    <span className="agent-composer-model-row-name">{ep.name}</span>
                  </span>
                  {isSelected ? (
                    <IconCheckmark2Small
                      size={14}
                      aria-hidden
                      className="agent-composer-model-row-check"
                    />
                  ) : null}
                </button>
              );
            })}
          </div>
        </>
      ) : null}

      <p className="agent-composer-model-title">{t("chat.engine.sectionClis")}</p>
      <div className="agent-composer-model-menu">
        {clis.map((cli) => {
          const isSelected = classification.kind === "cli" && classification.cli === cli.id;
          return (
            <button
              key={cli.id}
              type="button"
              className="agent-composer-model-row"
              role="menuitemradio"
              aria-checked={isSelected}
              disabled={!cli.installed}
              aria-disabled={!cli.installed}
              onClick={() => handleSelectCli(cli)}
            >
              <span className="agent-composer-model-choice-copy">
                <span className="agent-composer-model-row-name">{cli.name}</span>
                {!cli.installed ? (
                  <span className="agent-composer-model-choice-desc">
                    {cli.reason ?? t("chat.engine.notInstalled")}
                  </span>
                ) : null}
              </span>
              {isSelected ? (
                <IconCheckmark2Small
                  size={14}
                  aria-hidden
                  className="agent-composer-model-row-check"
                />
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  ) : null;

  return (
    <>
      {showNotice ? <ChatEngineNotice catalog={activeCatalog} model={model} /> : null}
      <div className="agent-composer-model" data-engine-picker="true" data-open={open || undefined}>
        <button
          ref={triggerRef}
          type="button"
          className="agent-composer-model-trigger"
          aria-label={t("chat.engine.pickerLabel", { name: activeEngineLabel })}
          aria-haspopup="menu"
          aria-expanded={open}
          disabled={readOnly}
          onClick={() => {
            if (readOnly) return;
            if (!open) {
              void loadCatalog();
              setOpen(true);
            } else {
              setOpen(false);
            }
          }}
        >
          <span className="agent-composer-model-trigger-name">{activeEngineLabel}</span>
          <IconChevronDownSmall size={12} aria-hidden />
        </button>
      </div>
      {open && anchorRef?.current
        ? createPortal(popoverContent, anchorRef.current)
        : popoverContent}
    </>
  );
}
