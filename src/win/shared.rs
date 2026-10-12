use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use skedaddle::config::Config;

use super::sync::{Cmd, Snapshot};

struct Shared {
    config: Arc<Config>,
    snap: Option<Arc<Snapshot>>,
    tx: Option<Sender<Cmd>>,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared { config: Arc::new(Config::default()), snap: None, tx: None });
}

pub fn config() -> Arc<Config> {
    SHARED.with(|s| s.borrow().config.clone())
}

pub fn snapshot() -> Option<Arc<Snapshot>> {
    SHARED.with(|s| s.borrow().snap.clone())
}

pub fn set_snapshot(snap: Arc<Snapshot>) {
    SHARED.with(|s| {
        let mut s = s.borrow_mut();
        s.config = snap.config.clone();
        s.snap = Some(snap);
    });
}

pub fn set_sender(tx: Sender<Cmd>) {
    SHARED.with(|s| s.borrow_mut().tx = Some(tx));
}

pub fn send(cmd: Cmd) {
    SHARED.with(|s| {
        if let Some(tx) = &s.borrow().tx {
            let _ = tx.send(cmd);
        }
    });
}
