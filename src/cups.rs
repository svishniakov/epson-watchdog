use anyhow::{bail, Context, Result};
use std::process::Command;

#[derive(Debug, PartialEq, Clone)]
pub enum PrinterState {
    EnabledIdle,
    EnabledPrinting,
    Disabled { reason: String },
    NotFound,
}

/// Parse `lpstat -p <name>` output.
fn parse_lpstat_output(output: &str, printer_name: &str) -> PrinterState {
    let target = format!("printer {}", printer_name);
    for line in output.lines() {
        if !line.starts_with(&target) {
            continue;
        }
        if line.contains(" disabled ") || line.contains("is stopped") {
            let reason = output
                .lines()
                .skip_while(|l| !l.contains(" disabled ") && !l.contains("is stopped"))
                .nth(1)
                .and_then(|l| l.strip_prefix('\t'))
                .unwrap_or("reason unknown")
                .to_string();
            return PrinterState::Disabled { reason };
        }
        if line.contains("is printing") {
            return PrinterState::EnabledPrinting;
        }
        // "is idle" or any other enabled state
        return PrinterState::EnabledIdle;
    }
    PrinterState::NotFound
}

pub fn get_printer_state(printer_name: &str) -> Result<PrinterState> {
    let output = Command::new("lpstat")
        .args(["-p", printer_name])
        .output()
        .context("Failed to run lpstat")?;

    if !output.status.success() {
        return Ok(PrinterState::NotFound);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(parse_lpstat_output(&stdout, printer_name))
}

pub fn is_accepting_jobs(printer_name: &str) -> Result<bool> {
    let output = Command::new("lpstat")
        .args(["-a", printer_name])
        .output()
        .context("Failed to run lpstat -a")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("accepting requests"))
}

pub fn enable_printer(printer_name: &str) -> Result<()> {
    log::info!("Running: cupsenable {}", printer_name);
    let output = Command::new("cupsenable")
        .arg(printer_name)
        .output()
        .context("Failed to run cupsenable")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cupsenable failed: {}", stderr);
    }
    Ok(())
}

pub fn accept_jobs(printer_name: &str) -> Result<()> {
    log::info!("Running: cupsaccept {}", printer_name);
    let output = Command::new("cupsaccept")
        .arg(printer_name)
        .output()
        .context("Failed to run cupsaccept")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cupsaccept failed: {}", stderr);
    }
    Ok(())
}

pub fn add_printer(
    printer_name: &str,
    device_uri: &str,
    ppd_model: &str,
    display_name: &str,
) -> Result<()> {
    log::info!("lpadmin: adding {} with URI {}", printer_name, device_uri);
    let output = Command::new("lpadmin")
        .args([
            "-p", printer_name,
            "-E",
            "-v", device_uri,
            "-m", ppd_model,
            "-D", display_name,
        ])
        .output()
        .context("Failed to run lpadmin")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("lpadmin add failed: {}", stderr);
    }
    Ok(())
}

pub fn remove_printer(printer_name: &str) -> Result<()> {
    log::info!("lpadmin: removing {}", printer_name);
    let output = Command::new("lpadmin")
        .args(["-x", printer_name])
        .output()
        .context("Failed to run lpadmin -x")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("lpadmin remove failed: {}", stderr);
    }
    Ok(())
}

pub fn set_default_printer(printer_name: &str) -> Result<()> {
    let output = Command::new("lpadmin")
        .args(["-d", printer_name])
        .output()
        .context("Failed to run lpadmin -d")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("lpadmin set-default failed: {}", stderr);
    }
    Ok(())
}

pub fn get_device_uri(printer_name: &str) -> Result<Option<String>> {
    let output = Command::new("lpstat")
        .args(["-v", printer_name])
        .output()
        .context("Failed to run lpstat -v")?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with(&format!("device for {}:", printer_name)) {
            if let Some(uri) = line.splitn(2, ": ").nth(1) {
                return Ok(Some(uri.trim().to_string()));
            }
        }
    }
    Ok(None)
}

/// Find a PPD model string for lpadmin -m.
/// Returns None if not found (caller should use "everywhere" as fallback).
pub fn find_ppd_model(make_model: &str) -> Result<Option<String>> {
    let output = Command::new("lpinfo")
        .args(["--make-and-model", make_model, "-m"])
        .output()
        .context("Failed to run lpinfo")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let ppd_id = parts[0].trim();
            if ppd_id != "everywhere" && !ppd_id.is_empty() {
                return Ok(Some(ppd_id.to_string()));
            }
        }
    }
    Ok(None)
}

pub fn print_status() -> Result<()> {
    let output = Command::new("lpstat")
        .args(["-p", "-d", "-v", "-a"])
        .output()
        .context("Failed to run lpstat")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));

    match crate::config::Config::load() {
        Ok(cfg) => {
            println!("Watchdog config:");
            println!("  printer_name:       {}", cfg.printer_name);
            println!("  mdns_instance_name: {}", cfg.mdns_instance_name);
            println!("  mdns_hostname:      {}", cfg.mdns_hostname);
            println!("  poll_interval_secs: {}", cfg.poll_interval_secs);
            println!("  plist_path:         {}", cfg.plist_path);
        }
        Err(e) => {
            println!("Watchdog config: not found ({})", e);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enabled_idle() {
        let output = "printer EPSON_L3150_Series is idle.  enabled since Wed Mar  4 10:26:09 2026\n";
        assert_eq!(
            parse_lpstat_output(output, "EPSON_L3150_Series"),
            PrinterState::EnabledIdle
        );
    }

    #[test]
    fn test_parse_disabled() {
        let output =
            "printer EPSON_L3150_Series disabled since Wed Mar  4 10:00:00 2026 -\n\treason unknown\n";
        let state = parse_lpstat_output(output, "EPSON_L3150_Series");
        assert!(matches!(state, PrinterState::Disabled { .. }));
    }

    #[test]
    fn test_parse_printing() {
        let output = "printer EPSON_L3150_Series is printing.  enabled since Wed Mar  4 10:00:00 2026\n";
        assert_eq!(
            parse_lpstat_output(output, "EPSON_L3150_Series"),
            PrinterState::EnabledPrinting
        );
    }

    #[test]
    fn test_parse_not_found() {
        let output = "printer OTHER_PRINTER is idle.\n";
        assert_eq!(
            parse_lpstat_output(output, "EPSON_L3150_Series"),
            PrinterState::NotFound
        );
    }
}
