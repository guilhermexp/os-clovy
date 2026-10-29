use super::*;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn manifest() -> Manifest {
    let files = [
        ("Encoder.mlmodelc/model.mil", b"encoder".as_slice()),
        ("parakeet_vocab.json", b"vocabulary".as_slice()),
    ]
    .into_iter()
    .map(|(path, data)| ModelFile {
        path: path.into(),
        bytes: data.len() as u64,
        sha256: format!("{:x}", Sha256::digest(data)),
    })
    .collect();
    Manifest {
        repository: "fixture/model".into(),
        revision: "fixture".into(),
        total_bytes: 17,
        files,
    }
}
struct Server {
    url: String,
    seen: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(corrupt: bool, delay: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let requests = seen.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            while !data.ends_with(b"\r\n\r\n") {
                let mut b = [0; 1];
                if stream.read(&mut b).await.unwrap() == 0 {
                    break;
                }
                data.extend_from_slice(&b);
            }
            let request = String::from_utf8(data).unwrap();
            requests.lock().unwrap().push(request.clone());
            let body: &[u8] = if request.contains("model.mil") {
                b"encoder"
            } else if corrupt {
                b"corruption"
            } else {
                b"vocabulary"
            };
            let start = request
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .map(|v| v.trim_end_matches('-').parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let headers = if start > 0 {
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\n",
                    body.len() - 1,
                    body.len()
                )
            } else {
                "HTTP/1.1 200 OK\r\n".into()
            };
            if stream
                .write_all(
                    format!(
                        "{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len() - start
                    )
                    .as_bytes(),
                )
                .await
                .is_err()
            {
                continue;
            }
            for byte in &body[start..] {
                if delay {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
                if stream.write_all(&[*byte]).await.is_err() {
                    break;
                }
            }
        }
    });
    Server { url, seen, task }
}
#[tokio::test]
async fn install_verifies_bytes_and_reopens_ready_without_network() {
    let temp = tempfile::tempdir().unwrap();
    let server = server(false, false).await;
    let model = TranscriptionModel::new(temp.path().join("model"), manifest());
    let progress = Mutex::new(Vec::new());
    model
        .install(&server.url, |s| progress.lock().unwrap().push(s))
        .await
        .unwrap();
    assert_eq!(model.state(), ModelState::Ready { bytes: 17 });
    assert!(progress.lock().unwrap().iter().any(|s| matches!(
        s,
        ModelState::Downloading {
            received: 17,
            total: 17
        }
    )));
    let reopened = TranscriptionModel::new(model.dir.clone(), manifest());
    assert_eq!(reopened.state(), ModelState::Ready { bytes: 17 });
    reopened.install(&server.url, |_| {}).await.unwrap();
    assert_eq!(server.seen.lock().unwrap().len(), 2);
    reopened.remove().await.unwrap();
    assert_eq!(reopened.state(), ModelState::Absent);
    assert!(!model.dir.exists());
}
#[tokio::test]
async fn corrupt_file_preserves_verified_files_and_retry_fetches_only_missing() {
    let temp = tempfile::tempdir().unwrap();
    let bad = server(true, false).await;
    let model = TranscriptionModel::new(temp.path().join("model"), manifest());
    let error = model.install(&bad.url, |_| {}).await.unwrap_err();
    assert!(error.contains("parakeet_vocab.json"));
    assert!(matches!(model.state(), ModelState::Failed { .. }));
    assert_eq!(
        std::fs::read(model.dir.join("Encoder.mlmodelc/model.mil")).unwrap(),
        b"encoder"
    );
    let good = server(false, false).await;
    model.install(&good.url, |_| {}).await.unwrap();
    assert_eq!(good.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn cancel_returns_absent_and_resume_uses_range() {
    let temp = tempfile::tempdir().unwrap();
    let server = server(false, true).await;
    let model = TranscriptionModel::new(temp.path().join("model"), manifest());
    model
        .install(&server.url, |s| {
            if matches!(s, ModelState::Downloading { received: 3, .. }) {
                model.cancel();
            }
        })
        .await
        .unwrap();
    assert_eq!(model.state(), ModelState::Absent);
    assert!(!model.dir.join("installed.json").exists());
    let reopened = TranscriptionModel::new(model.dir.clone(), manifest());
    assert_eq!(reopened.state(), ModelState::Absent);
    reopened.install(&server.url, |_| {}).await.unwrap();
    assert_eq!(reopened.state(), ModelState::Ready { bytes: 17 });
    assert!(server
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.to_ascii_lowercase().contains("range: bytes=3-")));
}
#[test]
fn disk_without_marker_is_verified_and_size_changes_invalidate_marker() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("model");
    std::fs::create_dir_all(dir.join("Encoder.mlmodelc")).unwrap();
    std::fs::write(dir.join("Encoder.mlmodelc/model.mil"), b"encoder").unwrap();
    assert_eq!(
        TranscriptionModel::new(dir.clone(), manifest()).state(),
        ModelState::Absent
    );
    std::fs::write(dir.join("parakeet_vocab.json"), b"vocabulary").unwrap();
    assert_eq!(
        TranscriptionModel::new(dir.clone(), manifest()).state(),
        ModelState::Ready { bytes: 17 }
    );
    std::fs::write(dir.join("parakeet_vocab.json"), b"bad").unwrap();
    assert_eq!(
        TranscriptionModel::new(dir, manifest()).state(),
        ModelState::Absent
    );
}
