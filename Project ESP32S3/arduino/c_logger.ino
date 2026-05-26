// LOGGING
#define TRACE 6
#define DEBUG 5
#define INFO 4
#define WARN 3
#define ERROR 2
#define FATAL 1
#define NONE 0

#ifdef LOG_LEVEL
bool _log_newline = true;
#define trace(message)   if ( LOG_LEVEL >= TRACE ) { if (_log_newline) Serial.print("[TRACE] "); Serial.print(message);   _log_newline = false;}
#define traceln(message) if ( LOG_LEVEL >= TRACE ) { if (_log_newline) Serial.print("[TRACE] "); Serial.println(message); _log_newline = true;}
#define debug(message)   if ( LOG_LEVEL >= DEBUG ) { if (_log_newline) Serial.print("[DEBUG] "); Serial.print(message);   _log_newline = false;}
#define debugln(message) if ( LOG_LEVEL >= DEBUG ) { if (_log_newline) Serial.print("[DEBUG] "); Serial.println(message); _log_newline = true;}
#define info(message)    if ( LOG_LEVEL >= INFO  ) { if (_log_newline) Serial.print("[ INFO] "); Serial.print(message);   _log_newline = false;}
#define infoln(message)  if ( LOG_LEVEL >= INFO  ) { if (_log_newline) Serial.print("[ INFO] "); Serial.println(message); _log_newline = true;}
#define warn(message)    if ( LOG_LEVEL >= WARN  ) { if (_log_newline) Serial.print("[ WARN] "); Serial.print(message);   _log_newline = false;}
#define warnln(message)  if ( LOG_LEVEL >= WARN  ) { if (_log_newline) Serial.print("[ WARN] "); Serial.println(message); _log_newline = true;}
#define error(message)   if ( LOG_LEVEL >= ERROR ) { if (_log_newline) Serial.print("[ERROR] "); Serial.print(message);   _log_newline = false;}
#define errorln(message) if ( LOG_LEVEL >= ERROR ) { if (_log_newline) Serial.print("[ERROR] "); Serial.println(message); _log_newline = true;}
#define fatal(message)   if ( LOG_LEVEL >= FATAL ) { if (_log_newline) Serial.print("[FATAL] "); Serial.print(message);   _log_newline = false;}
#define fatalln(message) if ( LOG_LEVEL >= FATAL ) { if (_log_newline) Serial.print("[FATAL] "); Serial.println(message); _log_newline = true;}

#else
#define trace(message)
#define traceln(message)
#define debug(message)
#define debugln(message)
#define info(message)
#define infoln(message)
#define warn(message)
#define warnln(message)
#define error(message)
#define errorln(message)
#define fatal(message)
#define fatalln(message)

#endif

