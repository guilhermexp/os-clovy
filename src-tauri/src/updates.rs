#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_are_disabled_in_dev_builds() {
        // Tests run as a development build: it never checks or installs.
        assert!(cfg!(debug_assertions));
        assert!(!updates_enabled());
    }

    #[test]
    fn the_endpoint_lives_only_in_the_tauri_config() {
        let source = include_str!("updates.rs");
        assert!(!source.contains(concat!("https", "://")));
        assert!(!source.contains(concat!("Release", "Channel")));
    }

    // The frontend's download-progress throttling (update-decision.ts) reads
    // `event.event` and `event.data.contentLength` / `event.data.chunkLength`
    // verbatim, so these serde shapes are a hard wire contract, not cosmetics.
    #[test]
    fn started_event_carries_content_length_under_data() {
        let json = serde_json::to_string(&DownloadEvent::Started {
            content_length: Some(100),
        })
        .unwrap();
        assert_eq!(json, r#"{"event":"Started","data":{"contentLength":100}}"#);
    }

    #[test]
    fn started_event_omits_content_length_when_unknown() {
        let json = serde_json::to_string(&DownloadEvent::Started {
            content_length: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"event":"Started","data":{}}"#);
    }

    #[test]
    fn progress_event_carries_chunk_length_under_data() {
        let json = serde_json::to_string(&DownloadEvent::Progress { chunk_length: 5 }).unwrap();
        assert_eq!(json, r#"{"event":"Progress","data":{"chunkLength":5}}"#);
    }

    #[test]
    fn finished_event_has_no_data_field() {
        let json = serde_json::to_string(&DownloadEvent::Finished).unwrap();
        assert_eq!(json, r#"{"event":"Finished"}"#);
    }

    #[test]
    fn update_meta_exposes_version_and_notes_to_the_frontend() {
        let json = serde_json::to_string(&UpdateMeta {
            version: "0.1.1".into(),
            body: Some("notes".into()),
        })
        .unwrap();
        assert_eq!(json, r#"{"version":"0.1.1","body":"notes"}"#);
    }

    #[test]
    fn update_meta_omits_absent_notes() {
        let json = serde_json::to_string(&UpdateMeta {
            version: "0.1.1".into(),
            body: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"version":"0.1.1"}"#);
    }
}
