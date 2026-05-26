use anyhow::{Result, bail};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_svc::io::Write;
use esp_idf_svc::http::client::{Configuration, EspHttpConnection};
use log::{info, warn};

pub struct DataUploader {
    client: Client<EspHttpConnection>,
    url: String,
}

impl DataUploader {
    pub fn new(url: &str) -> Result<Self> {
        let connection = EspHttpConnection::new(&Configuration {
            timeout: Some(core::time::Duration::from_secs(5)),
            ..Default::default()
        })?;
        Ok(Self {
            client: Client::wrap(connection),
            url: url.to_string(),
        })
    }

    pub fn post_value(&mut self, value: i32) -> Result<()> {
        let payload = format!("{{\"value\":{}}}", value);
        let payload_bytes = payload.as_bytes();
        let content_length = format!("{}", payload_bytes.len());
        let headers = [
            ("Content-Type", "application/json"),
            ("Content-Length", content_length.as_str()),
        ];

        let mut request = self
            .client
            .request(Method::Post, &self.url, &headers)?;
        request.write_all(payload_bytes)?;
        request.flush()?;

        let response = request.submit()?;
        let status = response.status();

        if !(200..300).contains(&status) {
            warn!("server responded with HTTP {}", status);
            bail!("HTTP {}", status);
        }

        info!("POST {} -> HTTP {}", payload, status);
        Ok(())
    }
}
