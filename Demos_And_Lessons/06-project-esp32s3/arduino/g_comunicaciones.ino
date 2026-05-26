void suscribirseATopics() {
  
  // TODO: añadir suscripciones a los topics MQTT ...
  mqtt_subscribe(HELLO_TOPIC);

}

void alRecibirMensajePorTopic(char* topic, String incomingMessage) {

  // TODO: Controlador que gestiona la recepción de datos

  // A partir de aquí debemos gestionar los mensajes
  //  recibidos por los diferentes topics (canales)
  //

  // Test JSON
  JsonDocument doc;
   // Parse the JSON input
  DeserializationError err = deserializeJson(doc, incomingMessage);
  // Parsing succeeded?
  if (err) {
    warn(F("deserializeJson() returned ")); warnln(err.f_str());
    return;
  }

  String msg = doc["message"];
  info("(JSON) Rebut message: "); infoln(msg);

  int lum = doc["luminosidad"];
  info("(JSON) Rebut luminosidad: "); infoln(lum);

  const char* temp = doc["temperatura"];
  info("(JSON) Rebut temperatura: "); infoln(temp);
  // Test JSON




  // If a message is received on the topic ...
  if (strcmp(topic, HELLO_TOPIC) == 0 ) {
    if(msg == "on") {
      infoln("TODO: Encender el led interno");
    }
    else if (msg == "off") {
      infoln("TODO: Apagar el led interno");
    }
    else warnln("**>> Solicitud no reconocida!");
  }

}

void enviarMensajePorTopic(const char* topic, String outgoingMessage) {

  mqtt_publish(topic, outgoingMessage.c_str());

}





