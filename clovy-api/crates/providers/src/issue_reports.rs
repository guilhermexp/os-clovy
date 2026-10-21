use async_trait::async_trait;
use clovy_domain::{DomainError, IssueReport, IssueReportDelivery, IssueReportSink};

fn report_file_names(report: &IssueReport) -> Vec<String> {
    let mut names = Vec::new();
    for name in report
        .attachment_names
        .iter()
        .chain(report.attachments.iter().map(|attachment| &attachment.name))
    {
        let name = name.trim();
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// The only issue report sink: the report becomes a structured log line, so it
/// still reaches whoever reads the service logs.
pub struct LogIssueReportSink;

fn log_issue_report(report: &IssueReport) {
    tracing::warn!(
        user_id = %report.user_id.0,
        description = %report.description,
        agent_diagnosis = report.agent_diagnosis.as_deref().unwrap_or(""),
        attachment_names = ?report.attachment_names,
        // The Debug impl reports name/type/length only; attachment bytes
        // never reach the logs.
        attachments = ?report.attachments,
        category = report.category.as_deref().unwrap_or(""),
        session_id = report.session_id.as_deref().unwrap_or(""),
        app_version = report.app_version.as_deref().unwrap_or(""),
        platform = report.platform.as_deref().unwrap_or(""),
        "issue_reports: report logged only"
    );
}

#[async_trait]
impl IssueReportSink for LogIssueReportSink {
    async fn deliver(&self, report: IssueReport) -> Result<IssueReportDelivery, DomainError> {
        log_issue_report(&report);
        Ok(IssueReportDelivery {
            unattached_names: report_file_names(&report),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clovy_domain::UserId;

    fn report() -> IssueReport {
        IssueReport {
            user_id: UserId("usr_test".to_string()),
            category: Some("bug".to_string()),
            description: "The recorder freezes\nwhen I pause it".to_string(),
            agent_diagnosis: Some("Likely the audio capture thread".to_string()),
            attachment_names: vec!["screenshot.png".to_string()],
            attachments: vec![clovy_domain::IssueReportAttachment {
                name: "screenshot.png".to_string(),
                content_type: "image/png".to_string(),
                bytes: bytes::Bytes::from_static(b"png-bytes"),
            }],
            session_id: Some("session-1".to_string()),
            app_version: Some("0.0.7".to_string()),
            platform: Some("macos".to_string()),
        }
    }

    #[tokio::test]
    async fn log_sink_returns_named_files_as_unattached() {
        let mut report = report();
        report.attachment_names.push("local-only.mov".to_string());

        let delivery = LogIssueReportSink
            .deliver(report)
            .await
            .expect("log-only delivery succeeds");

        assert_eq!(
            delivery.unattached_names,
            vec!["screenshot.png", "local-only.mov"]
        );
    }
}
