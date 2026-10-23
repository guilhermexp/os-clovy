//! Child processes without a console window on Windows.
//!
//! The release app is a GUI-subsystem program, so every console child it
//! starts (`cmd.exe` for npm `.cmd` shims, `node`, `gh`, the agent CLIs, the
//! sidecar) would open its own console, which the default terminal shows as
//! a terminal window. Every process Clovy creates is built here.
//!
//! `CREATE_NO_WINDOW` only affects console programs; a GUI program such as
//! `explorer.exe` still shows its window.

use std::ffi::OsStr;

/// `CREATE_NO_WINDOW`: the console child runs without a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A process command that can be told not to open a console.
pub trait HideConsole {
    /// Starts the child without a console window on Windows; no effect
    /// elsewhere.
    fn hide_console(&mut self) -> &mut Self;
}

impl HideConsole for std::process::Command {
    fn hide_console(&mut self) -> &mut Self {
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(self, CREATE_NO_WINDOW);
        self
    }
}

impl HideConsole for tokio::process::Command {
    fn hide_console(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}

/// `std::process::Command::new` without a console window.
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command.hide_console();
    command
}

/// `tokio::process::Command::new` without a console window.
pub fn tokio_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.hide_console();
    command
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Files that only hold tests (whole-file test modules).
    fn is_test_file(path: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        name == "tests.rs" || name.ends_with("_tests.rs") || name == "transaction_regressions.rs"
    }

    /// Line where the file's `#[cfg(test)]` module starts; code after it is
    /// test code.
    fn test_module_start(lines: &[&str]) -> usize {
        lines
            .windows(2)
            .position(|pair| {
                let attribute = pair[0].trim_start();
                let item = pair[1].trim_start();
                (attribute.starts_with("#[cfg(test") || attribute.starts_with("#[cfg(all(test"))
                    && (item.starts_with("mod ")
                        || item.starts_with("pub mod ")
                        || item.starts_with("pub(crate) mod "))
                    && item.ends_with('{')
            })
            .unwrap_or(lines.len())
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Every process Clovy starts goes through this module, so no new spawn
    /// site can bring the console windows back.
    #[test]
    fn every_process_is_created_without_a_console() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        let mut offenders = Vec::new();
        for file in files {
            if is_test_file(&file) || file.ends_with("hide_console.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            let lines: Vec<&str> = source.lines().collect();
            let production = &lines[..test_module_start(&lines)];
            for (index, line) in production.iter().enumerate() {
                if line.contains("Command::new(") && !line.trim_start().starts_with("//") {
                    offenders.push(format!(
                        "{}:{}",
                        file.strip_prefix(&root).unwrap().display(),
                        index + 1
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "create processes with hide_console::command / hide_console::tokio_command:\n{}",
            offenders.join("\n")
        );
    }
}
