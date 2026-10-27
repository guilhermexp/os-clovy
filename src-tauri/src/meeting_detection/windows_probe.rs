//! Windows microphone-ownership probe.
//!
//! The reader (WASAPI capture sessions, a Toolhelp process snapshot and the
//! microphone consent store) only collects raw data; `windows_input_processes`
//! turns that data into the same `MicrophoneInputProcess` list the macOS
//! CoreAudio probe produces, so the parser is testable on any host.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use super::MicrophoneInputProcess;

/// One row of the Toolhelp process snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessEntry {
    pub(crate) pid: u32,
    pub(crate) parent_pid: u32,
    pub(crate) exe_name: String,
}

/// One subkey of `CapabilityAccessManager\ConsentStore\microphone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConsentEntry {
    /// Package family name for packaged apps, `#`-separated path for
    /// `NonPackaged` ones.
    pub(crate) key: String,
    pub(crate) packaged: bool,
    pub(crate) last_used_time_start: u64,
    pub(crate) last_used_time_stop: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbeSnapshot {
    /// Process ids of capture sessions in `AudioSessionStateActive`.
    pub(crate) active_capture_pids: Vec<u32>,
    pub(crate) processes: Vec<ProcessEntry>,
    pub(crate) consent: Vec<ConsentEntry>,
}

pub(crate) fn windows_input_processes(_snapshot: &ProbeSnapshot) -> Vec<MicrophoneInputProcess> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting_detection::{
        active_allowed_external_processes, meeting_end_bundle_families,
    };
    use std::collections::BTreeSet;

    fn process(pid: u32, parent_pid: u32, exe_name: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            exe_name: exe_name.to_string(),
        }
    }

    fn consent(key: &str, packaged: bool, stop: u64) -> ConsentEntry {
        ConsentEntry {
            key: key.to_string(),
            packaged,
            last_used_time_start: 133_000_000_000_000_000,
            last_used_time_stop: stop,
        }
    }

    fn found(snapshot: &ProbeSnapshot) -> Vec<(u32, String, String)> {
        windows_input_processes(snapshot)
            .into_iter()
            .map(|process| (process.pid, process.bundle_id, process.app_label))
            .collect()
    }

    fn row(pid: u32, bundle_id: &str, label: &str) -> (u32, String, String) {
        (pid, bundle_id.to_string(), label.to_string())
    }

    fn base_processes() -> Vec<ProcessEntry> {
        vec![
            process(4, 0, "System"),
            process(100, 4, "explorer.exe"),
            // Chrome: browser process plus its audio service utility child.
            process(200, 100, "chrome.exe"),
            process(210, 200, "chrome.exe"),
            process(211, 200, "chrome.exe"),
            // New Teams hosting WebView2 processes.
            process(300, 100, "ms-teams.exe"),
            process(310, 300, "msedgewebview2.exe"),
            process(320, 310, "msedgewebview2.exe"),
            // Clovy with its own WebView2 and the dictation helper.
            process(400, 100, "clovy.exe"),
            process(410, 400, "msedgewebview2.exe"),
            process(420, 410, "msedgewebview2.exe"),
            process(430, 400, "clovy-dictation-helper.exe"),
            // Firefox with a content child.
            process(500, 100, "firefox.exe"),
            process(510, 500, "firefox.exe"),
            process(600, 100, "obs64.exe"),
        ]
    }

    #[test]
    fn chromium_audio_service_session_resolves_to_the_browser_process() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![210],
            processes: base_processes(),
            consent: Vec::new(),
        };

        assert_eq!(found(&snapshot), vec![row(200, "chrome.exe", "Chrome")]);
    }

    #[test]
    fn webview2_session_resolves_to_its_host_app() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![320],
            processes: base_processes(),
            consent: Vec::new(),
        };

        assert_eq!(found(&snapshot), vec![row(300, "ms-teams.exe", "Teams")]);
    }

    #[test]
    fn sessions_of_one_app_collapse_to_one_process() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![211, 210, 200, 0, 9_999],
            processes: base_processes(),
            consent: Vec::new(),
        };

        assert_eq!(found(&snapshot), vec![row(200, "chrome.exe", "Chrome")]);
    }

    #[test]
    fn clovy_and_its_helper_never_trigger_even_through_webview2() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![420, 430],
            processes: base_processes(),
            consent: vec![consent("C:#Program Files#Clovy#clovy.exe", false, 0)],
        };
        let processes = windows_input_processes(&snapshot);
        let owned = BTreeSet::from([400, 430]);

        assert_eq!(
            processes
                .iter()
                .map(|process| process.pid)
                .collect::<Vec<_>>(),
            vec![400, 430]
        );
        assert!(active_allowed_external_processes(&processes, &owned).is_empty());
        assert_eq!(
            meeting_end_bundle_families(&processes, &owned),
            Some(BTreeSet::new())
        );
    }

    #[test]
    fn unlisted_capture_process_is_reported_but_not_allowed() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![600, 210],
            processes: base_processes(),
            consent: Vec::new(),
        };
        let processes = windows_input_processes(&snapshot);

        assert_eq!(
            found(&snapshot),
            vec![
                row(200, "chrome.exe", "Chrome"),
                row(600, "obs64.exe", "obs64"),
            ]
        );
        assert_eq!(
            active_allowed_external_processes(&processes, &BTreeSet::new())
                .into_iter()
                .map(|process| process.pid)
                .collect::<Vec<_>>(),
            vec![200]
        );
        assert_eq!(
            meeting_end_bundle_families(&processes, &BTreeSet::new()),
            None
        );
    }

    #[test]
    fn consent_store_adds_a_running_allowed_browser_the_sessions_missed() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: Vec::new(),
            processes: base_processes(),
            consent: vec![consent(
                "C:#Program Files#Mozilla Firefox#firefox.exe",
                false,
                0,
            )],
        };

        assert_eq!(found(&snapshot), vec![row(500, "firefox.exe", "Firefox")]);
    }

    #[test]
    fn consent_store_adds_a_packaged_app_by_package_family_name() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: Vec::new(),
            processes: base_processes(),
            consent: vec![consent("MSTeams_8wekyb3d8bbwe", true, 0)],
        };

        assert_eq!(found(&snapshot), vec![row(300, "ms-teams.exe", "Teams")]);
    }

    #[test]
    fn consent_store_ignores_stale_stopped_and_unlisted_entries() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: Vec::new(),
            processes: base_processes(),
            consent: vec![
                // In use, but Zoom is not running: a stale entry.
                consent("C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", false, 0),
                // Firefox stopped using the microphone.
                consent(
                    "C:#Program Files#Mozilla Firefox#firefox.exe",
                    false,
                    133_000_000_000_000_100,
                ),
                // Running and in use, but not an allowed meeting app.
                consent("C:#Program Files#obs-studio#bin#64bit#obs64.exe", false, 0),
                ConsentEntry {
                    last_used_time_start: 0,
                    ..consent(
                        "C:#Program Files#Google#Chrome#Application#chrome.exe",
                        false,
                        0,
                    )
                },
            ],
        };

        assert!(found(&snapshot).is_empty());
    }

    #[test]
    fn consent_store_does_not_duplicate_an_app_the_sessions_found() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![210],
            processes: base_processes(),
            consent: vec![consent(
                "C:#Program Files#Google#Chrome#Application#chrome.exe",
                false,
                0,
            )],
        };

        assert_eq!(found(&snapshot), vec![row(200, "chrome.exe", "Chrome")]);
    }

    #[test]
    fn parent_walk_stops_on_cycles() {
        let snapshot = ProbeSnapshot {
            active_capture_pids: vec![700],
            processes: vec![
                process(700, 701, "msedgewebview2.exe"),
                process(701, 700, "msedgewebview2.exe"),
            ],
            consent: Vec::new(),
        };

        let processes = windows_input_processes(&snapshot);
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].bundle_id, "msedgewebview2.exe");
    }
}
