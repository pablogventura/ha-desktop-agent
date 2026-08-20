//! Length-prefixed JSON IPC between the Windows service and session helper.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};

use crate::entity::Value;

pub const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum IpcMessage {
    #[serde(rename = "snapshot")]
    Snapshot { values: HashMap<String, IpcValue> },
    #[serde(rename = "rpc")]
    Rpc {
        id: u64,
        method: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        urgent: bool,
        #[serde(default)]
        on: Option<bool>,
        #[serde(default)]
        delta: Option<i32>,
        #[serde(default)]
        action: Option<String>,
    },
    #[serde(rename = "result")]
    Result {
        id: u64,
        ok: bool,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IpcValue {
    Number(f64),
    Bool(bool),
    Text(String),
    Null,
}

impl From<&Value> for IpcValue {
    fn from(value: &Value) -> Self {
        match value {
            Value::Number(n) => Self::Number(*n),
            Value::Bool(v) => Self::Bool(*v),
            Value::Text(v) => Self::Text(v.clone()),
            Value::Unavailable => Self::Null,
        }
    }
}

impl From<IpcValue> for Value {
    fn from(value: IpcValue) -> Self {
        match value {
            IpcValue::Number(n) => Value::Number(n),
            IpcValue::Bool(v) => Value::Bool(v),
            IpcValue::Text(v) => Value::Text(v),
            IpcValue::Null => Value::Unavailable,
        }
    }
}

pub fn encode_frame(message: &IpcMessage) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(message)?;
    if json.len() > MAX_FRAME_BYTES {
        anyhow::bail!("ipc frame too large");
    }
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

pub fn write_frame<W: Write>(writer: &mut W, message: &IpcMessage) -> anyhow::Result<()> {
    let bytes = encode_frame(message)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read>(reader: &mut R) -> anyhow::Result<IpcMessage> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        anyhow::bail!("invalid ipc frame length {len}");
    }
    let mut json = vec![0u8; len];
    reader.read_exact(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}

pub fn pipe_name(device_id: &str) -> String {
    format!(r"\\.\pipe\ha-desktop-agent-{device_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrips_snapshot_and_rpc() {
        let mut values = HashMap::new();
        values.insert("locked".into(), IpcValue::Bool(false));
        values.insert("idle_time".into(), IpcValue::Number(1.5));
        let msg = IpcMessage::Snapshot { values };
        let encoded = encode_frame(&msg).unwrap();
        let decoded = read_frame(&mut Cursor::new(encoded)).unwrap();
        assert_eq!(msg, decoded);

        let rpc = IpcMessage::Rpc {
            id: 7,
            method: "notify".into(),
            title: Some("Hi".into()),
            body: Some("there".into()),
            urgent: true,
            on: None,
            delta: None,
            action: None,
        };
        let encoded = encode_frame(&rpc).unwrap();
        let decoded = read_frame(&mut Cursor::new(encoded)).unwrap();
        assert_eq!(rpc, decoded);
    }

    #[test]
    fn rejects_oversized_length() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_FRAME_BYTES as u32 + 1).to_le_bytes());
        assert!(read_frame(&mut Cursor::new(buf)).is_err());
    }
}
