import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ModelPickerDialog, modelOptions } from "../components/settings/ModelPickerDialog";

describe("ModelPickerDialog", () => {
  it("does not offer the retired automatic router", () => {
    const automatic = {
      provider: "open-software",
      id: "open-software/auto",
      name: "Auto",
      modelType: "text",
      traits: [],
      capabilities: [],
    };
    expect(modelOptions([automatic], automatic.id)).toEqual([]);
  });

  it("shows and selects a text model that does not call tools", async () => {
    const option = {
      provider: "openrouter",
      id: "notes/text-only",
      name: "Text only",
      modelType: "text",
      traits: [],
      capabilities: [],
    };
    const choose = vi.fn();
    render(
      <ModelPickerDialog
        open
        mode="generation"
        value=""
        options={[option]}
        search=""
        onSearchChange={vi.fn()}
        onClose={vi.fn()}
        onSelect={choose}
      />,
    );

    expect(screen.getByRole("dialog", { name: "Text model" })).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("option", { name: /text only/i }));
    expect(choose).toHaveBeenCalledWith(option.id);
  });
});
