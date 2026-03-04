use anyhow::{bail, Result};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PrinterInfo {
    /// mDNS instance name, e.g. "EPSON L3150 Series"
    pub instance_name: String,
    /// mDNS hostname, e.g. "EPSON-L3150-Series.local."
    pub hostname: String,
    /// TCP port
    pub port: u16,
    /// Resolved IPv4 addresses
    pub addresses: Vec<IpAddr>,
    /// Service type that matched
    pub service_type: String,
}

impl PrinterInfo {
    /// Returns dnssd:// URI suitable for CUPS lpadmin -v
    pub fn dnssd_uri(&self) -> String {
        let encoded = self.instance_name.replace(' ', "%20");
        format!("dnssd://{}._pdl-datastream._tcp.local./?bidi", encoded)
    }
}

/// Extract IPv4 addresses from ResolvedService.
fn extract_addresses(info: &ResolvedService) -> Vec<IpAddr> {
    let mut addrs: Vec<IpAddr> = info
        .get_addresses_v4()
        .into_iter()
        .map(|a: Ipv4Addr| IpAddr::V4(a))
        .collect();
    addrs.sort();
    addrs
}

/// Extract bare instance name from mDNS fullname.
/// "EPSON L3150 Series._pdl-datastream._tcp.local." → "EPSON L3150 Series"
fn instance_from_fullname(fullname: &str) -> String {
    if let Some(pos) = fullname.find("._") {
        return fullname[..pos].to_string();
    }
    fullname.split('.').next().unwrap_or(fullname).to_string()
}

/// Discover the printer by browsing mDNS for the given service types.
/// `instance_filter` is a substring to match against the fullname (case-insensitive).
pub fn discover_printer(
    service_types: &[&str],
    instance_filter: &str,
    timeout: Duration,
) -> Result<PrinterInfo> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

    let filter_lower = instance_filter.to_lowercase();

    for &service_type in service_types {
        log::info!("Browsing mDNS: {}", service_type);

        let receiver = mdns
            .browse(service_type)
            .map_err(|e| anyhow::anyhow!("Failed to browse {}: {}", service_type, e))?;

        let deadline = Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                log::info!("Timeout browsing {}", service_type);
                break;
            }

            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let fullname = info.get_fullname();
                    log::info!(
                        "Resolved: {} @ {}:{}",
                        fullname,
                        info.get_hostname(),
                        info.get_port()
                    );

                    if fullname.to_lowercase().contains(&filter_lower) {
                        let instance_name = instance_from_fullname(fullname);
                        let hostname = info.get_hostname().to_string();
                        let port = info.get_port();
                        let addresses = extract_addresses(&info);

                        let _ = mdns.stop_browse(service_type);

                        return Ok(PrinterInfo {
                            instance_name,
                            hostname,
                            port,
                            addresses,
                            service_type: service_type.to_string(),
                        });
                    }
                }
                Ok(ServiceEvent::ServiceFound(_, fullname)) => {
                    log::debug!("Found (pending resolve): {}", fullname);
                }
                Ok(other) => {
                    log::debug!("mDNS event: {:?}", other);
                }
                Err(_) => break,
            }
        }

        let _ = mdns.stop_browse(service_type);
    }

    bail!(
        "Printer matching '{}' not found after {}s. Is it powered on and on Wi-Fi?",
        instance_filter,
        timeout.as_secs()
    )
}

/// Lightweight presence check: is the printer currently visible on mDNS?
pub fn is_printer_online(
    service_types: &[&str],
    instance_filter: &str,
    timeout: Duration,
) -> bool {
    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            log::error!("Failed to create mDNS daemon: {}", e);
            return false;
        }
    };

    let filter_lower = instance_filter.to_lowercase();

    for &service_type in service_types {
        let receiver = match mdns.browse(service_type) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let deadline = Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if info.get_fullname().to_lowercase().contains(&filter_lower) {
                        let _ = mdns.stop_browse(service_type);
                        log::info!("Printer online: {}", info.get_fullname());
                        return true;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let _ = mdns.stop_browse(service_type);
    }

    log::debug!("Printer not found in mDNS within {}s", timeout.as_secs());
    false
}
