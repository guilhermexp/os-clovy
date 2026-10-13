#[path = "support/charge_server.rs"]
mod charge_server;

use charge_server::start_charge_server_with_responses;
use clovy_domain::{ChargeRequest, Credits, OsAccountsClient};
use clovy_providers::{OsAccountsHttpClient, default_client};
use std::error::Error;

#[tokio::test]
async fn a_head_probe_does_not_consume_the_charge_receipt() -> Result<(), Box<dyn Error>> {
    let api_url = start_charge_server_with_responses([concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: application/json\r\n",
        "Content-Length: 71\r\n",
        "Connection: close\r\n",
        "\r\n",
        r#"{"success":true,"data":{"credits_settled":12,"idempotent_replay":true}}"#,
    )
    .as_bytes()
    .to_vec()])
    .await;

    // A local HTTP discovery probe must not spend the scripted POST receipt.
    let probe_status = default_client().head(&api_url).send().await?.status();
    let client = OsAccountsHttpClient::new(default_client(), &api_url, "osk_test");
    let receipt = client
        .charge(ChargeRequest {
            action_token: "agts_test".to_string(),
            credits: Credits(12),
            idempotency_key: "note_generate:usr_123:note_1:v7".to_string(),
        })
        .await;

    assert_eq!(
        receipt.map(|value| (value.credits_charged.0, value.idempotent_replay)),
        Ok((12, true))
    );
    assert_eq!(probe_status, reqwest::StatusCode::NOT_FOUND);
    Ok(())
}
