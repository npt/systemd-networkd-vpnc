use systemd_networkd_vpnc::*;

fn main() -> Result<(), Error> {
    if matches!(Process::new()?.run()?, Changed::Yes) {
        Networkctl::new().reload()?;
    }
    Ok(())
}
