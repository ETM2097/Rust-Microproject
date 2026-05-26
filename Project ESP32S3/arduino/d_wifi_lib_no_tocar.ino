#define WIFI_CONNECTION_TIMEOUT_SECONDS 15

// Usamos comunicaciones TLS/SSL si se define el certificado raíz CA
#ifdef SSL_ROOT_CA
  WiFiClientSecure espWifiClient;
#else
  WiFiClient espWifiClient;
#endif

const char* wifiSSID = NET_SSID;
const char* wifiPasswd = NET_PASSWD;

void wifi_loop() {
  if ( !WiFi.isConnected() )
    wifi_reconnect(WIFI_CONNECTION_TIMEOUT_SECONDS);
}

void wifi_connect() {

  delay(10);

  WiFi.mode(WIFI_STA); //Optional
  trace("MAC Address: ");
  traceln(WiFi.macAddress());

#ifdef SSL_ROOT_CA
  // Set Root CA certificate
  espWifiClient.setCACert(SSL_ROOT_CA);
  traceln("Enabling TLS/SSL Communications ...");
#endif

#ifdef SSL_CLIENT_CERTIFICATE
  espWifiClient.setCertificate(SSL_CLIENT_CERTIFICATE);
  espWifiClient.setPrivateKey(SSL_CLIENT_PRIVATE_KEY);
  traceln("Allowing SSL validation with Client Certificate");
#endif

  wifi_reconnect(WIFI_CONNECTION_TIMEOUT_SECONDS);

}

void wifi_reconnect(uint retries) {

  trace("Connecting to ");
  traceln(wifiSSID);
  WiFi.begin(wifiSSID, wifiPasswd);

  uint8_t r = 0;
  while (WiFi.status() != WL_CONNECTED && r<retries ) {
    r++;
    delay(1000);
    trace(".");
  }
  traceln("");

  if ( WiFi.isConnected() ) {
    debugln("-=- Connected to the WiFi network");
    debug("Local ESP32 IP: ");
    debugln(WiFi.localIP().toString());
  } else {
    errorln("-X- Cannot connect to the WiFi newtwork");
  }
}



