use anyhow::{Result, bail};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};
use log::info;

pub fn connect(
    modem: Modem,
    ssid: &str,
    password: &str,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let auth_method = if password.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("SSID too long"))?,
        password: password
            .try_into()
            .map_err(|_| anyhow::anyhow!("password too long"))?,
        auth_method,
        ..Default::default()
    }))?;

    wifi.start()?;
    info!("WiFi started, connecting to SSID='{}'...", ssid);

    wifi.connect()?;
    info!("WiFi associated, waiting for DHCP lease...");

    wifi.wait_netif_up()?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    if ip_info.ip.is_unspecified() {
        bail!("DHCP did not assign an IP");
    }
    info!("IP acquired: {}", ip_info.ip);

    Ok(wifi)
}
