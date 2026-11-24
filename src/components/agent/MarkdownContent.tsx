import type { ReactNode } from "react";

function createInlineMarkdownPattern(): RegExp {
  return /(\*\*([^*\s](?:[^*]*[^*\s])?)\*\*|\*([^*\s](?:[^*]*[^*\s])?)\*|~~([^~]+)~~|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\))/g;
}

/** Renders formatted output from routines. */
export function MarkdownContent({ markdown }: { markdown: string }) {
  return <div className="agent-markdown">{renderMarkdownBlocks(markdown)}</div>;
}

function renderMarkdownBlocks(markdown: string) {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let paragraph: string[] = [];
  let key = 0;

  const flushParagraph = () => {
    const text = paragraph.join("\n").trim();
    paragraph = [];
    if (!text) return;
    blocks.push(<p key={`p-${key++}`}>{renderInlineMarkdown(text, key)}</p>);
  };

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = line.trim();
    if (!trimmed) {
      flushParagraph();
      continue;
    }

    if (trimmed.startsWith("```")) {
      flushParagraph();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].trim().startsWith("```")) {
        code.push(lines[index]);
        index += 1;
      }
      // Skip empty fences — including a stray trailing ``` while streaming —
      // so we don't flash an empty code block (a bare padded gray bar).
      const body = code.join("\n");
      if (body.trim()) {
        blocks.push(
          <pre key={`code-${key++}`}>
            <code>{body}</code>
          </pre>,
        );
      }
      continue;
    }

    // Thematic break (---, ***, ___) → a quiet rule instead of literal dashes.
    if (/^([-*_])\1{2,}$/.test(trimmed)) {
      flushParagraph();
      blocks.push(<hr key={`hr-${key++}`} className="agent-md-rule" />);
      continue;
    }

    // Blockquote: strip the > prefix and re-render the inner lines, so quotes
    // can hold paragraphs, lists, or code like any other block.
    if (trimmed.startsWith(">")) {
      flushParagraph();
      const quoted: string[] = [];
      while (index < lines.length && lines[index].trim().startsWith(">")) {
        quoted.push(lines[index].trim().replace(/^>\s?/, ""));
        index += 1;
      }
      index -= 1;
      blocks.push(
        <blockquote key={`quote-${key++}`}>{renderMarkdownBlocks(quoted.join("\n"))}</blockquote>,
      );
      continue;
    }

    // Pipe table: a |…| row followed by a |---|---| separator.
    const isTableRow = (value: string) =>
      value.startsWith("|") && value.endsWith("|") && value.length > 1;
    if (
      isTableRow(trimmed) &&
      index + 1 < lines.length &&
      /^\|(\s*:?-+:?\s*\|)+$/.test(lines[index + 1].trim())
    ) {
      flushParagraph();
      const splitRow = (value: string) =>
        value
          .slice(1, -1)
          .split("|")
          .map((cell) => cell.trim());
      const header = splitRow(trimmed);
      index += 2;
      const rows: string[][] = [];
      while (index < lines.length && isTableRow(lines[index].trim())) {
        rows.push(splitRow(lines[index].trim()));
        index += 1;
      }
      index -= 1;
      blocks.push(
        <div key={`table-${key++}`} className="agent-md-table">
          <table>
            <thead>
              <tr>
                {header.map((cell, cellIndex) => (
                  <th key={cellIndex}>{renderInlineMarkdown(cell, key)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {row.map((cell, cellIndex) => (
                    <td key={cellIndex}>{renderInlineMarkdown(cell, key)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    const heading = /^(#{1,4})\s+(.+)$/.exec(trimmed);
    if (heading) {
      flushParagraph();
      const level = Math.min(heading[1].length, 3);
      const content = renderInlineMarkdown(heading[2], key);
      blocks.push(
        level === 1 ? <h2 key={`h-${key++}`}>{content}</h2> : <h3 key={`h-${key++}`}>{content}</h3>,
      );
      continue;
    }

    const unordered = /^[-*]\s+(.+)$/.exec(trimmed);
    const ordered = /^\d+\.\s+(.+)$/.exec(trimmed);
    if (unordered || ordered) {
      flushParagraph();
      const orderedList = Boolean(ordered);
      const items: string[] = [];
      while (index < lines.length) {
        const candidate = lines[index].trim();
        const match = orderedList
          ? /^\d+\.\s+(.+)$/.exec(candidate)
          : /^[-*]\s+(.+)$/.exec(candidate);
        if (!match) break;
        items.push(match[1]);
        index += 1;
      }
      index -= 1;
      const listItems = items.map((item, itemIndex) => (
        <li key={`li-${key}-${itemIndex}`}>{renderInlineMarkdown(item, key + itemIndex)}</li>
      ));
      blocks.push(
        orderedList ? (
          <ol key={`list-${key++}`}>{listItems}</ol>
        ) : (
          <ul key={`list-${key++}`}>{listItems}</ul>
        ),
      );
      continue;
    }

    paragraph.push(line);
  }

  flushParagraph();
  return blocks;
}

function renderInlineMarkdown(text: string, keySeed: number): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = createInlineMarkdownPattern();
  let lastIndex = 0;
  let index = 0;
  let match = pattern.exec(text);
  while (match !== null) {
    if (match.index > lastIndex) nodes.push(text.slice(lastIndex, match.index));
    if (match[2]) {
      nodes.push(<strong key={`strong-${keySeed}-${index}`}>{match[2]}</strong>);
    } else if (match[3]) {
      nodes.push(<em key={`em-${keySeed}-${index}`}>{match[3]}</em>);
    } else if (match[4]) {
      nodes.push(<del key={`del-${keySeed}-${index}`}>{match[4]}</del>);
    } else if (match[5]) {
      nodes.push(<code key={`code-${keySeed}-${index}`}>{match[5]}</code>);
    } else if (match[6] && match[7]) {
      nodes.push(
        <a key={`link-${keySeed}-${index}`} href={match[7]} rel="noreferrer" target="_blank">
          {match[6]}
        </a>,
      );
    }
    lastIndex = pattern.lastIndex;
    index += 1;
    match = pattern.exec(text);
  }
  if (lastIndex < text.length) nodes.push(text.slice(lastIndex));
  return nodes;
}
