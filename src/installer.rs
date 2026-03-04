use crate::config::Config;
use crate::cups;
use crate::discovery;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const PLIST_LABEL: &str = "com.epson.l3150.watchdog";
const SERVICE_TYPES: &[&str] = &["_pdl-datastream._tcp.local.", "_ipp._tcp.local."];

pub fn install(
    printer_name: &str,
    discovery_timeout: u64,
    use_existing: bool,
) -> Result<()> {
    println!("=== Epson L3150 Watchdog — Install ===\n");

    let binary_path = std::env::current_exe()
        .context("Cannot determine current executable path")?
        .to_string_lossy()
        .to_string();
    println!("Binary: {}", binary_path);

    let (mdns_instance_name, mdns_hostname) = if use_existing {
        // Take data from what CUPS already has
        let uri = cups::get_device_uri(printer_name)?.ok_or_else(|| {
            anyhow::anyhow!(
                "Printer '{}' not found in CUPS. Remove --use-existing flag.",
                printer_name
            )
        })?;
        println!("Using existing CUPS printer '{}' ({})\n", printer_name, uri);

        // Derive instance name from dnssd URI: "EPSON%20L3150%20Series" → "EPSON L3150 Series"
        let instance = extract_instance_from_dnssd_uri(&uri)
            .unwrap_or_else(|| "EPSON L3150 Series".to_string());
        // Hostname is unknown without mDNS lookup — leave empty (watchdog doesn't need it)
        (instance, String::new())
    } else {
        println!(
            "Searching for printer on network (timeout {}s)…",
            discovery_timeout
        );

        let info = discovery::discover_printer(
            SERVICE_TYPES,
            "EPSON",
            Duration::from_secs(discovery_timeout),
        )?;

        println!("Found:    {}", info.instance_name);
        println!("Hostname: {}", info.hostname);
        println!("Port:     {}", info.port);
        if let Some(ip) = info.addresses.first() {
            println!("IP:       {}", ip);
        }
        println!();

        // Add to CUPS if not already present
        match cups::get_printer_state(printer_name)? {
            crate::cups::PrinterState::NotFound => {
                let ppd = cups::find_ppd_model("EPSON L3150")?;
                let ppd_model = ppd.as_deref().unwrap_or("everywhere");
                println!("Adding to CUPS (PPD: {})…", ppd_model);
                cups::add_printer(printer_name, &info.dnssd_uri(), ppd_model, &info.instance_name)?;
                cups::set_default_printer(printer_name)?;
                println!("Printer added and set as default\n");
            }
            _ => {
                println!("Printer already in CUPS — skipping lpadmin\n");
            }
        }

        (info.instance_name, info.hostname)
    };

    println!("mDNS instance: {}", mdns_instance_name);

    // Save config
    let plist_path = launchd_plist_path()?;
    let config = Config {
        printer_name: printer_name.to_string(),
        mdns_instance_name: mdns_instance_name.clone(),
        mdns_hostname: mdns_hostname.clone(),
        poll_interval_secs: 30,
        enable_delay_secs: 3,
        plist_path: plist_path.to_string_lossy().to_string(),
    };
    config.save()?;
    println!("Config saved to {}\n", Config::config_path()?.display());

    // Write launchd plist
    let log_dir = log_dir_path()?;
    fs::create_dir_all(&log_dir).context("Cannot create log directory")?;

    let plist_content = generate_plist(&binary_path, &log_dir.to_string_lossy());
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent).context("Cannot create LaunchAgents directory")?;
    }
    fs::write(&plist_path, &plist_content)
        .with_context(|| format!("Cannot write plist to {}", plist_path.display()))?;
    println!("Plist written to {}", plist_path.display());

    // Load with launchctl
    let status = Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .status()
        .context("Failed to run launchctl")?;

    if !status.success() {
        bail!(
            "launchctl load failed. Try manually:\n  launchctl load -w {}",
            plist_path.display()
        );
    }

    println!("\nWatchdog installed and running.");
    println!("Check status:  epson-watchdog status");
    println!(
        "View logs:     tail -f {}/epson-watchdog.log",
        log_dir.display()
    );
    Ok(())
}

pub fn uninstall(remove_printer_queue: bool) -> Result<()> {
    println!("=== Epson L3150 Watchdog — Uninstall ===\n");

    let plist_path = launchd_plist_path()?;

    if plist_path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .status();
        fs::remove_file(&plist_path)
            .with_context(|| format!("Cannot remove {}", plist_path.display()))?;
        println!("Launchd agent unloaded and plist removed");
    } else {
        println!("No plist found at {}", plist_path.display());
    }

    if remove_printer_queue {
        match Config::load() {
            Ok(cfg) => {
                cups::remove_printer(&cfg.printer_name)?;
                println!("CUPS printer '{}' removed", cfg.printer_name);
            }
            Err(_) => {
                println!("No config found — skipping CUPS removal");
            }
        }
    }

    if let Ok(path) = Config::config_path() {
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("Config removed");
        }
    }

    println!("\nUninstall complete.");
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn launchd_plist_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL)))
}

fn log_dir_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Logs")
        .join("epson-watchdog"))
}

fn extract_instance_from_dnssd_uri(uri: &str) -> Option<String> {
    // "dnssd://EPSON%20L3150%20Series._pdl-datastream._tcp.local./?bidi"
    let without_scheme = uri.strip_prefix("dnssd://")?;
    // Find where service type starts ("._")
    let instance_encoded = if let Some(pos) = without_scheme.find("._") {
        &without_scheme[..pos]
    } else {
        without_scheme.split('.').next()?
    };
    Some(instance_encoded.replace("%20", " "))
}

fn generate_plist(binary_path: &str, log_dir: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.epson.l3150.watchdog</string>

    <key>ProgramArguments</key>
    <array>
        <string>{binary_path}</string>
        <string>run</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>ThrottleInterval</key>
    <integer>30</integer>

    <key>StandardOutPath</key>
    <string>{log_dir}/epson-watchdog.log</string>

    <key>StandardErrorPath</key>
    <string>{log_dir}/epson-watchdog.log</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        binary_path = binary_path,
        log_dir = log_dir
    )
}
