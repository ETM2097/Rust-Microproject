# GIIROB — Pruebas de integración del sistema completo

Guía de pruebas manuales end-to-end del firmware ESP32-S3. Las verificaciones de base de datos y el bridge MQTT-DB son responsabilidad del servicio externo (otro repo).

---

## Prerrequisitos

Verificar que todos los servicios están activos antes de comenzar:

| Servicio | Cómo arrancarlo | Señal de OK |
|---|---|---|
| Bridge BD | Ver rama del bridge (servicio externo) | Línea `Bridge listo — esperando mensajes MQTT...` |
| Bridge RoboDK | `python MqttListener.py` (desde la carpeta robodk) | Línea `pick_worker iniciado` |
| RoboDK | Abrir la escena `.rdk` | Delta presente, targets y `cap_template` visibles |
| ESP32 | Flasheado y alimentado | LED de estado, conexión Wi-Fi activa |

> Las verificaciones de PostgreSQL a lo largo de esta guía requieren el bridge activo. El código del bridge y la configuración de la BD están en otra rama del repositorio.

Para monitorizar todos los mensajes MQTT del sistema en tiempo real:
```bash
mosquitto_sub -h broker.hivemq.com -t "giirob/pr2-A1/#" -v
```

Para publicar mensajes de prueba:
```bash
mosquitto_pub -h broker.hivemq.com -t "<topic>" -m '<json>'
```

---

---

## Restricciones del esquema

- `id_caja`, `id_lote`, `proveedor` → `CHAR(5)`, máximo 5 caracteres.
- `color` en `caja` → solo acepta: `RED`, `GREEN`, `BLUE`, `YELLOW`, `ORANGE`, `WHITE`.
- `proveedor` en `proveedor_material` → FK a `proveedor`, debe existir antes.
- `lotes` en `box_completed` → FK a `lote`, el lote debe existir antes.
- `id_operario` en `palet` → FK a `operario`, debe existir antes.

---

## Bloque 1 — Conectividad y estado del sistema

### 1.1 Solicitar estado completo
<!-- Funciona correctamente -->
**Topic:** `giirob/pr2-A1/devices/scada/action`
```json
{ "cmd": "status" }
```

**Verificar en:** `giirob/pr2-A1/devices/scada/status`

Respuesta esperada (campos clave):
```json
{
  "mode": "Manual",
  "id_lote": "",
  "total_processed": 0,
  "auto_target": 0,
  "tolvas": { "TOLVA_1": 0, "TOLVA_2": 0, "TOLVA_3": 0, "TOLVA_4": 0, "TOLVA_5": 0, "TOLVA_6": 0 },
  "pallets": { "PALLET_1": 0, "PALLET_2": 0, "PALLET_3": 0, "PALLET_4": 0, "PALLET_5": 0, "PALLET_6": 0 }
}
```

---

### 1.2 Cambiar a modo Auto
<!-- Funciona correctamente -->
**Topic:** `giirob/pr2-A1/devices/scada/action`
```json
{ "cmd": "set_mode", "mode": "auto" }
```

Solicitar estado de nuevo (prueba 1.1) y verificar que `"mode": "Auto"`.

---

### 1.3 Cambiar a modo Manual
<!-- Funciona correctamente -->
```json
{ "cmd": "set_mode", "mode": "manual" }
```

Verificar `"mode": "Manual"` en la respuesta de estado.

---

### 1.4 Reset del sistema

```json
{ "cmd": "reset" }
```

**Verificar en ESP32:** todos los contadores de tolvas y pallets a 0, `id_lote` limpio, modo sin cambio.

**Verificar en BD:** el bridge Python reinicia `total_tapas_clasificadas = 0` en todos los lotes. **No elimina** cajas, pallets ni material_caja (a diferencia del bridge anterior).

```sql
SELECT id_lote, total_tapas_clasificadas FROM lote;
-- Todos los lotes deben tener total_tapas_clasificadas = 0
```

---

## Bloque 2 — Gestión de lotes (SCADA → Bridge → DB)

> **Nota:** la creación de lotes en la BD es responsabilidad del bridge (otra rama). Desde el ESP32 solo se envía el comando `gen` vía MQTT.

### 2.1 Crear lote con proveedor
<!-- Funciona correctamente -->
**Topic:** `giirob/pr2-A1/devices/scada/action`
```json
{ "cmd": "gen", "id_lote": "L0020", "proveedor": "P0001", "quantity": 200 }
```

**Verificar lote:**
<!-- Funciona correctamente -->
```sql
SELECT id_lote, total_tapas_entrada, fecha_inicio
FROM lote
WHERE id_lote = 'L0020';
-- Debe mostrar L0020, 200, fecha de hoy
```

**Verificar proveedor:**
<!-- Funciona correctamente -->
```sql
SELECT proveedor, lote FROM proveedor_material WHERE lote = 'L0020';
-- Debe mostrar P0001, L0020
```

---

### 2.2 Crear lote sin proveedor
<!-- Funciona correctamente -->
```json
{ "cmd": "gen", "id_lote": "L0021", "quantity": 50 }
```

```sql
SELECT id_lote FROM lote WHERE id_lote = 'L0021';
-- Debe existir

SELECT COUNT(*) FROM proveedor_material WHERE lote = 'L0021';
-- Debe devolver 0
```

---

### 2.3 Lote duplicado — quantity no cambia
<!-- Funciona correctamente -->
Publicar el mismo lote con cantidad distinta:
```json
{ "cmd": "gen", "id_lote": "L0020", "proveedor": "P0002", "quantity": 999 }
```

```sql
SELECT total_tapas_entrada FROM lote WHERE id_lote = 'L0020';
-- Debe seguir siendo 200, no 999
```

---

### 2.4 Alias de campo — `lote` en lugar de `id_lote`
<!-- ⚠️ Solo válido para el bridge — el ESP32 no reconoce la clave `lote` -->
El bridge acepta ambas claves (`id_lote` y `lote`) al suscribirse directamente a `scada/action`.  
**El ESP32 solo lee `id_lote`**: si se usa `lote`, `state.id_lote` queda como `None` y las tapas generadas no quedarán asociadas al lote en el estado interno del ESP32.
```json
{ "cmd": "gen", "lote": "L0022", "quantity": 10 }
```

```sql
SELECT id_lote FROM lote WHERE id_lote = 'L0022';
-- El lote puede existir en BD (creado por el bridge), pero el ESP32 no lo rastreará
```

---

### 2.5 Payload inválido — sin quantity
<!-- Funciona correctamente -->
```json
{ "cmd": "gen", "id_lote": "L0099" }
```

**Verificar en los logs del bridge:** mensaje `gen sin id_lote o quantity inválido`.

```sql
SELECT COUNT(*) FROM lote WHERE id_lote = 'L0099';
-- Debe devolver 0
```

---

## Bloque 3 — Modo Manual (ESP32 + RoboDK)

> ⚠️ **Las pruebas 3.1–3.3 describen el prototipo anterior** que usaba `camera/data`, `delta/action` y comandos PICK explícitos. En el sistema actual el Delta clasifica directamente y reporta vía `delta/status`; el ESP32 no envía PICKs ni procesa `camera/data`. Estas pruebas se mantienen como referencia histórica y deben ser actualizadas cuando se defina el flujo real con RoboDK.

### 3.1 Generar una tapa roja
<!-- Todavía no se sabe -->
Asegurarse de estar en modo Manual (prueba 1.3) y tener el lote activo.

**Topic:** `giirob/pr2-A1/devices/scada/action`
```json
{ "cmd": "gen", "id_lote": "L0020", "color": "red", "quantity": 1 }
```

**Secuencia esperada (sistema actual):**
1. ESP32 publica SPAWN en `robodk/action` → `{"cmd":"spawn","color":"red","id_cap":"C0001","device":"ESP32-S3"}`
2. RoboDK genera la tapa; el Delta la clasifica en TOLVA_1
3. Delta publica en `delta/status` → `{"status":"completed","color":"red","id_cap":"C0001"}`
4. ESP32 incrementa `tolva_counts[0]`, `total_processed`, decrementa `manual_remaining`

---

### 3.2 Color incorrecto en modo Manual (tapa descartada)

> ⚠️ Prueba del prototipo anterior. En el sistema actual no hay validación de color por el ESP32 antes de depositar en tolva; el Delta clasifica cada tapa en la tolva correcta independientemente.

---

### 3.3 Detección con precisión baja (descartada)

> ⚠️ Prueba del prototipo anterior. En el sistema actual no hay campo `precision` ni topic `camera/data`.

---

## Bloque 4 — Modo Auto

### 4.1 Lote pequeño (6 tapas)
<!-- Revisar más tarde cuando esté robodk -->
Cambiar a modo Auto y lanzar lote:
```json
{ "cmd": "gen", "id_lote": "L0020", "quantity": 6 }
```

**Secuencia esperada:**
1. ESP32 genera 6 spawns en colores rotativos: red → green → yellow → blue → white → orange
2. Cada tapa aparece en RoboDK, es detectada por la "cámara" y clasificada por el Delta
3. Al completar las 6 tapas, ESP32 publica en `scada/status`:
```json
{ "event": "batch_complete", "message": "Lote de producción completado", "total": 6, "device": "ESP32-S3" }
```

**Verificar estado tras completar:**
```json
{ "cmd": "status" }
```
`auto_validated` debe ser 6, igual a `auto_target`.

---

### 4.2 Protección contra desbordamiento de tolva

Con el sistema en Auto, cuando `tolva_counts[i] >= 20` (umbral `AMR_TOLVA_THRESHOLD`), el ESP32 deja de enviar SPAWNs para ese color hasta que el AMR vacíe la tolva.

**Verificar en logs del ESP32:** mensaje de rechazo de tapa por tolva llena.
**Verificar en `robodk/action`:** no aparece SPAWN para el color cuya tolva está saturada.

---

## Bloque 5 — Coordinación AMR

### 5.1 AMR enviado a tolva (automático)
<!-- Funciona correctamente -->
Cuando `tolva_counts[i] >= 20` (`AMR_TOLVA_THRESHOLD`), el ESP32 publica automáticamente:

**Verificar en `amr/action`:**
```json
{ "cmd": "goto", "location": "TOLVA_1" }
```

---

### 5.2 AMR llega a la tolva (simulado)
<!-- Funciona correctamente -->
**Topic:** `giirob/pr2-A1/devices/amr/status`
```json
{ "status": "arrived", "location": "TOLVA_1" }
```

**Verificar en logs del ESP32:** registra llegada, inicia espera de 6 s.
**Verificar en estado:** `amr_pending_tolva` y `amr_arrived_tolva` consistentes.

---

### 5.3 AMR enviado a cobot_pick (automático tras 6 s)
<!-- Funciona correctamente -->
Después de 6 s de la llegada, el ESP32 publica:

**Verificar en `amr/action`:**
```json
{ "cmd": "goto", "location": "cobot_pick" }
```

**Verificar en `db/push`:** se publica `box_completed`:
```json
{
  "event": "box_completed",
  "id_caja": "CXXXX",
  "color": "red",
  "codigo_etiqueta": "ETQXXXXXXX",
  "estado": true,
  "lotes": ["L0020"]
}
```

**Verificar en PostgreSQL:**
```sql
SELECT id_caja, color, estado, id_palet FROM caja ORDER BY id_caja DESC LIMIT 5;
```

---

### 5.4 AMR llega a cobot_pick (simulado)
<!-- Funciona correctamente -->
```json
{ "status": "arrived", "location": "cobot_pick" }
```

**Verificar:** `cobot_ready = true` (solicitar estado). El ESP32 enviará orden al Cobot.

---

### 5.5 AMR llega a tolva incorrecta (manejo de error)
<!-- Funciona correctamente -->
Enviar el AMR a `tolva_1` y simular llegada a `tolva_2`:

```json
{ "status": "arrived", "location": "TOLVA_2" }
```

**Verificar en logs del ESP32:** registra el error de tolva inesperada pero no bloquea el flujo.

---

## Bloque 6 — Coordinación Cobot y persistencia de pallet

### 6.1 Cobot paletiza — pallet abierto

Tras `cobot_ready = true`, el ESP32 envía `start` al Cobot. Simular finalización:

**Topic:** `giirob/pr2-A1/devices/cobot/status`
```json
{ "status": "completed", "id_pallet": "P0001" }
```

**Verificar en `db/push`:**
```json
{
  "event": "caja_paletizada",
  "id_caja": "CXXXX",
  "id_palet": "P0001",
  "id_color": "RED",
  "estado": false
}
```

**Verificar en PostgreSQL:**
```sql
SELECT id_palet, id_color, estado, id_operario FROM palet WHERE id_palet = 'P0001';
-- id_operario debe ser NULL (pallet aún abierto)

SELECT id_caja, id_palet FROM caja WHERE id_palet = 'P0001';
```

---

### 6.2 Cierre de pallet (6 cajas — operario asignado)

Tras 6 finalizaciones del Cobot para el mismo pallet, se activa el flujo de cierre:

**Secuencia esperada:**
1. ESP32 publica en `db/pull`:
```json
{ "query": "operarios" }
```
2. Bridge responde en `db/pull/response`:
```json
{ "operarios": [{"id_operario": "OP001", "nombre": "Carlos", "apellido": "Martínez"}, ...] }
```
3. ESP32 elige un operario y publica en `db/push`:
```json
{
  "event": "caja_paletizada",
  "id_caja": "CXXXX",
  "id_palet": "P0001",
  "id_color": "RED",
  "estado": true,
  "id_operario": "OP003"
}
```
4. ESP32 publica en `scada/status`:
```json
{ "event": "pallet_full", "id_palet": "P0001" }
```

**Verificar en PostgreSQL:**
```sql
SELECT id_palet, estado, id_operario FROM palet WHERE id_palet = 'P0001';
-- estado = true, id_operario entre OP001 y OP005

SELECT p.id_palet, p.estado, o.nombre, o.apellido
FROM palet p
JOIN operario o ON o.id_operario = p.id_operario
WHERE p.id_palet = 'P0001';
```

---

### 6.3 Rotación de pallets

Tras cerrar el pallet P0001 (rojo), el ESP32 ejecuta `pallets[0].0 += 6`. La próxima caja **roja** va al pallet P0007; las cajas de otros colores continúan en su pallet propio (P0002 = verde, P0003 = amarillo, etc.).

**Verificar en `cobot/action` para la próxima caja roja:**
```json
{ "cmd": "start", "id_pallet": "P0007", "color": "red", "boxes_stacked": 0 }
```

**Verificar en `cobot/action` para una caja verde (sin cambio):**
```json
{ "cmd": "start", "id_pallet": "P0002", "color": "green", "boxes_stacked": 0 }
```

---

## Bloque 7 — Sistema de emergencia

### 7.1 Activar emergencia por MQTT

**Topic:** `giirob/pr2-A1/system/emergency/action`
```json
{ "cmd": "estop", "source": "SCADA" }
```

**Verificar en `emergency/status`:**
```json
{ "status": "emergency_active", "source": "SCADA" }
```

**Verificar comportamiento:**
- El Python bridge descarta picks nuevos y vacía la cola de picks pendientes
- El ESP32 ignora comandos SCADA `action`
- LED de emergencia encendido (si hay hardware)

---

### 7.2 Comandos SCADA ignorados durante emergencia

Con emergencia activa, publicar:
```json
{ "cmd": "gen", "id_lote": "L0099", "quantity": 1 }
```

**Verificar:** el ESP32 NO procesa el spawn. No aparece ningún mensaje en `robodk/action`.

> El bridge Python SÍ procesa el mensaje (crea el lote en DB), porque el bridge no tiene estado de emergencia — solo el ESP32.

---

### 7.3 Reanudar desde emergencia por MQTT

**Topic:** `giirob/pr2-A1/system/emergency/action`
```json
{ "cmd": "resume", "source": "SCADA" }
```

**Verificar en `emergency/status`:**
```json
{ "status": "emergency_inactive", "source": "SCADA" }
```

El Python bridge vuelve a aceptar picks.

---

### 7.4 Emergencia desde el AMR (colisión)

```json
{ "cmd": "estop", "source": "AMR", "reason": "collision" }
```

**Verificar en `emergency/status`:**
```json
{ "status": "emergency_active", "source": "AMR" }
```

**Verificar:** mismo comportamiento que 7.1. El campo `source` y `reason` son informativos.

---

### 7.5 Pick encolado antes de emergencia (descartado)

1. Encolar un pick manualmente publicando en `delta/action`:
```json
{ "cmd": "pick", "x": 10.0, "y": 5.0, "color": "red", "tolva": "tolva_1", "id_cap": "cap_test" }
```
2. Inmediatamente activar emergencia (prueba 7.1).

**Verificar en logs del Python bridge:** mensaje `Emergencia: N pick(s) descartados de la cola`. El pick NO se ejecuta al reanudar.

---

## Bloque 8 — Bridge de base de datos

> Las pruebas de este bloque son responsabilidad del servicio externo (bridge MQTT-DB). Se documentan aquí solo como referencia de los topics y payloads que publica el ESP32.

### 8.1 Registrar caja sin lotes

**Topic:** `giirob/pr2-A1/db/push`
```json
{
  "event": "BOX_COMPLETED",
  "id_caja": "T0001",
  "color": "green",
  "codigo_etiqueta": "ETQ9990001",
  "estado": true,
  "lotes": []
}
```

```sql
SELECT id_caja, color, codigo_etiqueta, estado, id_palet
FROM caja WHERE id_caja = 'T0001';
-- color=GREEN, id_palet=NULL
```

---

### 8.2 Registrar caja con lote

El lote `L0020` debe existir (prueba 2.1).

```json
{
  "event": "BOX_COMPLETED",
  "id_caja": "T0002",
  "color": "blue",
  "codigo_etiqueta": "ETQ9990002",
  "estado": true,
  "lotes": ["L0020"]
}
```

```sql
SELECT lote, id_caja FROM material_caja WHERE id_caja = 'T0002';
-- Debe mostrar L0020, T0002
```

---

### 8.3 Actualizar caja — color y etiqueta cambian, `id_palet` no

Publicar un segundo `BOX_COMPLETED` para la misma caja:
```json
{
  "event": "BOX_COMPLETED",
  "id_caja": "T0001",
  "color": "yellow",
  "codigo_etiqueta": "ETQ9999999",
  "estado": false,
  "lotes": []
}
```

```sql
SELECT color, codigo_etiqueta, estado, id_palet FROM caja WHERE id_caja = 'T0001';
-- color=YELLOW, etiqueta=ETQ9999999, estado=false, id_palet=NULL (sin cambio)
```

---

### 8.4 Paletizar caja — pallet abierto

```json
{
  "event": "caja_paletizada",
  "id_caja": "T0001",
  "id_palet": "P0001",
  "id_color": "YELLOW",
  "estado": false
}
```

```sql
SELECT id_palet FROM caja WHERE id_caja = 'T0001';                   -- 'P0001'
SELECT id_operario FROM palet WHERE id_palet = 'P0001';              -- NULL
```

---

### 8.5 `id_palet` sobrevive a un segundo `BOX_COMPLETED`

Tras la prueba 8.4, publicar de nuevo:
```json
{
  "event": "BOX_COMPLETED",
  "id_caja": "T0001",
  "color": "orange",
  "codigo_etiqueta": "ETQ9999999",
  "estado": true,
  "lotes": []
}
```

```sql
SELECT id_palet FROM caja WHERE id_caja = 'T0001';
-- DEBE seguir siendo '00010' — el ON CONFLICT no toca id_palet
```

---

### 8.6 Cerrar pallet con operario

```json
{
  "event": "caja_paletizada",
  "id_caja": "T0002",
  "id_palet": "P0001",
  "id_color": "YELLOW",
  "estado": true,
  "id_operario": "OP002"
}
```

```sql
SELECT estado, id_operario FROM palet WHERE id_palet = 'P0001';
-- estado=true, id_operario='OP002'
```

---

### 8.7 Cerrar pallet sin `id_operario` (warning, sin fallo)

```json
{
  "event": "caja_paletizada",
  "id_caja": "T0001",
  "id_palet": 11,
  "codigo_palet": "PALET000011",
  "id_color": "YELLOW",
  "estado": true
}
```

**Verificar en logs del bridge:** mensaje `Palet <id> cerrado sin id_operario`.

```sql
SELECT id_operario FROM palet WHERE id_palet = '00011';
-- NULL — no se asignó
```

---

### 8.8 Consulta de operarios vía `db/pull`

**Topic:** `giirob/pr2-A1/db/pull`
```json
{ "query": "operarios" }
```

**Verificar en `db/pull/response`:**
```json
{
  "operarios": [
    {"id_operario": "OP001", "nombre": "Carlos",   "apellido": "Martínez"},
    {"id_operario": "OP002", "nombre": "Laura",    "apellido": "Sánchez"},
    {"id_operario": "OP003", "nombre": "Miguel",   "apellido": "Torres"},
    {"id_operario": "OP004", "nombre": "Ana",      "apellido": "Romero"},
    {"id_operario": "OP005", "nombre": "Fernando", "apellido": "Jiménez"}
  ]
}
```

---

### 8.9 JSON malformado (sin crash del bridge)

**Topic:** `giirob/pr2-A1/db/push`
```
esto no es json
```

**Verificar en logs del bridge:** mensaje de error JSON. El bridge sigue funcionando y procesa el siguiente mensaje correctamente.

---

## Limpieza de datos de prueba

```sql
UPDATE caja SET id_palet = NULL              WHERE id_caja IN ('T0001', 'T0002');
DELETE FROM palet                            WHERE id_palet IN ('P0001', 'P0002');
DELETE FROM material_caja                    WHERE id_caja IN ('T0001', 'T0002');
DELETE FROM caja                             WHERE id_caja IN ('T0001', 'T0002');
DELETE FROM proveedor_material               WHERE lote IN ('L0020', 'L0021', 'L0022');
DELETE FROM lote                             WHERE id_lote IN ('L0020', 'L0021', 'L0022');
DELETE FROM operario                         WHERE id_operario IN ('OP001', 'OP002', 'OP003', 'OP004', 'OP005');
DELETE FROM proveedor                        WHERE num_proveedor IN ('P0001','P0002','P0003','P0004','P0005');
```
