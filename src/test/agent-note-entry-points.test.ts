import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { noteReferenceToken } from "../lib/note-reference";
import { NoteHeaderActions } from "../components/note-editor/NoteHeaderActions";

function installClipboard(writeText = vi.fn().mockResolvedValue(undefined)) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  return writeText;
}

describe("note header actions", () => {
  it("copies a portable reference for Clovy", async () => {
    const user = userEvent.setup();
    const writeText = installClipboard();
    render(createElement(NoteHeaderActions, { noteId: "note-1", noteTitle: "Launch plan" }));

    await user.click(screen.getByRole("button", { name: "Note actions" }));
    await user.click(screen.getByRole("menuitem", { name: "Copy reference for Clovy" }));

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        noteReferenceToken({ id: "note-1", title: "Launch plan" }),
      ),
    );
  });

  it("exports the note from the actions menu", async () => {
    const user = userEvent.setup();
    const onExportPdf = vi.fn();
    render(
      createElement(NoteHeaderActions, {
        noteId: "note-1",
        noteTitle: "Launch plan",
        onExportPdf,
      }),
    );

    await user.click(screen.getByRole("button", { name: "Note actions" }));
    await user.click(screen.getByRole("menuitem", { name: "Export as PDF" }));

    expect(onExportPdf).toHaveBeenCalledTimes(1);
  });

  it("downloads audio once and closes the actions menu", async () => {
    const user = userEvent.setup();
    const onDownloadAudio = vi.fn();
    render(
      createElement(NoteHeaderActions, {
        noteId: "note-1",
        noteTitle: "Launch plan",
        onDownloadAudio,
      }),
    );

    await user.click(screen.getByRole("button", { name: "Note actions" }));
    await user.click(screen.getByRole("menuitem", { name: "Download audio" }));

    expect(onDownloadAudio).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("omits audio download when no handler is provided", async () => {
    const user = userEvent.setup();
    render(createElement(NoteHeaderActions, { noteId: "note-1", noteTitle: "Launch plan" }));

    await user.click(screen.getByRole("button", { name: "Note actions" }));

    expect(screen.queryByRole("menuitem", { name: "Download audio" })).not.toBeInTheDocument();
  });
});
