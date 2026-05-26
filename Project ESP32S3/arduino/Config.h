// COMM BAUDS
#define BAUDS 115200

#define LOGGER_ENABLED            // Comentar para deshabilitar el logger por consola serie

#define LOG_LEVEL TRACE           // nivells en c_logger: TRACE, DEBUG, INFO, WARN, ERROR, FATAL, NONE

// DEVICE
//#define DEVICE_ESP_ID             "54CE0361421"   // ESP32 ID
#define DEVICE_GIIROB_PR2_ID      "00" //"giirobpr2_00"

// WIFI
#define NET_SSID                  "UPV-PSK"
#define NET_PASSWD                "giirob-pr2-2023"

// MQTT
#define MQTT_SERVER_IP            "mqtt.dsic.upv.es"
#define MQTT_SERVER_PORT          1883
#define MQTT_USERNAME             "giirob"    // Descomentar esta línea (y la siguiente) para que se conecte al broker MQTT usando usuario y contraseña
#define MQTT_PASSWORD             "UPV2024"

#define HELLO_TOPIC               "giirob/pr2/devices/hello"    // TODO: topic ejemplo para ejercicio inicial de saludo de los dispositivos



// IO
#define LED_BUILTIN               2


