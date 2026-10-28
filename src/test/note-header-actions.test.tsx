import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NoteHeaderActions } from "../components/note-editor/NoteHeaderActions";

describe("local note actions", () => {
  it("keeps chat, PDF export and audio download without publishing a share", () => {
    const onAskClovy = vi.fn();
    const onExportPdf = vi.fn();
    const onDownloadAudio = vi.fn();
    render(
      <NoteHeaderActions
        noteId="note-1"
        noteTitle="Synthetic note"
        onAskClovy={onAskClovy}
        onExportPdf={onExportPdf}
        onDownloadAudio={onDownloadAudio}
      />,
    );
    expect(screen.queryByRole("button", { name: /share/i })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Ask Clovy" }));
    expect(onAskClovy).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Note actions" }));
    expect(screen.queryByRole("menuitem", { name: /share/i })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Export as PDF" }));
    expect(onExportPdf).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Note actions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Download audio" }));
    expect(onDownloadAudio).toHaveBeenCalledOnce();
  });
});
