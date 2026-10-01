//! Delivery of feed snapshots to Kotlin's [`FeedObserver`], in order.
//!
//! Snapshots are queued from inside the state actor, so they are in revision
//! order, and one task delivers them one at a time. Each delivery runs on the
//! blocking pool: the observer is foreign code, and however long it takes it
//! must not stall the runtime the rest of the core runs on.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::feed::{FeedObserver, FeedSnapshot};

enum Notice {
    Observer(Option<Arc<dyn FeedObserver>>),
    Snapshot(Box<FeedSnapshot>),
}

#[derive(Clone)]
pub(crate) struct Notifier {
    tx: mpsc::UnboundedSender<Notice>,
}

impl Notifier {
    pub(crate) fn spawn() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Notice>();
        tokio::spawn(async move {
            let mut observer: Option<Arc<dyn FeedObserver>> = None;
            while let Some(notice) = rx.recv().await {
                match notice {
                    Notice::Observer(next) => observer = next,
                    Notice::Snapshot(snapshot) => {
                        let Some(target) = observer.clone() else {
                            continue;
                        };
                        let delivered =
                            tokio::task::spawn_blocking(move || target.feed_changed(*snapshot))
                                .await;
                        if let Err(error) = delivered {
                            tracing::warn!(%error, "feed observer failed");
                        }
                    }
                }
            }
            tracing::debug!("notifier stopped");
        });
        Self { tx }
    }

    pub(crate) fn set_observer(&self, observer: Option<Arc<dyn FeedObserver>>) {
        let _ = self.tx.send(Notice::Observer(observer));
    }

    pub(crate) fn publish(&self, snapshot: FeedSnapshot) {
        let _ = self.tx.send(Notice::Snapshot(Box::new(snapshot)));
    }
}
