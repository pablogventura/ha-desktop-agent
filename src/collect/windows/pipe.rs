use super::apply_ipc_snapshot;
use super::session::{handle_rpc, snapshot_message, SessionState};
use crate::config::Config;
use crate::entity::Value;
use crate::ipc::{pipe_name, read_frame, write_frame, IpcMessage};
use crate::transport::resolve_device_id;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::sync::oneshot;
use tracing::{info, warn};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    LocalFree, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

const PIPE_BUFFER: u32 = 64 * 1024;

enum HubRequest {
    Rpc {
        message: IpcMessage,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
}

pub struct SessionHub {
    values: Arc<Mutex<HashMap<String, Value>>>,
    tx: Mutex<Option<mpsc::Sender<HubRequest>>>,
    next_id: AtomicU64,
}

impl SessionHub {
    pub fn new() -> Self {
        Self {
            values: Arc::new(Mutex::new(HashMap::new())),
            tx: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn values(&self) -> Arc<Mutex<HashMap<String, Value>>> {
        self.values.clone()
    }

    pub async fn call(&self, mut message: IpcMessage) -> anyhow::Result<()> {
        if let IpcMessage::Rpc { id, .. } = &mut message {
            *id = self.next_id.fetch_add(1, Ordering::Relaxed);
        }
        let tx = self
            .tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("session helper is not connected"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(HubRequest::Rpc {
            message,
            reply: reply_tx,
        })
        .map_err(|_| anyhow::anyhow!("session helper is not connected"))?;
        match tokio::time::timeout(Duration::from_secs(8), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => anyhow::bail!("session helper dropped rpc"),
            Err(_) => anyhow::bail!("session helper rpc timed out"),
        }
    }
}

struct PipeStream {
    handle: OwnedHandle,
}

impl PipeStream {
    fn as_win_handle(&self) -> HANDLE {
        HANDLE(self.handle.as_raw_handle())
    }

    fn bytes_available(&self) -> u32 {
        let mut avail = 0u32;
        unsafe {
            PeekNamedPipe(self.as_win_handle(), None, 0, None, Some(&mut avail), None).ok();
        }
        avail
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut read = 0u32;
        unsafe {
            ReadFile(self.as_win_handle(), Some(buf), Some(&mut read), None)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
        }
        Ok(read as usize)
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0u32;
        unsafe {
            WriteFile(self.as_win_handle(), Some(buf), Some(&mut written), None)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn spawn_pipe_server(config: Config, hub: Arc<SessionHub>) {
    thread::spawn(move || {
        if let Err(err) = pipe_server_loop(config, hub) {
            warn!("named pipe server exited: {err:#}");
        }
    });
}

fn pipe_server_loop(config: Config, hub: Arc<SessionHub>) -> anyhow::Result<()> {
    let name = pipe_name(&resolve_device_id(&config));
    info!(pipe = %name, "named pipe listening");
    loop {
        let mut stream = match accept_client(&name) {
            Ok(stream) => stream,
            Err(err) => {
                warn!("named pipe accept failed: {err}");
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        info!("session helper connected");
        let (tx, rx) = mpsc::channel::<HubRequest>();
        *hub.tx.lock().unwrap() = Some(tx);
        let mut pending: HashMap<u64, oneshot::Sender<anyhow::Result<()>>> = HashMap::new();
        loop {
            while let Ok(req) = rx.try_recv() {
                match req {
                    HubRequest::Rpc { message, reply } => {
                        if let IpcMessage::Rpc { id, .. } = &message {
                            pending.insert(*id, reply);
                        }
                        if write_frame(&mut stream, &message).is_err() {
                            break;
                        }
                    }
                }
            }
            if stream.bytes_available() >= 4 {
                match read_frame(&mut stream) {
                    Ok(IpcMessage::Snapshot { values }) => {
                        let mut map = hub.values.lock().unwrap();
                        apply_ipc_snapshot(&mut map, values);
                    }
                    Ok(IpcMessage::Result { id, ok, error }) => {
                        if let Some(reply) = pending.remove(&id) {
                            let result = if ok {
                                Ok(())
                            } else {
                                Err(anyhow::anyhow!(error.unwrap_or_else(|| "rpc failed".into())))
                            };
                            let _ = reply.send(result);
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
        *hub.tx.lock().unwrap() = None;
        hub.values.lock().unwrap().clear();
    }
}

fn accept_client(name: &str) -> anyhow::Result<PipeStream> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:(A;;GA;;;SY)(A;;GA;;;IU)"),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )?;
    }
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            Some(&mut sa),
        )
    };
    unsafe {
        let _ = LocalFree(windows::Win32::Foundation::HLOCAL(sd.0 as _));
    }
    if handle.is_invalid() {
        anyhow::bail!("CreateNamedPipeW failed");
    }
    unsafe {
        if let Err(err) = ConnectNamedPipe(handle, None) {
            // ERROR_PIPE_CONNECTED
            if err.code().0 != 535 && err.code().0 != 0x800700E7u32 as i32 {
                anyhow::bail!("ConnectNamedPipe failed: {err}");
            }
        }
    }
    let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    Ok(PipeStream { handle: owned })
}

pub fn run_session_client(config: Config) -> anyhow::Result<()> {
    super::session::init_com().ok();
    let name = pipe_name(&resolve_device_id(&config));
    let state = SessionState::default();
    loop {
        match connect_client(&name) {
            Ok(mut stream) => {
                info!(pipe = %name, "connected to service pipe");
                let tick = Duration::from_millis(config.poll.fast_ms.max(200));
                loop {
                    let msg = snapshot_message(&config, &state);
                    if write_frame(&mut stream, &msg).is_err() {
                        break;
                    }
                    let deadline = std::time::Instant::now() + tick;
                    while std::time::Instant::now() < deadline {
                        if stream.bytes_available() >= 4 {
                            match read_frame(&mut stream) {
                                Ok(msg) => {
                                    let reply = handle_rpc(&state, &msg);
                                    if write_frame(&mut stream, &reply).is_err() {
                                        return Ok(());
                                    }
                                }
                                Err(_) => break,
                            }
                        } else {
                            thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            }
            Err(err) => {
                warn!("waiting for service pipe: {err}");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn connect_client(name: &str) -> anyhow::Result<PipeStream> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    };
    if handle.is_invalid() || handle == INVALID_HANDLE_VALUE {
        anyhow::bail!("CreateFileW pipe failed");
    }
    let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    Ok(PipeStream { handle: owned })
}
