// Private share viewer (JUN-308). Everything sensitive happens here, in the
// recipient's browser: the invite key arrives in the URL fragment (never sent
// to any server), unwraps the content key, and decrypts the payload fetched
// from clovy-api. No analytics, no external requests (CSP-enforced).
"use strict";
(function () {
  var statusEl = document.getElementById("status");
  var NOT_AVAILABLE =
    "This link isn't available. It may have been revoked.";
  var INVITE_NEEDS_APP =
    "This share is for a specific recipient. Open it in Clovy, or ask the " +
    "sender for a passcode link.";

  function showStatus(text, isError) {
    statusEl.hidden = false;
    statusEl.textContent = text;
    statusEl.className = isError ? "error" : "";
    document.getElementById("passcode").hidden = true;
    document.getElementById("content").hidden = true;
  }

  function b64urlDecode(text) {
    var normalized = text.replace(/-/g, "+").replace(/_/g, "/");
    while (normalized.length % 4 !== 0) normalized += "=";
    return b64Decode(normalized);
  }

  function b64Decode(text) {
    var bin = atob(text);
    var bytes = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }

  async function aesGcmDecrypt(keyBytes, iv, ciphertext) {
    var key = await crypto.subtle.importKey(
      "raw", keyBytes, { name: "AES-GCM" }, false, ["decrypt"]
    );
    var plain = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: iv }, key, ciphertext
    );
    return new Uint8Array(plain);
  }

  async function derivePasscodeKey(passcode, salt) {
    var material = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(passcode.normalize("NFKC")),
      "PBKDF2",
      false,
      ["deriveBits"]
    );
    var bits = await crypto.subtle.deriveBits(
      { name: "PBKDF2", hash: "SHA-256", salt: salt, iterations: 600000 },
      material,
      256
    );
    return new Uint8Array(bits);
  }

  // ── Rendering ─────────────────────────────────────────────────────────────
  function escapeHtml(text) {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function inlineMarkdown(text) {
    return text
      .replace(/`([^`]+)`/g, function (_m, code) { return "<code>" + code + "</code>"; })
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/\*([^*]+)\*/g, "<em>$1</em>")
      .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, function (_m, label, url) {
        return '<a href="' + url + '" rel="noreferrer nofollow noopener" target="_blank">' + label + "</a>";
      });
  }

  // Deliberately small markdown subset: headings, lists, code fences,
  // paragraphs, bold/italic/code/links. Input is escaped FIRST; the only
  // tags in the output are the ones this function emits.
  function renderMarkdown(markdown) {
    var lines = escapeHtml(markdown.replace(/\r\n/g, "\n")).split("\n");
    var html = [];
    var inCode = false;
    var inList = false;
    function closeList() {
      if (inList) { html.push("</ul>"); inList = false; }
    }
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (/^```/.test(line)) {
        closeList();
        html.push(inCode ? "</code></pre>" : "<pre><code>");
        inCode = !inCode;
        continue;
      }
      if (inCode) { html.push(line + "\n"); continue; }
      var heading = /^(#{1,3})\s+(.*)$/.exec(line);
      if (heading) {
        closeList();
        var level = heading[1].length;
        html.push("<h" + level + ">" + inlineMarkdown(heading[2]) + "</h" + level + ">");
        continue;
      }
      var item = /^\s*[-*]\s+(.*)$/.exec(line);
      if (item) {
        if (!inList) { html.push("<ul>"); inList = true; }
        html.push("<li>" + inlineMarkdown(item[1]) + "</li>");
        continue;
      }
      closeList();
      if (line.trim() === "") continue;
      html.push("<p>" + inlineMarkdown(line) + "</p>");
    }
    if (inCode) html.push("</code></pre>");
    closeList();
    return html.join("");
  }

  function renderSession(messages) {
    var html = [];
    for (var i = 0; i < messages.length; i++) {
      var role = messages[i].role === "user" ? "user" : "assistant";
      var label = role === "user" ? "You" : "Clovy";
      html.push(
        '<div class="turn ' + role + '" aria-label="' + label + '"><div class="turn-body">' +
        renderMarkdown(String(messages[i].content || "")) +
        "</div></div>"
      );
    }
    return html.join("");
  }

  function showContent(payload) {
    statusEl.hidden = true;
    document.getElementById("topbar").hidden = false;
    var content = document.getElementById("content");
    var kindLabel = payload.kind === "session" ? "session" : "meeting note";
    var owner = payload.owner_name || "Someone";
    document.getElementById("content-title").textContent =
      payload.title || "Untitled";
    document.getElementById("content-meta").textContent =
      owner + " shared this " + kindLabel + " with you";
    var body = document.getElementById("content-body");
    if (payload.kind === "session" && Array.isArray(payload.messages)) {
      body.innerHTML = renderSession(payload.messages);
    } else {
      body.innerHTML = renderMarkdown(String(payload.markdown || ""));
    }
    content.hidden = false;
  }

  // ── Main flow ─────────────────────────────────────────────────────────────
  async function decryptLinkData(data, linkKey) {
    var contentKey = await aesGcmDecrypt(
      linkKey,
      b64Decode(data.envelopeIvB64),
      b64Decode(data.envelopeB64)
    );
    var plaintext = await aesGcmDecrypt(
      contentKey,
      b64Decode(data.ivB64),
      b64Decode(data.ciphertextB64)
    );
    var payload = JSON.parse(new TextDecoder().decode(plaintext));
    document.getElementById("passcode").hidden = true;
    showContent(payload);
  }

  async function viewLinkShare(shareId, fragment) {
    var parts = fragment.split(".");
    if (parts.length !== 4 || parts[0] !== "link" ||
        (parts[2] !== "key" && parts[2] !== "pass")) {
      showStatus("This link is incomplete. Ask for a fresh share link.", true);
      return;
    }
    var inviteId = parts[1];
    var material;
    try { material = b64urlDecode(parts[3]); } catch (error) {
      showStatus("This link is incomplete. Ask for a fresh share link.", true);
      return;
    }
    showStatus("Loading encrypted share…", false);
    var response = await fetch(
      "/v1/shares/" + encodeURIComponent(shareId) +
      "/link-view?link=" + encodeURIComponent(inviteId)
    );
    if (!response.ok) {
      showStatus("This link isn't available. It may have been stopped.", true);
      return;
    }
    var body = await response.json().catch(function () { return null; });
    var data = body && body.success && body.data;
    if (!data || !data.envelopeB64) {
      showStatus("This link isn't available. It may have been stopped.", true);
      return;
    }
    if (parts[2] === "key") {
      if (material.length !== 32) throw new Error("invalid link key");
      try {
        await decryptLinkData(data, material);
      } catch (error) {
        showStatus("This link's key doesn't match its content.", true);
      }
      return;
    }
    if (material.length !== 16) throw new Error("invalid passcode salt");
    statusEl.hidden = true;
    var panel = document.getElementById("passcode");
    var form = document.getElementById("passcode-form");
    var input = document.getElementById("passcode-input");
    var errorEl = document.getElementById("passcode-error");
    var toggle = document.getElementById("passcode-toggle");
    panel.hidden = false;
    input.focus();
    if (toggle) {
      toggle.onclick = function () {
        var reveal = input.type === "password";
        input.type = reveal ? "text" : "password";
        toggle.setAttribute("aria-pressed", reveal ? "true" : "false");
        toggle.setAttribute(
          "aria-label",
          reveal ? "Hide passcode" : "Show passcode"
        );
        input.focus();
      };
    }
    form.onsubmit = async function (event) {
      event.preventDefault();
      errorEl.hidden = true;
      try {
        var linkKey = await derivePasscodeKey(input.value, material);
        await decryptLinkData(data, linkKey);
      } catch (error) {
        errorEl.textContent = "That passcode didn't work.";
        errorEl.hidden = false;
        input.select();
      }
    };
  }

  function viewShare() {
    // Invite shares are decrypted for a signed-in recipient; the browser
    // viewer has no sign-in, so only passcode link shares open here.
    showStatus(INVITE_NEEDS_APP, true);
    return Promise.resolve();
  }

  function main() {
    if (!crypto || !crypto.subtle) {
      showStatus("This browser doesn't support the decryption this page needs.", true);
      return;
    }
    var path = location.pathname;
    var match = /^\/s\/(shr_[A-Za-z0-9_-]+)$/.exec(path);
    if (!match) {
      showStatus(NOT_AVAILABLE, true);
      return;
    }
    var fragment = location.hash.replace(/^#/, "");
    if (!fragment) {
      showStatus("This link is incomplete. Ask for a fresh share link.", true);
      return;
    }
    var flow = fragment.indexOf("link.") === 0 ? viewLinkShare : viewShare;
    flow(match[1], fragment).catch(function () {
      showStatus(NOT_AVAILABLE, true);
    });
  }

  main();
})();
