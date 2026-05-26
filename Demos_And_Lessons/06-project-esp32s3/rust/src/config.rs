
//Creedenciales para la red 
pub const WIFI_SSID: &str = "HUAWEI-2.4G-pXj3";
pub const WIFI_PASS: &str = "779JAFqe";

//Credenciales para el MQTT
pub const MQTT_URL: &str = "mqtt://broker.hivemq.com:1883";
pub const MQTT_CLIENT_ID: &str = "ESP32_PR2A1";
pub const MQTT_USER: &str = "";
pub const MQTT_PASSWORD: &str = "";

//Topics para suscribirse en el MQTT
pub const MQTT_SUB_TOPICS: &[&str] = &[
	"giirob/pr2-A1/devices/delta/status",
	"giirob/pr2-A1/devices/scada/action",
	"giirob/pr2-A1/devices/amr/status",
	"giirob/pr2-A1/devices/cobot/status",
	"giirob/pr2-A1/system/emergency/action",
	"giirob/pr2-A1/db/pull/response",
];

//Topics para publicar en el MQTT
pub const MQTT_TOPIC_SCADA_STATUS: &str = "giirob/pr2-A1/devices/scada/status";
pub const MQTT_TOPIC_ROBODK_ACTION: &str = "giirob/pr2-A1/devices/robodk/action";
pub const MQTT_TOPIC_AMR_ACTION: &str = "giirob/pr2-A1/devices/amr/action";
pub const MQTT_TOPIC_COBOT_ACTION: &str = "giirob/pr2-A1/devices/cobot/action";
pub const MQTT_TOPIC_DB_PUSH: &str = "giirob/pr2-A1/db/push";
pub const MQTT_TOPIC_DB_PULL: &str = "giirob/pr2-A1/db/pull";
pub const MQTT_SUB_TOPIC_DB_PULL_RESPONSE: &str = "giirob/pr2-A1/db/pull/response";

//Topics para recibir
pub const MQTT_TOPIC_SCADA_ACTION: &str = "giirob/pr2-A1/devices/scada/action";
pub const MQTT_TOPIC_DELTA_STATUS: &str = "giirob/pr2-A1/devices/delta/status";
pub const MQTT_TOPIC_EMERGENCY_ACTION: &str = "giirob/pr2-A1/system/emergency/action";
pub const MQTT_TOPIC_EMERGENCY_STATUS: &str = "giirob/pr2-A1/system/emergency/status";
pub const MQTT_TOPIC_AMR_STATUS: &str = "giirob/pr2-A1/devices/amr/status";
pub const MQTT_TOPIC_COBOT_STATUS: &str = "giirob/pr2-A1/devices/cobot/status";

//Colores válidos
pub const VALID_COLORS: &[&str] = &["red", "green", "yellow", "blue", "white", "orange"];

//Limite de tapas por tolva
pub const AMR_TOLVA_THRESHOLD: u64 = 20;    
//Tiempo estimado de entrega de tapas al AMR en segundos
pub const AMR_ARRIVAL_DELAY_SECS: u64 = 6;
//Nombre de la ubicación del almacén para el AMR
pub const AMR_WAREHOUSE_LOCATION: &str = "cobot_pick";
//Tiempo maximo de espera para que el AMR llegue 
pub const AMR_TIMEOUT_SECS: u64 = 120;
//Capacidad de cajas por pallet
pub const PALLET_CAPACITY: u64 = 6;       
//Tiempo de espera para el cobot 
pub const COBOT_TIMEOUT_SECS: u64 = 60;

