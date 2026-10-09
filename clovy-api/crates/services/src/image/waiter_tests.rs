//! Same-request waiters: a request that arrives while an attempt with the same
//! `requestId` is running waits for that attempt. When the attempt fails or is
//! interrupted, the waiter must get that outcome, never take the request over
//! and run a second paid generation behind the client's back.

use super::{
    ImageGenerateOutput, ImageGenerateParams, ImageModelPrice, ImageService, ImageServiceDeps,
};
use crate::ServiceError;
use async_trait::async_trait;
use clovy_domain::{
    Authorization, AuthorizeRequest, ChargeRequest, DomainError, GeneratedImage, ImageEditRequest,
    ImageEditor, ImageGenerationRequest, ImageGenerator, OsAccountsClient, ProviderCredentials,
    Receipt, UserId,
};
use std::collections::BTreeMap;
use std::future::poll_fn;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Poll;
use tokio::sync::Notify;

#[derive(Default)]
struct Wallet {
    authorizations: AtomicU64,
    paid_charges: AtomicU64,
}

impl Wallet {
    fn authorizations(&self) -> u64 {
        self.authorizations.load(Ordering::SeqCst)
    }

    fn paid_charges(&self) -> u64 {
        self.paid_charges.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl OsAccountsClient for Wallet {
    async fn authorize(&self, request: AuthorizeRequest) -> Result<Authorization, DomainError> {
        self.authorizations.fetch_add(1, Ordering::SeqCst);
        Ok(Authorization {
            allowed: true,
            action_token: Some("agt_waiter".to_string()),
            cap_credits: Some(request.estimate),
            reason: None,
        })
    }

    async fn charge(&self, request: ChargeRequest) -> Result<Receipt, DomainError> {
        if request.credits.0 > 0 {
            self.paid_charges.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Receipt {
            credits_charged: request.credits,
            idempotent_replay: false,
        })
    }
}

/// Holds the first generation open until the test releases it, then fails it
/// when asked to; every later call succeeds at once. Each call is a paid run.
struct FirstRunGate {
    calls: AtomicU64,
    fail_first: bool,
    entered: Notify,
    release: Notify,
}

impl FirstRunGate {
    fn new(fail_first: bool) -> Self {
        Self {
            calls: AtomicU64::new(0),
            fail_first,
            entered: Notify::new(),
            release: Notify::new(),
        }
    }

    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ImageGenerator for FirstRunGate {
    async fn generate(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<GeneratedImage, DomainError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 1 {
            self.entered.notify_one();
            self.release.notified().await;
            if self.fail_first {
                return Err(DomainError::UpstreamProvider);
            }
        }
        Ok(GeneratedImage {
            image_base64: format!("generated-{call}"),
            mime_type: "image/png".to_string(),
            model: request.model.0,
            provider: "fal".to_string(),
        })
    }
}

struct NoEdits;

#[async_trait]
impl ImageEditor for NoEdits {
    async fn edit(&self, _request: ImageEditRequest) -> Result<GeneratedImage, DomainError> {
        Err(DomainError::UpstreamProvider)
    }
}

fn service(wallet: Arc<Wallet>, generator: Arc<FirstRunGate>) -> Arc<ImageService> {
    Arc::new(ImageService::new(ImageServiceDeps {
        os_accounts: wallet,
        generator,
        editor: Arc::new(NoEdits),
        pricing: BTreeMap::from([("flux-dev".to_string(), ImageModelPrice::new(20))]),
        default_model: "flux-dev".to_string(),
        edit_pricing: BTreeMap::new(),
        default_edit_model: "flux-dev".to_string(),
        hold_ttl_seconds: 60,
    }))
}

fn same_request() -> ImageGenerateParams {
    ImageGenerateParams {
        user_id: UserId("usr_1".to_string()),
        request_id: Some("req_same".to_string()),
        prompt: "a cat".to_string(),
        model: "flux-dev".to_string(),
        width: None,
        height: None,
        safe_mode: None,
        provider_credentials: ProviderCredentials::default(),
    }
}

fn spawn_generate(
    service: &Arc<ImageService>,
) -> tokio::task::JoinHandle<Result<ImageGenerateOutput, ServiceError>> {
    let service = service.clone();
    tokio::spawn(async move { service.generate(same_request()).await })
}

/// Polls `future` once with the test task's own context. `Pending` proves the
/// waiter parked on the owner's attempt; its waker is then the test task's.
async fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    poll_fn(|cx| Poll::Ready(Pin::new(&mut *future).poll(cx))).await
}

#[tokio::test]
async fn a_waiter_on_a_failed_attempt_gets_its_failure_without_resubmitting() {
    let wallet = Arc::new(Wallet::default());
    let generator = Arc::new(FirstRunGate::new(true));
    let service = service(wallet.clone(), generator.clone());
    let owner = spawn_generate(&service);
    generator.entered.notified().await;
    let mut waiter = pin!(service.generate(same_request()));
    assert!(
        poll_once(&mut waiter).await.is_pending(),
        "the waiter must park on the owner's attempt"
    );

    generator.release.notify_one();
    let owner = owner.await.expect("owner task");
    let waiter = waiter.await;

    assert_eq!(generator.calls(), 1, "the waiter ran a second generation");
    assert!(matches!(owner, Err(ServiceError::UpstreamProvider)));
    assert!(
        matches!(waiter, Err(ServiceError::UpstreamProvider)),
        "waiter: {waiter:?}"
    );
    assert_eq!(wallet.authorizations(), 1);
    assert_eq!(wallet.paid_charges(), 0);
}

#[tokio::test]
async fn a_waiter_on_an_interrupted_attempt_gets_502_without_resubmitting() {
    let wallet = Arc::new(Wallet::default());
    let generator = Arc::new(FirstRunGate::new(false));
    let service = service(wallet.clone(), generator.clone());
    let owner = spawn_generate(&service);
    generator.entered.notified().await;
    let mut waiter = pin!(service.generate(same_request()));
    assert!(
        poll_once(&mut waiter).await.is_pending(),
        "the waiter must park on the owner's attempt"
    );

    owner.abort();
    assert!(owner.await.is_err_and(|error| error.is_cancelled()));
    let waiter = waiter.await;

    assert_eq!(generator.calls(), 1, "the waiter ran a second generation");
    assert!(
        matches!(waiter, Err(ServiceError::UpstreamProvider)),
        "waiter: {waiter:?}"
    );
    assert_eq!(wallet.paid_charges(), 0);
}

#[tokio::test]
async fn a_retry_after_a_finished_failure_runs_a_new_attempt() {
    let wallet = Arc::new(Wallet::default());
    let generator = Arc::new(FirstRunGate::new(true));
    // The first run fails as soon as it starts.
    generator.release.notify_one();
    let service = service(wallet.clone(), generator.clone());

    let first = service.generate(same_request()).await;
    let retry = service.generate(same_request()).await;

    assert!(matches!(first, Err(ServiceError::UpstreamProvider)));
    assert_eq!(
        retry.map(|output| output.image.image_base64),
        Ok("generated-2".to_string())
    );
    assert_eq!(generator.calls(), 2);
}
