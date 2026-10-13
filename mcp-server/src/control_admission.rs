//! Advisory, bounded identity for polling a native control which was admitted.
//! Disk markers never grant permission to start work or deliver a completion;
//! only the owning desktop's private in-memory admission can do that.

use serde::{Deserialize, Serialize};

pub const NATIVE_CONTROL_ADMISSION_MAX_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeControlAdmission {
    pub version: u8,
    pub session_id: String,
    pub window_id: String,
    pub document_id: String,
    pub process_instance_id: String,
    pub request_id: String,
    pub epoch: u64,
    pub revision: u64,
    pub expires_ms: u64,
    pub admitted_ms: u64,
}

impl NativeControlAdmission {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !super::session::is_valid_session_id(&self.session_id)
            || self.request_id.is_empty()
            || self.request_id.len() > 64
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-')
            || [
                &self.window_id,
                &self.document_id,
                &self.process_instance_id,
            ]
            .iter()
            .any(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
            || self.epoch == 0
            || self.revision == 0
            || self.admitted_ms > self.expires_ms
        {
            return Err("Invalid native control admission identity".into());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        let body = serde_json::to_string(self).map_err(|error| error.to_string())?;
        if body.len() > NATIVE_CONTROL_ADMISSION_MAX_BYTES {
            return Err("Native control admission identity exceeds its byte limit".into());
        }
        Ok(body)
    }

    pub fn decode(body: &str) -> Result<Self, String> {
        if body.len() > NATIVE_CONTROL_ADMISSION_MAX_BYTES {
            return Err("Native control admission identity exceeds its byte limit".into());
        }
        let marker: Self =
            serde_json::from_str(body).map_err(|_| "Invalid native control admission marker")?;
        marker.validate()?;
        Ok(marker)
    }
}
