use crate::config::Config;
use crate::cups::{self, PrinterState};
use crate::discovery;
use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, PartialEq, Clone)]
enum WatchState {
    Online,
    Offline,
    Recovering,
}

/// Main async daemon loop, called by `epson-watchdog run`.
/// Runs forever — launchd will restart on non-zero exit.
pub async fn run() -> Result<()> {
    log::info!("epson-watchdog starting");

    let config = Config::load()?;
    log::info!(
        "Monitoring printer '{}' (mDNS: '{}')",
        config.printer_name,
        config.mdns_instance_name
    );

    let poll = Duration::from_secs(config.poll_interval_secs);
    let enable_delay = Duration::from_secs(config.enable_delay_secs);
    let mdns_timeout = Duration::from_secs(8);

    // Service types to search — pdl-datastream first (confirmed for L3150), ipp as fallback
    let service_types = vec![
        "_pdl-datastream._tcp.local.".to_string(),
        "_ipp._tcp.local.".to_string(),
    ];

    let mut state = WatchState::Offline; // start pessimistic

    loop {
        log::debug!("Watchdog tick — state: {:?}", state);

        // mDNS check runs on a blocking thread (mdns-sd is synchronous)
        let types_clone = service_types.clone();
        let instance_clone = config.mdns_instance_name.clone();
        let printer_online = tokio::task::spawn_blocking(move || {
            let type_refs: Vec<&str> = types_clone.iter().map(String::as_str).collect();
            discovery::is_printer_online(&type_refs, &instance_clone, mdns_timeout)
        })
        .await
        .unwrap_or(false);

        let cups_state = cups::get_printer_state(&config.printer_name)
            .unwrap_or(PrinterState::NotFound);
        let accepting = cups::is_accepting_jobs(&config.printer_name).unwrap_or(false);

        log::info!(
            "mDNS: {} | CUPS: {:?} | accepting: {}",
            if printer_online { "online" } else { "offline" },
            cups_state,
            accepting
        );

        match (&state, printer_online) {
            // Printer visible on network
            (_, true) => {
                let needs_enable = matches!(cups_state, PrinterState::Disabled { .. });
                let needs_accept = !accepting;

                if needs_enable || needs_accept {
                    log::info!(
                        "Printer reappeared — re-enabling CUPS queue (needs_enable={}, needs_accept={})",
                        needs_enable,
                        needs_accept
                    );
                    // Brief pause for CUPS to resolve the dnssd URI
                    sleep(enable_delay).await;

                    if needs_enable {
                        if let Err(e) = cups::enable_printer(&config.printer_name) {
                            log::error!("cupsenable failed: {}", e);
                        } else {
                            log::info!("Queue enabled");
                        }
                    }
                    if needs_accept {
                        if let Err(e) = cups::accept_jobs(&config.printer_name) {
                            log::error!("cupsaccept failed: {}", e);
                        } else {
                            log::info!("Queue accepting jobs");
                        }
                    }
                    state = WatchState::Recovering;
                } else {
                    if state != WatchState::Online {
                        log::info!("Printer fully online and healthy");
                    }
                    state = WatchState::Online;
                }
            }

            // Printer just disappeared
            (WatchState::Online, false) => {
                log::warn!(
                    "Printer '{}' disappeared from mDNS. Jobs will queue until it returns.",
                    config.printer_name
                );
                state = WatchState::Offline;
            }

            // Still offline
            (WatchState::Offline | WatchState::Recovering, false) => {
                log::debug!("Still offline, waiting…");
                state = WatchState::Offline;
            }
        }

        sleep(poll).await;
    }
}
