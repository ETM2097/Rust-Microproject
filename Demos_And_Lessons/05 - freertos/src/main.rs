use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    // Necesario para inicializar los drivers de ESP-IDF
    esp_idf_svc::sys::link_patches();

    // Cola circular de capacidad 8 — bloquea al productor si está llena
    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    // Tarea productora — corre en su propio hilo (xTaskCreatePinnedToCore por debajo)
    thread::Builder::new()
        .name("productor".into())
        .stack_size(4096)
        .spawn(move || {
            for i in 0.. {
                println!("[Productor] enviando: {i}");
                tx.send(i).unwrap(); // bloquea si la cola está llena
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
