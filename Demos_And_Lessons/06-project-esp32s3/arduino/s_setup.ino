void on_setup() {

    // initialize digital pin LED_BUILTIN as an output.
    pinMode(LED_BUILTIN, OUTPUT);

    setInternalLed(0);

    String hello_msg = String("Hola Mundo! Desde dispositivo ") + deviceID;

    // Test JSON
    JsonDocument doc;
    doc["message"] = hello_msg;
    doc["luminosidad"] = 450;
    doc["temperatura"] = 21.5;
    String hello_msg_json;
    serializeJson(doc, hello_msg_json);
    enviarMensajePorTopic(HELLO_TOPIC, hello_msg_json);


}

