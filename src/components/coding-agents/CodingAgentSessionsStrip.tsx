import { useEffect, useState } from "react";
import { useT } from "../../i18n";
import {
  CODING_AGENT_NAMES,
  type CodingAgentBlockDto,
  codingAgentBlocks,
  onCodingAgentsUpdated,
} from "../../lib/coding-agents";

const TIME_FORMAT: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };

/**
 * The coding-agent lane of the "Today" view: the blocks of one local day
 * with agent, project, time, and summary. Self-contained (loads its own data
 * and refreshes when the scanner reports new blocks or summaries), so the
 * view only mounts it with the day it shows.
 */
export function CodingAgentSessionsStrip({ day }: { day: Date }) {
  const t = useT();
  const [blocks, setBlocks] = useState<CodingAgentBlockDto[]>([]);
  const dayStart = new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();

  useEffect(() => {
    let active = true;
    const from = new Date(dayStart);
    const to = new Date(from.getFullYear(), from.getMonth(), from.getDate() + 1);
    const refresh = () => {
      void codingAgentBlocks(from, to)
        .then((next) => {
          if (active) setBlocks(next);
        })
        .catch(() => {});
    };
    refresh();
    let unlisten: (() => void) | undefined;
    void onCodingAgentsUpdated(refresh).then((cleanup) => {
      if (active) unlisten = cleanup;
      else cleanup();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [dayStart]);

  return (
    <section className="coding-agent-strip" aria-labelledby="coding-agent-strip-heading">
      <h3 id="coding-agent-strip-heading" className="coding-agent-strip-title">
        {t("codingAgents.strip.title")}
      </h3>
      {blocks.length === 0 ? (
        <p className="settings-empty">{t("codingAgents.strip.empty")}</p>
      ) : (
        <ol className="coding-agent-strip-list" aria-label={t("codingAgents.strip.listAria")}>
          {blocks.map((block) => (
            <li key={block.id} className="coding-agent-block" data-state={block.state}>
              <div className="coding-agent-block-meta">
                <span className="coding-agent-block-agent">{CODING_AGENT_NAMES[block.source]}</span>
                <span className="coding-agent-block-project">
                  {block.project ?? t("codingAgents.strip.noProject")}
                </span>
                <time dateTime={block.startedAt}>
                  {t("codingAgents.strip.timeRange", {
                    start: new Date(block.startedAt).toLocaleTimeString([], TIME_FORMAT),
                    end: new Date(block.endedAt).toLocaleTimeString([], TIME_FORMAT),
                  })}
                </time>
                <span>{t("codingAgents.strip.prompts", { count: block.promptCount })}</span>
              </div>
              {block.summary ? (
                <p className="coding-agent-block-summary">{block.summary}</p>
              ) : (
                <>
                  <p className="coding-agent-block-status">
                    {block.state === "live"
                      ? t("codingAgents.strip.live")
                      : t("codingAgents.strip.waiting")}
                  </p>
                  {block.firstPrompt ? (
                    <p className="coding-agent-block-prompt">{block.firstPrompt}</p>
                  ) : null}
                </>
              )}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
