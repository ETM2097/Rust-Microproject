# Entorno Rust + ESP32-S3 (Windows, Linux, macOS)

Guía para configurar el entorno y compilar/flashear el proyecto del firmware ESP32-S3.

> El proyecto usa `cargo run` con `espflash` — no usa `idf.py`.
> El directorio del firmware es la raíz del repo (`c:\p\c`), no una subcarpeta.

---

## Requisitos comunes

- Git
- Cable USB de datos (no solo de carga)
- Drivers USB-Serial instalados según la placa:
  - CP210x: https://www.silabs.com/developer-tools/usb-to-uart-bridge-vcp-drivers
  - CH340: https://www.wch-ic.com/downloads/CH341SER_ZIP.html

---

## Windows

### 1. Instalar Rust

```powershell
# Descarga e instala rustup desde https://rustup.rs
# Luego verifica:
rustc --version
```

### 2. Instalar toolchain Xtensa (ESP32-S3)

```powershell
cargo install espup
espup install
# Reinicia PowerShell después para cargar las variables de entorno
```

### 3. Instalar espflash

```powershell
cargo install espflash
```

### 4. Clonar y compilar

```powershell
git clone <repo>
cd c
cargo build
```

### 5. Compilar y flashear

```powershell
# Verifica el puerto COM en Administrador de dispositivos (ej: COM4)
cargo run
# espflash detecta el puerto automáticamente o muestra un menú de selección
```

Si espflash no detecta el puerto:

```powershell
espflash flash --monitor --port COM4 target\xtensa-esp32s3-espidf\debug\prueba2
```

---

## Linux

### 1. Instalar Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Reinicia la terminal, luego:
rustc --version
```

### 2. Instalar toolchain Xtensa

```bash
cargo install espup
espup install
source ~/export-esp.sh   # añadir al .bashrc o .zshrc para no repetirlo
```

### 3. Instalar espflash

```bash
cargo install espflash
```

### 4. Permisos de puerto serie

```bash
sudo usermod -aG dialout $USER
# Cierra sesión y vuelve a entrar para que el cambio aplique
```

### 5. Compilar y flashear

```bash
git clone <repo>
cd c
cargo run
# El puerto se detecta automáticamente (/dev/ttyUSB0 o /dev/ttyACM0)
```

Si espflash no detecta el puerto:

```bash
espflash flash --monitor --port /dev/ttyUSB0 target/xtensa-esp32s3-espidf/debug/prueba2
```

---

## macOS

### 1. Instalar Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustc --version
```

### 2. Instalar toolchain Xtensa

```bash
cargo install espup
espup install
source ~/export-esp.sh
```

### 3. Instalar espflash y ldproxy

```bash
cargo install espflash
cargo install ldproxy
```

> `ldproxy` es el wrapper de linker requerido por `esp-idf-svc`. Sin él, `cargo run` falla con `linker ldproxy not found`.

### 4. Compilar y flashear

```bash
git clone <repo>
cd c
source ~/export-esp.sh   # obligatorio antes de cada cargo run en terminal nueva
cargo run
# Puerto típico: /dev/tty.usbserial-XXXX o /dev/tty.usbmodem-XXXX
```


---

## Configuración antes de flashear

Edita `src/config.rs` con las credenciales de tu red y broker MQTT:

```rust
pub const WIFI_SSID: &str = "tu_red";
pub const WIFI_PASS: &str = "tu_contraseña";
pub const MQTT_URL:  &str = "mqtt://broker.hivemq.com:1883";
```

---

## Comandos útiles

| Comando | Descripción |
|---|---|
| `cargo build` | Compila sin flashear |
| `cargo check` | Verifica errores más rápido que build |
| `cargo run` | Compila, flashea y abre monitor serie |
| `espflash monitor` | Abre solo el monitor sin flashear |
| `espflash erase-flash --port COMx` | Borra la flash completa del ESP32 |

---

## Solución de problemas frecuentes

**Error: `toolchain 'esp' is not installed`**
```bash
espup install
source ~/export-esp.sh   # Linux/macOS
# En Windows: reinicia PowerShell
```

**Error al flashear: puerto ocupado**
- Cierra cualquier otro monitor serie (Arduino IDE, PuTTY, etc.)
- En Linux: verifica que el usuario está en el grupo `dialout`

**El ESP32 no aparece como puerto COM/tty**
- Prueba otro cable USB (muchos cables solo cargan, no transmiten datos)
- Instala los drivers CP210x o CH340 según el chip del adaptador de tu placa

**`cargo build` falla con error de linker o clang**
- Asegúrate de haber ejecutado `espup install` y cargado el entorno (`source ~/export-esp.sh`)
- En Windows, verifica que las variables de entorno se cargaron (reinicia PowerShell)

**Error: `linker ldproxy not found`**
```bash
cargo install ldproxy
```
Ocurre en macOS y Linux cuando `ldproxy` no está instalado. Es el wrapper de linker que requiere `esp-idf-svc` y debe instalarse manualmente.
