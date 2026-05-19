use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowsVersion {
    Windows7,
    Windows8,
    Windows81,
    Windows10,
    Windows11,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityLevel {
    Full,
    Compatible,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCapabilities {
    pub windows_version: WindowsVersion,
    pub capability_level: CapabilityLevel,
    pub supports_appcontainer: bool,
    pub supports_wfp: bool,
    pub supports_wireguard_nt: bool,
    pub supports_wintun: bool,
}

impl HostCapabilities {
    pub fn for_version(windows_version: WindowsVersion) -> Self {
        match windows_version {
            WindowsVersion::Windows10 | WindowsVersion::Windows11 => Self {
                windows_version,
                capability_level: CapabilityLevel::Full,
                supports_appcontainer: true,
                supports_wfp: true,
                supports_wireguard_nt: true,
                supports_wintun: true,
            },
            WindowsVersion::Windows8 | WindowsVersion::Windows81 => Self {
                windows_version,
                capability_level: CapabilityLevel::Compatible,
                supports_appcontainer: true,
                supports_wfp: true,
                supports_wireguard_nt: false,
                supports_wintun: true,
            },
            WindowsVersion::Windows7 => Self {
                windows_version,
                capability_level: CapabilityLevel::Degraded,
                supports_appcontainer: false,
                supports_wfp: false,
                supports_wireguard_nt: false,
                supports_wintun: true,
            },
            WindowsVersion::Unknown => Self {
                windows_version,
                capability_level: CapabilityLevel::Degraded,
                supports_appcontainer: false,
                supports_wfp: false,
                supports_wireguard_nt: false,
                supports_wintun: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_has_full_capabilities() {
        let caps = HostCapabilities::for_version(WindowsVersion::Windows11);

        assert_eq!(caps.capability_level, CapabilityLevel::Full);
        assert!(caps.supports_appcontainer);
        assert!(caps.supports_wfp);
        assert!(caps.supports_wireguard_nt);
    }

    #[test]
    fn windows_7_is_degraded() {
        let caps = HostCapabilities::for_version(WindowsVersion::Windows7);

        assert_eq!(caps.capability_level, CapabilityLevel::Degraded);
        assert!(!caps.supports_appcontainer);
        assert!(!caps.supports_wireguard_nt);
    }
}
