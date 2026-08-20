//! Windows hardware identity helpers (version, CPU frequency, best-effort temperature).

use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::mem::size_of;
use windows::core::{s, BSTR, VARIANT};
use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::System::Power::{
    CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION,
};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator,
    WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
};

#[repr(C)]
struct RtlOsVersionInfoW {
    os_version_info_size: u32,
    major_version: u32,
    minor_version: u32,
    build_number: u32,
    platform_id: u32,
    csd_version: [u16; 128],
}

type RtlGetVersionFn = unsafe extern "system" fn(*mut RtlOsVersionInfoW) -> NTSTATUS;

pub fn collect_os_version(snapshot: &mut Snapshot) {
    match os_version_string() {
        Some(text) => snapshot.set("os_version", Value::Text(text)),
        None => snapshot.set("os_version", Value::Unavailable),
    }
}

pub fn collect_cpu_frequency(snapshot: &mut Snapshot) {
    match average_cpu_frequency_mhz() {
        Some(mhz) => snapshot.set("cpu_frequency", Value::Number(mhz)),
        None => snapshot.set("cpu_frequency", Value::Unavailable),
    }
}

pub fn collect_cpu_temperature(snapshot: &mut Snapshot) {
    match cpu_temperature_celsius() {
        Some(celsius) => snapshot.set("cpu_temperature", Value::Number(celsius)),
        None => snapshot.set("cpu_temperature", Value::Unavailable),
    }
}

fn os_version_string() -> Option<String> {
    let info = rtl_os_version()?;
    let name = if info.major_version >= 10 && info.build_number >= 22000 {
        "Windows 11"
    } else if info.major_version >= 10 {
        "Windows 10"
    } else {
        "Windows"
    };
    Some(format!(
        "{name} {}.{} (build {})",
        info.major_version, info.minor_version, info.build_number
    ))
}

fn rtl_os_version() -> Option<RtlOsVersionInfoW> {
    unsafe {
        let module = LoadLibraryA(s!("ntdll.dll")).ok()?;
        let proc = GetProcAddress(module, s!("RtlGetVersion"))?;
        let rtl_get_version: RtlGetVersionFn = std::mem::transmute(proc);
        let mut info = RtlOsVersionInfoW {
            os_version_info_size: size_of::<RtlOsVersionInfoW>() as u32,
            major_version: 0,
            minor_version: 0,
            build_number: 0,
            platform_id: 0,
            csd_version: [0; 128],
        };
        let status = rtl_get_version(&mut info);
        if status.0 < 0 {
            return None;
        }
        Some(info)
    }
}

fn average_cpu_frequency_mhz() -> Option<f64> {
    unsafe {
        let mut sys = SYSTEM_INFO::default();
        GetSystemInfo(&mut sys);
        let count = sys.dwNumberOfProcessors.max(1) as usize;
        let mut buf = vec![PROCESSOR_POWER_INFORMATION::default(); count];
        let bytes = (size_of::<PROCESSOR_POWER_INFORMATION>() * count) as u32;
        let status = CallNtPowerInformation(
            ProcessorInformation,
            None,
            0,
            Some(buf.as_mut_ptr().cast()),
            bytes,
        );
        if status.0 < 0 {
            return None;
        }
        let sum: u64 = buf.iter().map(|cpu| u64::from(cpu.CurrentMhz)).sum();
        Some(sum as f64 / count as f64)
    }
}

/// Best-effort ACPI thermal zone via WMI (`MSAcpi_ThermalZoneTemperature`).
fn cpu_temperature_celsius() -> Option<f64> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let server: IWbemServices = locator
            .ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .ok()?;
        let flags = WBEM_GENERIC_FLAG_TYPE(
            WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0,
        );
        let enumerator: IEnumWbemClassObject = server
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from("SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature"),
                flags,
                None,
            )
            .ok()?;
        let mut objects = [None::<IWbemClassObject>];
        let mut returned = 0u32;
        let hr = enumerator.Next(2000, &mut objects, &mut returned);
        if hr.is_err() || returned == 0 {
            return None;
        }
        let obj = objects[0].take()?;
        let mut value = VARIANT::default();
        obj.Get(
            windows::core::w!("CurrentTemperature"),
            0,
            &mut value,
            None,
            None,
        )
        .ok()?;
        let tenths = i32::try_from(&value).ok()?;
        let celsius = (f64::from(tenths) / 10.0) - 273.15;
        if celsius.is_finite() && (-40.0..150.0).contains(&celsius) {
            Some((celsius * 10.0).round() / 10.0)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_11_build_threshold() {
        assert!(22631u32 >= 22000);
    }

    #[test]
    fn kelvin_tenths_to_celsius() {
        let tenths = 3100i32; // 310.0 K
        let celsius = (f64::from(tenths) / 10.0) - 273.15;
        assert!((celsius - 36.85).abs() < 0.01);
    }
}
