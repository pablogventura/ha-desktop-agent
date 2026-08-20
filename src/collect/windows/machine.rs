use crate::collect::iface_filter::skip_lan_iface;
use crate::collect::net::{apply_net, IfaceView};
use crate::collect::proc::{bytes_as_gb, cpu_usage_percent, CpuSample};
use crate::collect::win32_parse::{
    apply_system_power_status, chassis_from_raw_smbios, listen_ports_from_tcp_owner_pid_table,
    SystemPowerStatus,
};
use crate::config::Config;
use crate::entity::{truncate_ha_state, Value};
use crate::snapshot::Snapshot;
use std::net::Ipv4Addr;
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetExtendedTcpTable, GetIfTable2, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
    WlanOpenHandle, WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::Storage::FileSystem::GetSystemFirmwareTable;
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::ProcessStatus::{K32EnumProcesses, K32GetProcessImageFileNameW};
use windows::Win32::System::Shutdown::{
    InitiateShutdownW, LockWorkStation, SHTDN_REASON_MAJOR_OTHER, SHUTDOWN_FORCE_OTHERS,
    SHUTDOWN_POWEROFF, SHUTDOWN_RESTART,
};
use windows::Win32::System::SystemInformation::{
    GetComputerNameW, GetSystemPowerStatus, GetSystemTimes, GetTickCount64, GlobalMemoryStatusEx,
    MEMORYSTATUSEX, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

#[derive(Default)]
pub struct WindowsMachine {
    previous_cpu: Option<CpuSample>,
    lan_prev: Option<(String, u64, u64, Instant)>,
}

impl WindowsMachine {
    pub fn collect(&mut self, config: &Config, snapshot: &mut Snapshot) {
        self.collect_identity(snapshot);
        self.collect_cpu_mem(snapshot);
        snapshot.set("cpu_power", Value::Unavailable);
        snapshot.set("dram_power", Value::Unavailable);
        snapshot.set("cpu_temperature", Value::Unavailable);
        if config.sensors.battery {
            collect_battery(snapshot);
        }
        collect_processes(config, snapshot);
        if config.sensors.disk {
            crate::collect::disk::collect_disk(snapshot);
        }
        let lan = collect_net(config, snapshot);
        if config.sensors.lan_ip {
            sample_lan_rates(&mut self.lan_prev, lan.as_deref(), snapshot);
        }
        if config.sensors.wifi {
            collect_wifi(snapshot);
        }
        if config.sensors.gpu {
            crate::collect::nvidia::collect_nvidia(snapshot);
        }
        super::merge_tick_defaults(snapshot);
    }
}

fn collect_identity(snapshot: &mut Snapshot) {
    snapshot.set("operating_system", Value::Text("Windows".into()));
    if let Some(name) = computer_name() {
        snapshot.set("hostname", Value::Text(name));
    }
    let hours = GetTickCount64() as f64 / 3_600_000.0;
    snapshot.set("uptime", Value::Number(hours));
    match read_chassis() {
        Some(chassis) => snapshot.set("chassis", Value::Text(chassis.to_string())),
        None => snapshot.set("chassis", Value::Unavailable),
    }
}

impl WindowsMachine {
    fn collect_identity(&self, snapshot: &mut Snapshot) {
        collect_identity(snapshot);
    }

    fn collect_cpu_mem(&mut self, snapshot: &mut Snapshot) {
        if let Some(sample) = cpu_sample() {
            if let Some(prev) = self.previous_cpu {
                if let Some(pct) = cpu_usage_percent(prev, sample) {
                    snapshot.set("cpu_usage", Value::Number(pct));
                }
            }
            self.previous_cpu = Some(sample);
        }
        let mut mem = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
            let used = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys);
            snapshot.set("ram_total", Value::Number(bytes_as_gb(mem.ullTotalPhys)));
            snapshot.set(
                "ram_available",
                Value::Number(bytes_as_gb(mem.ullAvailPhys)),
            );
            snapshot.set("ram_used", Value::Number(bytes_as_gb(used)));
            let ram_pct = if mem.ullTotalPhys == 0 {
                0.0
            } else {
                (used as f64 / mem.ullTotalPhys as f64) * 100.0
            };
            snapshot.set("ram_usage", Value::Number(ram_pct));
            snapshot.set(
                "swap_total",
                Value::Number(bytes_as_gb(mem.ullTotalPageFile)),
            );
            let swap_used = mem.ullTotalPageFile.saturating_sub(mem.ullAvailPageFile);
            snapshot.set("swap_used", Value::Number(bytes_as_gb(swap_used)));
            let swap_pct = if mem.ullTotalPageFile == 0 {
                0.0
            } else {
                (swap_used as f64 / mem.ullTotalPageFile as f64) * 100.0
            };
            snapshot.set("swap_usage", Value::Number(swap_pct));
        }
    }
}

fn cpu_sample() -> Option<CpuSample> {
    let mut idle = Default::default();
    let mut kernel = Default::default();
    let mut user = Default::default();
    unsafe {
        GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
    }
    let idle_t = filetime_u64(idle);
    let kernel_t = filetime_u64(kernel);
    let user_t = filetime_u64(user);
    Some(CpuSample {
        idle: idle_t,
        total: kernel_t.saturating_add(user_t),
    })
}

fn filetime_u64(ft: windows::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

fn computer_name() -> Option<String> {
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    unsafe { GetComputerNameW(windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn read_chassis() -> Option<&'static str> {
    let signature = u32::from_le_bytes(*b"RSMB");
    let size = unsafe { GetSystemFirmwareTable(signature, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let written = unsafe { GetSystemFirmwareTable(signature, 0, Some(&mut buf)) };
    if written == 0 {
        return None;
    }
    buf.truncate(written as usize);
    chassis_from_raw_smbios(&buf)
}

fn collect_battery(snapshot: &mut Snapshot) {
    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        snapshot.set("battery_present", Value::Unavailable);
        snapshot.set("ac_power", Value::Unavailable);
        return;
    }
    let parsed = SystemPowerStatus {
        ac_line: status.ACLineStatus,
        battery_flag: status.BatteryFlag,
        battery_percent: status.BatteryLifePercent,
    };
    let chassis = snapshot.get("chassis").and_then(|v| match v {
        Value::Text(text) => Some(text.as_str()),
        _ => None,
    });
    apply_system_power_status(snapshot, parsed, chassis);
}

fn collect_processes(config: &Config, snapshot: &mut Snapshot) {
    let names = process_image_names();
    for monitor in &config.processes {
        let needle = monitor.match_name.to_ascii_lowercase();
        let running = names.iter().any(|name| name.contains(&needle));
        snapshot.set(format!("{}_running", monitor.id), Value::Bool(running));
    }
}

pub fn process_image_names() -> Vec<String> {
    let mut pids = vec![0u32; 4096];
    let mut needed = 0u32;
    unsafe {
        if K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed).is_err() {
            return Vec::new();
        }
    }
    pids.truncate((needed as usize) / 4);
    let mut names = Vec::new();
    for pid in pids {
        if pid == 0 {
            continue;
        }
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            continue;
        };
        let mut buf = [0u16; 260];
        let n = unsafe { K32GetProcessImageFileNameW(handle, &mut buf) };
        let _ = unsafe { CloseHandle(handle) };
        if n == 0 {
            continue;
        }
        let path = String::from_utf16_lossy(&buf[..n as usize]).to_ascii_lowercase();
        if let Some(base) = std::path::Path::new(&path).file_name() {
            names.push(base.to_string_lossy().into_owned());
        }
        names.push(path);
    }
    names
}

fn collect_net(config: &Config, snapshot: &mut Snapshot) -> Option<String> {
    let ifaces = adapter_ifaces();
    let listening = listening_ports();
    let defaults = default_adapter_names(&ifaces);
    let tailscaled = process_image_names()
        .iter()
        .any(|name| name.contains("tailscale"));
    apply_net(config, snapshot, &ifaces, &listening, &defaults, tailscaled)
}

fn adapter_ifaces() -> Vec<IfaceView> {
    let Ok(addrs) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut names: Vec<String> = addrs.iter().map(|iface| iface.name.clone()).collect();
    names.sort();
    names.dedup();
    let mut ifaces = Vec::new();
    for name in names {
        let ipv4: Vec<Ipv4Addr> = addrs
            .iter()
            .filter(|iface| iface.name == name)
            .filter_map(|iface| match iface.ip() {
                std::net::IpAddr::V4(ip)
                    if !ip.is_loopback()
                        && !ip.is_link_local()
                        && !ip.is_unspecified()
                        && !ip.is_multicast() =>
                {
                    Some(ip)
                }
                _ => None,
            })
            .collect();
        let loopback = addrs
            .iter()
            .any(|iface| iface.name == name && iface.is_loopback());
        if loopback {
            continue;
        }
        ifaces.push(IfaceView {
            name,
            ipv4,
            up: true,
        });
    }
    ifaces
}

fn default_adapter_names(ifaces: &[IfaceView]) -> Vec<String> {
    ifaces
        .iter()
        .filter(|iface| iface.up && !skip_lan_iface(&iface.name) && !iface.ipv4.is_empty())
        .map(|iface| iface.name.clone())
        .collect()
}

fn listening_ports() -> std::collections::BTreeSet<u16> {
    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    if size == 0 {
        return Default::default();
    }
    let mut buf = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            Some(buf.as_mut_ptr().cast()),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return Default::default();
    }
    buf.truncate(size as usize);
    listen_ports_from_tcp_owner_pid_table(&buf)
}

fn sample_lan_rates(
    prev: &mut Option<(String, u64, u64, Instant)>,
    iface: Option<&str>,
    snapshot: &mut Snapshot,
) {
    let Some(name) = iface else {
        snapshot.set("lan_rx", Value::Unavailable);
        snapshot.set("lan_tx", Value::Unavailable);
        *prev = None;
        return;
    };
    let Some((rx, tx)) = adapter_octets(name) else {
        snapshot.set("lan_rx", Value::Unavailable);
        snapshot.set("lan_tx", Value::Unavailable);
        return;
    };
    let now = Instant::now();
    if let Some((prev_name, prev_rx, prev_tx, prev_at)) = prev {
        if prev_name == name {
            let elapsed = now.duration_since(*prev_at).as_secs_f64();
            if elapsed > 0.0 {
                let rx_kb = (rx.saturating_sub(*prev_rx) as f64 / elapsed) / 1000.0;
                let tx_kb = (tx.saturating_sub(*prev_tx) as f64 / elapsed) / 1000.0;
                snapshot.set("lan_rx", Value::Number(rx_kb));
                snapshot.set("lan_tx", Value::Number(tx_kb));
            }
        }
    }
    *prev = Some((name.to_string(), rx, tx, now));
}

fn adapter_octets(name: &str) -> Option<(u64, u64)> {
    unsafe {
        let mut table = std::ptr::null_mut();
        GetIfTable2(&mut table).ok()?;
        if table.is_null() {
            return None;
        }
        let info = &*table;
        let mut found = None;
        let target = name.to_ascii_lowercase();
        for i in 0..info.NumEntries {
            let row = &*info.Table.as_ptr().add(i as usize);
            let alias = wide_trim(&row.Alias);
            let descr = wide_trim(&row.Description);
            if alias.eq_ignore_ascii_case(&target) || descr.eq_ignore_ascii_case(&target) {
                found = Some((row.InOctets, row.OutOctets));
                break;
            }
        }
        FreeMibTable(table.cast());
        found
    }
}

fn wide_trim(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn collect_wifi(snapshot: &mut Snapshot) {
    unsafe {
        let mut handle = Default::default();
        let mut version = 0u32;
        if WlanOpenHandle(2, None, &mut version, &mut handle).is_err() {
            snapshot.set("wifi_ssid", Value::Unavailable);
            snapshot.set("wifi_signal", Value::Unavailable);
            return;
        }
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut list).is_err() || list.is_null() {
            let _ = WlanCloseHandle(handle, None);
            snapshot.set("wifi_ssid", Value::Unavailable);
            snapshot.set("wifi_signal", Value::Unavailable);
            return;
        }
        let info_list = &*list;
        let mut found = false;
        if info_list.dwNumberOfItems > 0 {
            let iface = &info_list.InterfaceInfo[0];
            let mut data_size = 0u32;
            let mut opcode = 0u32;
            let mut data = std::ptr::null_mut();
            if WlanQueryInterface(
                handle,
                &iface.InterfaceGuid,
                wlan_intf_opcode_current_connection,
                None,
                &mut data_size,
                &mut data,
                Some(&mut opcode),
            )
            .is_ok()
                && !data.is_null()
            {
                let conn = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                let ssid_len =
                    conn.wlanAssociationAttributes.dot11Ssid.uSSIDLength.min(32) as usize;
                let ssid = String::from_utf8_lossy(
                    &conn.wlanAssociationAttributes.dot11Ssid.ucSSID[..ssid_len],
                )
                .trim()
                .to_string();
                if ssid.is_empty() {
                    snapshot.set("wifi_ssid", Value::Unavailable);
                } else {
                    snapshot.set("wifi_ssid", Value::Text(truncate_ha_state(&ssid)));
                }
                let signal = f64::from(conn.wlanAssociationAttributes.wlanSignalQuality);
                snapshot.set("wifi_signal", Value::Number(signal.min(100.0)));
                found = true;
                WlanFreeMemory(data);
            }
        }
        WlanFreeMemory(list.cast());
        let _ = WlanCloseHandle(handle, None);
        if !found {
            snapshot.set("wifi_ssid", Value::Unavailable);
            snapshot.set("wifi_signal", Value::Unavailable);
        }
    }
}

pub fn power_action(action: &str) -> anyhow::Result<()> {
    match action {
        "lock" => unsafe {
            LockWorkStation().map_err(|err| anyhow::anyhow!("LockWorkStation failed: {err}"))
        },
        "suspend" => {
            unsafe {
                SetSuspendState(false, false, false);
            }
            Ok(())
        }
        "hibernate" => {
            unsafe {
                SetSuspendState(true, false, false);
            }
            Ok(())
        }
        "shutdown" => initiate_shutdown(false),
        "reboot" => initiate_shutdown(true),
        other => anyhow::bail!("unknown action {other}"),
    }
}

fn initiate_shutdown(reboot: bool) -> anyhow::Result<()> {
    let flags = SHUTDOWN_FORCE_OTHERS
        | if reboot {
            SHUTDOWN_RESTART
        } else {
            SHUTDOWN_POWEROFF
        };
    unsafe {
        InitiateShutdownW(
            PCWSTR::null(),
            PCWSTR::null(),
            0,
            flags,
            SHTDN_REASON_MAJOR_OTHER,
        )
        .map_err(|err| anyhow::anyhow!("InitiateShutdownW failed: {err}"))
    }
}
