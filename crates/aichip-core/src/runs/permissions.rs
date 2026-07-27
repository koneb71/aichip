//! Permission broker: bridges the MCP permission proxy (server side) and the
//! dashboard. A run's engine calls the aichip MCP tool when it needs
//! permission; the broker parks that request, surfaces it as an event, and
//! resolves it when the user clicks Allow/Deny (or the timeout hits → deny).

use aichip_shared::{AichipEvent, EventEnvelope};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::bus::EventBus;

pub struct PendingPermission {
    pub run_id: Uuid,
    pub tool_name: String,
    pub input: serde_json::Value,
    responder: oneshot::Sender<bool>,
}

#[derive(Clone)]
pub struct PermissionBroker {
    bus: EventBus,
    pending: std::sync::Arc<Mutex<HashMap<String, PendingPermission>>>,
    timeout: Duration,
}

impl PermissionBroker {
    pub fn new(bus: EventBus) -> Self {
        Self {
            bus,
            pending: Default::default(),
            timeout: Duration::from_secs(15 * 60),
        }
    }

    /// Called by the MCP proxy. Blocks until the user resolves or timeout.
    pub async fn request(
        &self,
        run_id: Uuid,
        tool_name: String,
        input: serde_json::Value,
    ) -> bool {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            request_id.clone(),
            PendingPermission {
                run_id,
                tool_name: tool_name.clone(),
                input: input.clone(),
                responder: tx,
            },
        );
        self.bus.publish(EventEnvelope {
            run_id,
            step_id: None,
            seq: -1, // permission events are ephemeral; not part of the replay log
            ts: Utc::now(),
            event: AichipEvent::PermissionRequested {
                request_id: request_id.clone(),
                tool_name,
                input,
            },
        });

        let allowed = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(allowed)) => allowed,
            _ => false, // timeout or dropped → deny
        };
        self.pending.lock().unwrap().remove(&request_id);
        self.bus.publish(EventEnvelope {
            run_id,
            step_id: None,
            seq: -1,
            ts: Utc::now(),
            event: AichipEvent::PermissionResolved {
                request_id,
                allowed,
            },
        });
        allowed
    }

    /// Called by the dashboard route when the user clicks Allow/Deny.
    pub fn resolve(&self, request_id: &str, allowed: bool) -> bool {
        if let Some(p) = self.pending.lock().unwrap().remove(request_id) {
            let _ = p.responder.send(allowed);
            true
        } else {
            false
        }
    }

    pub fn pending_for_run(&self, run_id: Uuid) -> Vec<(String, String, serde_json::Value)> {
        self.pending
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p)| p.run_id == run_id)
            .map(|(id, p)| (id.clone(), p.tool_name.clone(), p.input.clone()))
            .collect()
    }
}
