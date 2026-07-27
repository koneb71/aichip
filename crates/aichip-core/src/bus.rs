use aichip_shared::EventEnvelope;
use tokio::sync::broadcast;

/// In-process pub/sub for run events. The server's WS hub subscribes for
/// live fan-out; the orchestrator persists every envelope to the `events`
/// table before publishing, so reconnecting clients can replay from the DB.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<EventEnvelope>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(4096);
        Self { tx }
    }

    pub fn publish(&self, envelope: EventEnvelope) {
        // Send fails only when there are no subscribers — fine, the DB
        // already has the event.
        let _ = self.tx.send(envelope);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.tx.subscribe()
    }
}
