use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tracing::{info, warn};

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

pub fn begin_shutdown() {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }

    info!("Shutdown initiated; new requests should be rejected");
}

pub fn track_active_request() {
    ACTIVE_REQUESTS.fetch_add(1, Ordering::SeqCst);
}

pub fn untrack_active_request() {
    let current = ACTIVE_REQUESTS.load(Ordering::SeqCst);
    if current > 0 {
        ACTIVE_REQUESTS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn get_active_request_count() -> usize {
    ACTIVE_REQUESTS.load(Ordering::SeqCst)
}

pub async fn wait_for_active_requests(timeout_ms: u64) {
    if get_active_request_count() == 0 {
        return;
    }

    let start = std::time::Instant::now();
    while get_active_request_count() > 0 {
        if start.elapsed().as_millis() >= timeout_ms as u128 {
            warn!(
                active_requests = get_active_request_count(),
                timeout_ms, "Timed out waiting for active requests"
            );
            return;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};

        if let Ok(mut stream) = signal(SignalKind::terminate()) {
            let _ = stream.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
