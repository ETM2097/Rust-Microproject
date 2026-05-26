# 05 — FreeRTOS Desde Rust: Tareas, Colas y el Modelo de Hilos de `std`

Las lecciones anteriores usaron `esp-hal` bare-metal: un único hilo de ejecución,
sin sistema operativo, con `#![no_std]`. Ahora cambiamos de ecosistema.

Esta lección usa **`esp-idf-svc`**, las bindings de Rust sobre ESP-IDF, que sí
ejecuta FreeRTOS por debajo. La novedad: en este ecosistema Rust puede usar la
biblioteca estándar completa (`std`), incluida la API de hilos y canales de
`std::sync::mpsc` — y FreeRTOS gestiona cada hilo como una tarea real.

El programa que escribimos es un **productor–consumidor clásico**: una tarea
genera números, otra los consume más lentamente, y una cola FreeRTOS actúa de
búfer entre ambas.

Necesitas saber:

- Las lecciones 01 a 03 — toolchain ESP32-S3, ownership y borrowing.
- Qué es una tarea de FreeRTOS (`xTaskCreate`, `xQueueSend`, `xQueueReceive`).
- Básicamente, cómo funciona la concurrencia en C con FreeRTOS.

No necesitas saber Rust asíncrono ni `embassy`. Eso es para lecciones posteriores.

---

## 1. Por qué cambiamos de `esp-hal` a `esp-idf-svc`

| | `esp-hal` (lecciones 01–03) | `esp-idf-svc` (esta lección) |
|---|---|---|
| **Sistema base** | Bare-metal, sin RTOS | ESP-IDF + FreeRTOS |
| **`std` disponible** | No (`#![no_std]`) | Sí |
| **Hilos** | No — un hilo, tú controlas el bucle | Sí — `std::thread` sobre tareas FreeRTOS |
| **Colas** | Implementadas a mano o con `embassy` | `std::sync::mpsc` sobre colas FreeRTOS |
| **Wi-Fi, BT, MQTT** | No disponibles | Sí, vía servicios de IDF |
| **Cuando usarlo** | Firmware pequeño, aprendizaje, control preciso | Proyectos con conectividad, equipos ya en IDF |

Aquí nos interesa el modelo de hilos: cómo Rust hace que el patrón
productor–consumidor sea **más seguro** que en C sin añadir coste en tiempo de
ejecución.

---

## 2. El patrón productor–consumidor en C (FreeRTOS clásico)

En C con FreeRTOS, el patrón estándar se ve así:

```c
// Creamos una cola de hasta 8 enteros de 32 bits
QueueHandle_t cola = xQueueCreate(8, sizeof(int32_t));

// Tarea productora
void tarea_productora(void *arg) {
    int32_t i = 0;
    while (1) {
        xQueueSend(cola, &i, portMAX_DELAY);   // bloquea si la cola está llena
        ESP_LOGI(TAG, "[Productor] enviando: %ld", i);
        i++;
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

// Tarea consumidora
void tarea_consumidora(void *arg) {
    int32_t valor;
    while (1) {
        xQueueReceive(cola, &valor, portMAX_DELAY);
        ESP_LOGI(TAG, "[Consumidor] recibió: %ld", valor);
        vTaskDelay(pdMS_TO_TICKS(1200));
    }
}

void app_main(void) {
    xTaskCreatePinnedToCore(tarea_productora, "productor", 4096, NULL, 5, NULL, 0);
    xTaskCreatePinnedToCore(tarea_consumidora, "consumidor", 4096, NULL, 5, NULL, 0);
}
```

Funciona, pero hay varios problemas que el compilador de C no te ayuda a detectar:

- **`cola` es global** — cualquier tarea puede escribir o leer desde ella, aunque
  no deba.
- **`&i` pasa un puntero al stack del hilo** — si la cola tiene semántica de copia,
  bien; si no, el dato puede quedar inválido cuando la tarea productora avanza.
- **Nada impide usar la `QueueHandle_t` equivocada** — es solo un puntero opaco;
  el compilador no sabe qué tipo de dato contiene.

---

## 3. La versión Rust: el mismo patrón, más seguro

Lee el archivo [src/main.rs](src/main.rs):

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();

    // Cola circular de capacidad 8 — bloquea al productor si está llena
    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    // Tarea productora
    thread::Builder::new()
        .name("productor".into())
        .stack_size(4096)
        .spawn(move || {
            for i in 0.. {
                println!("[Productor] enviando: {i}");
                tx.send(i).unwrap();
                thread::sleep(Duration::from_millis(500));
            }
        })
        .unwrap();

    // Tarea consumidora — más lenta, la cola se irá llenando
    thread::Builder::new()
        .name("consumidor".into())
        .stack_size(4096)
        .spawn(move || {
            for valor in rx {
                println!("[Consumidor] recibió: {valor}");
                thread::sleep(Duration::from_millis(1200));
            }
        })
        .unwrap();

    // Mantener el hilo main vivo
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
```

Por debajo, `std::thread::spawn` llama a `xTaskCreatePinnedToCore` y
`mpsc::sync_channel` envuelve una cola FreeRTOS. La diferencia no está en el
rendimiento — está en lo que el compilador garantiza.

---

## 4. Recorrido línea a línea

### `esp_idf_svc::sys::link_patches()`

Esta llamada es **obligatoria** al inicio de `main` cuando se usa `esp-idf-svc`.
Enlaza los parches de ESP-IDF que algunas herramientas de enlace estático
necesitan para funcionar correctamente en Xtensa. Si la omites, el firmware puede
paniquear al arrancar con un error difícil de diagnosticar.

### `mpsc::sync_channel::<i32>(8)`

`sync_channel` crea un canal con capacidad máxima de `8` mensajes (`sync` =
síncrono = el productor bloquea si la cola está llena). Devuelve un par:

- `tx` — el extremo de envío (*transmitter*). Es `Clone`-able; varios
  productores pueden tener su propia copia.
- `rx` — el extremo de recepción (*receiver*). Es único — solo puede haber un
  consumidor. Intentar clonar `rx` es un error de compilación.

El tipo del canal se infiere de la anotación `::<i32>` — solo valores de tipo
`i32` pueden pasar por él. Mandar un `f32` o un puntero sería un error de tipos
en tiempo de compilación.

Compáralo con C:

```c
QueueHandle_t cola = xQueueCreate(8, sizeof(int32_t));
```

En C, la cola es un puntero opaco. Nada impide pasar un `float*` a
`xQueueSend` si el tamaño de elemento coincide con el de un `float`.

### `thread::Builder::new().name(...).stack_size(...).spawn(move || { ... })`

Cada llamada crea una **tarea FreeRTOS**. Los argumentos son:

| Rust | Equivalente FreeRTOS en C |
|---|---|
| `.name("productor")` | El nombre de la tarea (visible en `vTaskList()`) |
| `.stack_size(4096)` | El `usStackDepth` de `xTaskCreate` (en bytes aquí, en palabras en C) |
| `.spawn(move \|\| { ... })` | La función de tarea + captura de variables del entorno |

La palabra clave `move` es la más importante. Hace que el closure **tome
posesión** de todas las variables que usa del entorno exterior:

- `tx` (el extremo de envío) es movido dentro de la tarea productora.
- `rx` (el extremo de recepción) es movido dentro de la tarea consumidora.

Después de esas dos capturas, las variables `tx` y `rx` **ya no existen** en
`main`. Si intentaras usar `tx` desde fuera del closure, el compilador te
detendría con `use of moved value`. Igual que con los pines GPIO en las
lecciones anteriores — la propiedad se transfiere y el acceso se vuelve
exclusivo automáticamente.

### `for valor in rx { ... }`

`rx` implementa el trait `Iterator`. El bucle `for valor in rx` llama a
`rx.recv()` internamente en cada iteración, bloqueando hasta que haya un mensaje
disponible. Cuando el lado `tx` desaparece (por ejemplo, la tarea productora
termina o hace `drop(tx)`), el iterador devuelve `None` y el bucle termina de
forma limpia.

En C, esa lógica de "detectar que el productor terminó" es manual.

### El bucle infinito de `main`

```rust
loop {
    thread::sleep(Duration::from_secs(60));
}
```

En ESP-IDF con `std`, `main()` es una tarea FreeRTOS como las demás. Si `main`
retorna, FreeRTOS termina la tarea y el comportamiento es indefinido (la placa
suele reiniciarse). El bucle infinito mantiene la tarea `main` viva sin consumir
CPU.

---

## 5. Lo que el compilador garantiza automáticamente

| Garantía | C + FreeRTOS | Rust + `std::sync::mpsc` |
|---|---|---|
| Solo un consumidor puede recibir de la cola | Convención, comentarios | Garantizado por tipo: `Receiver<T>` no implementa `Clone` |
| Los mensajes son del tipo correcto | Solo si el tamaño de `sizeof` coincide | Error de tipos en compilación si el tipo no coincide |
| El productor no usa la cola después de pasarla a la tarea | Convención | `move` transfiere el ownership; usar `tx` después es error de compilación |
| El consumidor no escribe en la cola | Convención | `Receiver<T>` solo tiene `.recv()`, no `.send()` |
| Uso de datos después de que la tarea termine | Riesgo de use-after-free | El canal mantiene los datos vivos mientras alguien los necesite |

Ninguna de estas garantías tiene coste en tiempo de ejecución. El compilador las
verifica en `cargo build` y el binario generado es equivalente en velocidad al
código C.

---

## 6. Comportamiento del programa

Al flashear y abrir el monitor serie, verás algo así:

```text
[Productor] enviando: 0
[Consumidor] recibió: 0
[Productor] enviando: 1
[Productor] enviando: 2
[Productor] enviando: 3
[Consumidor] recibió: 1
[Productor] enviando: 4
...
```

El productor envía cada 500 ms. El consumidor procesa cada 1200 ms. La cola
(capacidad 8) actúa de búfer. Cuando la cola se llene, el productor se bloqueará
automáticamente hasta que el consumidor vacíe un hueco — sin que tengas que
escribir ningún código de sincronización explícito.

---

## 7. Comparativa C vs Rust

### C (ESP-IDF + FreeRTOS)

```c
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "esp_log.h"

static const char *TAG = "demo";
static QueueHandle_t cola;

void tarea_productora(void *arg) {
    int32_t i = 0;
    for (;;) {
        xQueueSend(cola, &i, portMAX_DELAY);
        ESP_LOGI(TAG, "[Productor] enviando: %ld", i);
        i++;
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

void tarea_consumidora(void *arg) {
    int32_t valor;
    for (;;) {
        xQueueReceive(cola, &valor, portMAX_DELAY);
        ESP_LOGI(TAG, "[Consumidor] recibió: %ld", valor);
        vTaskDelay(pdMS_TO_TICKS(1200));
    }
}

void app_main(void) {
    cola = xQueueCreate(8, sizeof(int32_t));
    xTaskCreatePinnedToCore(tarea_productora, "productor", 4096, NULL, 5, NULL, 0);
    xTaskCreatePinnedToCore(tarea_consumidora, "consumidor", 4096, NULL, 5, NULL, 0);
}
```

### Rust (`esp-idf-svc` + `std`)

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();

    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    thread::Builder::new()
        .name("productor".into())
        .stack_size(4096)
        .spawn(move || {
            for i in 0.. {
                println!("[Productor] enviando: {i}");
                tx.send(i).unwrap();
                thread::sleep(Duration::from_millis(500));
            }
        })
        .unwrap();

    thread::Builder::new()
        .name("consumidor".into())
        .stack_size(4096)
        .spawn(move || {
            for valor in rx {
                println!("[Consumidor] recibió: {valor}");
                thread::sleep(Duration::from_millis(1200));
            }
        })
        .unwrap();

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
```

### Diferencias relevantes

| Aspecto | C (ESP-IDF) | Rust (`esp-idf-svc`) |
|---|---|---|
| **Tipo de cola** | `QueueHandle_t` (puntero opaco) | `SyncSender<i32>` / `Receiver<i32>` (tipos concretos) |
| **Quién puede enviar** | Cualquiera que tenga el handle | Solo quien tiene `tx` (ownership) |
| **Quién puede recibir** | Cualquiera que tenga el handle | Solo quien tiene `rx` (único por diseño) |
| **Tipo de los mensajes** | Solo verifica el tamaño en bytes | Error de compilación si el tipo no coincide |
| **Creación de tarea** | `xTaskCreatePinnedToCore(fn, name, stack, arg, prio, handle, core)` | `thread::Builder::new().name().stack_size().spawn(closure)` |
| **Variables capturadas por la tarea** | Pasadas como `void*`, requires casting manual | Capturadas por `move`, tipos verificados en compilación |
| **Cola global** | Sí, accesible desde cualquier parte del código | No — `tx` y `rx` se mueven dentro de sus tareas |
| **Fin del canal** | Lógica manual para detectar que el productor terminó | `for valor in rx` termina automáticamente |

---

## 8. Cuándo usar `esp-idf-svc` vs `esp-hal`

Esta lección usa `esp-idf-svc` porque necesitamos FreeRTOS y `std`. Pero la
elección no es trivial:

**Usa `esp-hal` (bare-metal) cuando:**
- No necesitas Wi-Fi, BT, ni servicios de red.
- Quieres el ciclo de compilación más rápido y el control más preciso.
- Estás aprendiendo Rust embebido desde cero (lecciones 01–03).
- El firmware es pequeño y determinista.

**Usa `esp-idf-svc` cuando:**
- Necesitas Wi-Fi, MQTT, TLS, o cualquier servicio de IDF.
- Quieres portar código existente de ESP-IDF a Rust gradualmente.
- Tu equipo ya conoce FreeRTOS y quieres aprovechar ese conocimiento.
- Usas `std::thread`, `std::sync::mpsc`, o cualquier primitiva de `std`.

---

## 9. Compilar, flashear y monitorear

### Prerrequisitos

A diferencia de las lecciones 01–03, este proyecto usa `esp-idf-svc`, que
requiere que ESP-IDF esté instalado en el sistema (CMake, Ninja, Python, el
toolchain Xtensa).

Sigue la guía oficial de instalación si no tienes IDF configurado:
`docs.espressif.com/projects/esp-idf/en/latest/esp32s3/get-started`

También necesitas las variables de entorno del toolchain Rust para Xtensa:

```powershell
# Windows PowerShell (después de instalar espup)
. $env:USERPROFILE\export-esp.ps1
```

### Compilar y flashear

```powershell
cd "05 - freertos"
cargo run --release
```

`cargo run` compila el proyecto, lo flashea y abre el monitor serie.
La configuración del runner está en [.cargo/config.toml](.cargo/config.toml).

### Si hay errores de enlace con ESP-IDF

Si el build falla buscando cabeceras de IDF, asegúrate de que:

1. `IDF_PATH` apunta a tu instalación de ESP-IDF.
2. Has activado el entorno de IDF en la terminal actual.
3. La versión de `esp-idf-svc` en `Cargo.toml` es compatible con tu versión
   de IDF instalada.

---

## 10. Estructura del proyecto

```text
05 - freertos/
├── Cargo.toml          # Dependencias: esp-idf-svc, esp-idf-hal, embuild
├── Cargo.lock
├── rust-toolchain.toml # Fija el toolchain `esp` de Xtensa
├── build.rs            # Ejecuta embuild para configurar ESP-IDF
├── .gitignore
└── src/
    └── main.rs         # Productor–consumidor con std::thread y mpsc
```

`build.rs` usa `embuild` para localizar la instalación de ESP-IDF, generar los
bindings de C necesarios y configurar las variables de entorno de enlace. Es
estándar en proyectos `esp-idf-svc` — no necesitas modificarlo.

---

## 11. Dónde ir después

- Cambia la capacidad del canal de `8` a `1` y observa cómo el productor
  bloquea mucho más seguido — la cola ya no puede absorber la diferencia de
  velocidad.
- Añade un segundo productor: crea otro `tx` con `tx.clone()` y lánzalo en
  un tercer hilo. El compilador acepta esto porque `SyncSender<T>` implementa
  `Clone` — pero `Receiver<T>` no, así que el consumidor sigue siendo único.
- Lee el capítulo **"Fearless Concurrency"** de *The Rust Programming Language*
  (`rust-lang.org/book`, capítulo 16). Usa `String` en lugar de GPIO o colas,
  pero los principios de `Send`, `Sync`, `move` y `mpsc` son exactamente los
  mismos que viste aquí.
- Si necesitas estado compartido (no canales) entre tareas, el siguiente paso
  es `Arc<Mutex<T>>` — la forma segura de compartir datos mutables entre hilos
  sin data races.
