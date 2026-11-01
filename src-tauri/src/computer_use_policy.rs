//! Shared desktop application safety boundary, including pre-launch resolution.
pub(crate) fn blocked_app(app: &str) -> bool {
    let normalized: String = app
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "clovy",
        "terminal",
        "windowsterminal",
        "cmdexe",
        "powershell",
        "pwsh",
        "conhost",
        "openconsole",
        "iterm",
        "warp",
        "alacritty",
        "kitty",
        "ghostty",
        "wezterm",
        "hyper",
        "codex",
        "chatgpt",
        "systemsettings",
        "systempreferences",
        "universalaccessauthwarn",
        "securityagent",
        "credentialui",
        "consentexe",
        "logonuiexe",
        "taskmgr",
        "regedit",
        "rundll32",
        "controlexe",
        "mmcexe",
        "keychainaccess",
        "loginwindow",
        "installer",
        "activitymonitor",
        "scripteditor",
        "automator",
        "shortcuts",
        "1password",
        "bitwarden",
        "dashlane",
        "keepass",
        "keeperpassword",
        "nordpass",
        "protonpass",
        "roboform",
        "strongbox",
        "enpass",
    ]
    .iter()
    .any(|blocked| normalized.contains(blocked))
}
