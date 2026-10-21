//! Windows test support: a fake CLI installed the way npm installs one, a
//! `.cmd` shim in front of a Node script. The script records the argv and
//! stdin it received as JSON (`{"args": [...], "stdin": "..."}`).

use serde_json::Value;
use std::path::{Path, PathBuf};

/// The template npm writes for a package binary (`cmd-shim`), as found in
/// `%APPDATA%\npm\codex.cmd`.
const NPM_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\fake-cli.js\" %*\r\n";

/// Writes `<dir>/<name>.cmd`; each run overwrites `record`.
pub(crate) fn npm_cmd_cli(dir: &Path, name: &str, record: &Path) -> PathBuf {
    let script = format!(
        "let data = \"\";\nprocess.stdin.setEncoding(\"utf8\");\nprocess.stdin.on(\"data\", (chunk) => (data += chunk));\nprocess.stdin.on(\"end\", () => require(\"fs\").writeFileSync({record}, JSON.stringify({{ args: process.argv.slice(2), stdin: data }})));\n",
        record = Value::String(record.display().to_string())
    );
    std::fs::write(dir.join("fake-cli.js"), script).unwrap();
    let shim = dir.join(format!("{name}.cmd"));
    std::fs::write(&shim, NPM_SHIM).unwrap();
    shim
}

/// What the fake CLI recorded, if it ran.
pub(crate) fn read_record(record: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(record).ok()?;
    serde_json::from_str(&text).ok()
}
