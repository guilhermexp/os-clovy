function _clovyGuard(doc, documentId, backendNodeId, expected, operation, text, key) {
  if (!this.isConnected || this.ownerDocument !== doc || doc !== document)
    return { error: "stale_reference" };

  const safeText = (node) => {
    if (node.nodeType === 3) return node.nodeValue || "";
    if (node.nodeType !== 1) return "";
    if (node.matches("input,textarea,select,[contenteditable]") || node.isContentEditable)
      return "";
    return Array.from(node.childNodes).map(safeText).join("");
  };
  const tree = this.getRootNode();
  const referencedLabel = (this.getAttribute("aria-labelledby") || "")
    .split(/\s+/)
    .filter(Boolean)
    .map((id) => tree.getElementById?.(id) || doc.getElementById(id))
    .filter((node) => node && node !== this)
    .map(safeText)
    .join(" ")
    .trim();
  const explicitLabel = Array.from(this.labels || [])
    .map(safeText)
    .join(" ")
    .trim();
  const tag = this.tagName.toLowerCase();
  const inputType = tag === "input" ? this.type.toLowerCase() : "";
  const role =
    this.getAttribute("role") ||
    { a: "link", button: "button", textarea: "textbox", select: "combobox" }[tag] ||
    (tag === "input"
      ? { button: "button", submit: "button", checkbox: "checkbox", radio: "radio" }[inputType] ||
        "textbox"
      : "");
  const label =
    referencedLabel ||
    (this.getAttribute("aria-label") || "").trim() ||
    explicitLabel ||
    this.getAttribute("alt") ||
    this.getAttribute("title") ||
    (["input", "textarea", "select"].includes(tag) || this.isContentEditable
      ? ""
      : safeText(this).trim().slice(0, 1000));
  const name = this.getAttribute("name") || "";
  const element = {
    tag,
    inputType,
    role,
    name,
    id: this.id || "",
    label,
    autocomplete: this.getAttribute("autocomplete") || "",
    inForm: !!(this.form || this.closest("form")),
    contentEditable: this.isContentEditable,
  };
  const inspection = {
    url: location.href,
    origin: location.origin,
    documentId,
    backendNodeId,
    element,
  };
  if (!expected) return { inspection };
  const equal = (a, b) =>
    a === b ||
    (a &&
      b &&
      typeof a === "object" &&
      typeof b === "object" &&
      Object.keys(a).length === Object.keys(b).length &&
      Object.keys(a).every((k) => equal(a[k], b[k])));
  if (!equal(inspection, expected)) return { error: "stale_reference" };
  // Password, one-time codes and payment details are human-only, including click/press.
  if (
    inputType === "password" ||
    /(^|\s)(one-time-code|cc-[^\s]+)(\s|$)/i.test(element.autocomplete) ||
    /(?:password|passcode|otp|verification.?code|card.?number|cvv|cvc)/i.test(
      [element.name, element.id, label].join(" "),
    )
  )
    return { error: "sensitive_field" };
  const verify = () => {
    const check = _clovyGuard.call(this, doc, documentId, backendNodeId, null, null, null, null);
    return check.inspection && equal(check.inspection, expected);
  };
  if (operation === "click") {
    this.click();
    return { inspection };
  }
  if (operation === "fill") {
    if (
      (!["input", "textarea"].includes(tag) && !this.isContentEditable) ||
      this.disabled ||
      this.readOnly ||
      (tag === "input" &&
        ["hidden", "file", "checkbox", "radio", "button", "submit", "reset", "image"].includes(
          inputType,
        ))
    )
      return { error: "unsupported_operation" };
    this.focus();
    if (!verify() || this.getRootNode().activeElement !== this || this.readOnly || this.disabled)
      return { error: "effect_unknown" };
    if (this.isContentEditable) this.textContent = text;
    else {
      const setter = Object.getOwnPropertyDescriptor(
        tag === "input" ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype,
        "value",
      ).set;
      setter.call(this, text);
    }
    this.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }),
    );
    this.dispatchEvent(new Event("change", { bubbles: true }));
    return { inspection };
  }
  if (operation === "press") {
    if (!["Enter", "Space", "Escape", "Backspace", "Delete"].includes(key))
      return { error: "unsupported_key" };
    const editing = key === "Backspace" || key === "Delete";
    if (editing && (this.readOnly || this.disabled)) return { error: "unsupported_operation" };
    this.focus();
    if (!verify() || this.getRootNode().activeElement !== this) return { error: "effect_unknown" };
    if (editing && (this.readOnly || this.disabled)) return { error: "effect_unknown" };
    const actual = key === "Space" ? " " : key;
    const proceed = this.dispatchEvent(
      new KeyboardEvent("keydown", { key: actual, bubbles: true, cancelable: true }),
    );
    if (!verify() || this.getRootNode().activeElement !== this) return { error: "effect_unknown" };
    if (editing && (this.readOnly || this.disabled)) return { error: "effect_unknown" };
    if (proceed && key === "Enter") {
      if (tag === "button" || tag === "a" || ["submit", "button"].includes(inputType)) this.click();
      else if (this.form && tag !== "textarea")
        HTMLFormElement.prototype.requestSubmit.call(this.form);
    } else if (
      proceed &&
      key === "Space" &&
      (tag === "button" || ["checkbox", "radio", "button", "submit"].includes(inputType))
    )
      this.click();
    else if (
      proceed &&
      ["Backspace", "Delete"].includes(key) &&
      ["input", "textarea"].includes(tag)
    ) {
      const start = this.selectionStart,
        end = this.selectionEnd;
      if (start === null || end === null) return { error: "effect_unknown" };
      const from = start === end && key === "Backspace" ? Math.max(0, start - 1) : start;
      const to = start === end && key === "Delete" ? end + 1 : end;
      this.setRangeText("", from, to, "end");
      this.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: key === "Delete" ? "deleteContentForward" : "deleteContentBackward",
        }),
      );
    }
    this.dispatchEvent(new KeyboardEvent("keyup", { key: actual, bubbles: true }));
    return { inspection };
  }
  return { error: "unsupported_operation" };
}
