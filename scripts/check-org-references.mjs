#!/usr/bin/env node
// Fails when tracked files still reference the Open Software organization's
// infrastructure (its domains, the short-link domain, the GitHub organization,
// or Blacksmith runners). Each scope pairs git pathspecs with the pattern that
// must not match there; add a scope instead of widening an existing one.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const rootDir = path.resolve(path.dirname(scriptPath), "..");
const selfPath = path.relative(rootDir, scriptPath).split(path.sep).join("/");

export const ORG_REFERENCE_SCOPES = [
  {
    name: "repository and CI",
    pathspecs: [".github", "clovy-api", "scripts", "Makefile"],
    pattern: /opensoftware\.co\b|june\.link|open-software-network|blacksmith/,
  },
];

export function findOrgReferences({ cwd = rootDir, scopes = ORG_REFERENCE_SCOPES } = {}) {
  const findings = [];
  for (const scope of scopes) {
    const files = execFileSync("git", ["ls-files", "-z", "--", ...scope.pathspecs], {
      cwd,
      encoding: "utf8",
    })
      .split("\0")
      .filter((file) => file && file !== selfPath);
    for (const file of files) {
      let text;
      try {
        text = readFileSync(path.join(cwd, file), "utf8");
      } catch {
        continue;
      }
      text.split("\n").forEach((line, index) => {
        if (scope.pattern.test(line)) {
          findings.push({ scope: scope.name, file, line: index + 1, text: line.trim() });
        }
      });
    }
  }
  return findings;
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const findings = findOrgReferences();
  for (const finding of findings) {
    console.error(`${finding.file}:${finding.line}: ${finding.text}`);
  }
  if (findings.length > 0) {
    console.error(`${findings.length} organization reference(s) found.`);
    process.exit(1);
  }
  console.log("No organization references found.");
}
