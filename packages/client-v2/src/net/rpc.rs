use crate::net::protocol::RpcResult;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::oneshot;

pub struct RpcManager {
    next_id: AtomicU32,
    pending: Mutex<HashMap<u32, oneshot::Sender<RpcResult>>>,
}

impl RpcManager {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU32::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn alloc_id(&self) -> (u32, oneshot::Receiver<RpcResult>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        (id, rx)
    }

    pub fn fulfill(&self, id: u32, result: RpcResult) {
        if let Some(tx) = self.pending.lock().remove(&id) {
            let _ = tx.send(result);
        }
    }
}
