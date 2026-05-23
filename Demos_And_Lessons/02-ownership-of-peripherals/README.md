# 02 — Ownership of Peripherals: The Bug C Lets Through

In lesson 01 you blinked an LED. The Rust version looked fancier (typed pins,
moved values, `#![no_std]`), but at a glance the payoff was unclear — both
programs blink, both are short, both compile.

This lesson is where the payoff actually shows up. We will write the **same
bug** in both languages and watch the C compiler accept it silently while the
Rust compiler refuses to build. The bug is a classic embedded footgun:
**two unrelated modules grabbing the same GPIO pin.**

You already know:

- Basic C/C++ and what a GPIO is.
- How to compile and flash an ESP32-S3 (see [lesson 01](../01-setup-and-blinky/README.md)).
- A bit of Rust syntax from lesson 01 — `#![no_std]`, `#[esp_hal::main]`,
  `Output::new(...)`.

You do **not** need to know what "ownership" means yet. That is what this
lesson is about.

---

## 1. The bug we want to catch

Imagine a real project: someone writes a small `led` module that owns
GPIO2. Later, someone else (or the same person, three months later) writes a
`button` module — and accidentally also picks GPIO2. Maybe the schematic
changed. Maybe it was copy-paste. Either way, both modules call
`gpio_set_direction(...)` on the same pin during init.

What happens?

- The pin is whatever the **last** `init()` configured it as.
- The LED stops working, or the button reads garbage, or both.
- Nothing in the build output tells you anything is wrong.
- You spend an afternoon with a logic analyzer.

This is the kind of bug Rust's type system was built to make impossible.

---

## 2. The C version: silent breakage

Look at [c/main/main.c](c/main/main.c):

```c
void app_main(void)
{
    /* The bug: both init() calls touch GPIO2. The last one is who takes it.
       The compiler is silent about it. */
    led_init();
    button_init();

    while (1)
    {
        led_toggle();
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}
```

Each module hides its pin choice behind a `#define`:

```c
// c/main/led.c
#define LED_GPIO     GPIO_NUM_2

void led_init(void)
{
    gpio_reset_pin(LED_GPIO);
    gpio_set_direction(LED_GPIO, GPIO_MODE_OUTPUT);
}
```

```c
// c/main/button.c
#define BUTTON_GPIO  GPIO_NUM_2   /* same pin — by accident */

void button_init(void)
{
    gpio_reset_pin(BUTTON_GPIO);
    gpio_set_direction(BUTTON_GPIO, GPIO_MODE_INPUT);
    gpio_set_pull_mode(BUTTON_GPIO, GPIO_PULLUP_ONLY);
}
```

### Why doesn't the compiler complain?

Because as far as C is concerned, `GPIO_NUM_2` is just the integer `2`. It
lives in an `enum`. You can copy it, pass it around, store it in a struct,
or compute it. The GPIO driver functions take an integer and poke a
register. There is no notion of "who owns pin 2." Two different `.c` files
each saying `gpio_set_direction(2, ...)` is perfectly valid C — even
though, on real hardware, the second call **silently undoes** the first.

You only find out at runtime, by squinting at a logic analyzer or noticing
the LED has stopped blinking.

This is the same family of bug as a double-free or two threads writing the
same variable without a lock: **shared mutable state with no owner.**

---

## 3. The Rust version: the compiler refuses

Open [rust/src/bin/broken.rs](rust/src/bin/broken.rs). It encodes the exact
same mistake:

```rust
#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // "LED module" takes ownership of GPIO2.
    let mut _led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );

    // "Button module" tries to take GPIO2 too. ERROR: value already moved.
    let _button = Input::new(
        peripherals.GPIO2,
        InputConfig::default().with_pull(Pull::Up),
    );

    loop {}
}
```

Try to build it:

```bash
cd rust/
cargo build --release --bin broken
```

You get a compile error, not a runtime bug:

```text
error[E0382]: use of moved value: `peripherals.GPIO2`
  --> src/bin/broken.rs:33:9
   |
26 |         peripherals.GPIO2,
   |         ----------------- value moved here
...
33 |         peripherals.GPIO2,
   |         ^^^^^^^^^^^^^^^^^ value used here after move
   |
   = note: move occurs because `peripherals.GPIO2` has type `GpioPin<2>`,
           which does not implement the `Copy` trait
```

The firmware does not flash. The bug does not ship. Read the error message
once more — it tells you *exactly* what is wrong and where.

### And the correct version

[rust/src/bin/ok.rs](rust/src/bin/ok.rs) is the same project, but only the
LED module exists. GPIO2 is moved into `Output::new` exactly once and
nothing else can touch it:

```rust
let peripherals = esp_hal::init(esp_hal::Config::default());

let mut led = Output::new(
    peripherals.GPIO2,
    Level::Low,
    OutputConfig::default(),
);
let delay = Delay::new();

loop {
    led.toggle();
    delay.delay_millis(500);
}
```

```bash
cargo run --release --bin ok
```

Builds, flashes, blinks. No drama.

---

## 4. So… what is "ownership"?

Ownership is the rule the Rust compiler enforces to make the bug above
impossible. It boils down to three sentences:

1. Every value has exactly **one owner** (the variable that holds it).
2. When you pass a value into a function or assign it to another variable,
   ownership **moves**. The old binding is gone — using it again is a
   compile error.
3. Some "cheap" values (integers, booleans, raw pointers, …) opt out of
   this by implementing the `Copy` trait. They are duplicated on use,
   exactly like in C.

That's it. There is no garbage collector, no reference counting, no runtime
check. The compiler reads the code, builds a graph of who owns what, and
rejects programs where a value is used after it was given away.

### Why this catches the GPIO bug

`peripherals.GPIO2` has type `GpioPin<2>`. The HAL author deliberately did
**not** implement `Copy` for it. So:

- The first `Output::new(peripherals.GPIO2, ...)` *moves* the pin into the
  LED driver. From the compiler's point of view, `peripherals.GPIO2` no
  longer exists.
- The second `Input::new(peripherals.GPIO2, ...)` tries to use a name that
  no longer refers to anything. Compile error.

The pin's identity is enforced **by the type system**, not by convention.
You literally cannot write the bug.

### Compared to C's "ownership"

C has ownership too — but it is purely a *convention* between humans.
A comment that says `// caller must free` or `// only call from task X`
is "ownership documentation." Nothing checks it. The compiler treats
`int*`, `GPIO_NUM_2`, and `malloc`-allocated buffers all the same: bags
of bytes.

| Concept              | C                                              | Rust                                                   |
|----------------------|------------------------------------------------|--------------------------------------------------------|
| Who owns this value? | Convention, comments, code review.             | Encoded in the type and checked by the compiler.       |
| Use-after-free       | Runtime crash (if you're lucky).               | Compile error.                                         |
| Double "init" of a pin | Silent — last write wins.                    | Compile error (`use of moved value`).                  |
| Aliasing             | Anywhere can hold a `GPIO_NUM_x`.              | Only one place can hold a `GpioPin<N>` at a time.      |
| Cost at runtime      | None — but you pay in bugs.                    | None — ownership is purely a compile-time concept.     |

---

## 5. Walk-through of the Rust code

The two binaries are tiny. Let's annotate the parts that matter.

### `ok.rs`

```rust
#![no_std]
#![no_main]
```

Bare-metal crate attributes (covered in lesson 01): no standard library,
no C-style main wired up by a hosted runtime.

```rust
use esp_backtrace as _;
```

Pulls in the panic / exception handler for side effects only.

```rust
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
```

The HAL pieces we need: a blocking delay, the `Output` driver, the
`Level` enum (`High` / `Low`), and `OutputConfig` (drive strength,
pull resistors, push-pull vs open-drain).

```rust
#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
```

`esp_hal::init` is the singleton entry point. It is callable at most once
and returns a struct that *owns every peripheral on the chip* — every
GPIO, every UART, every timer. Each field has a unique type, so the
compiler can track who took what.

```rust
    let mut led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );
```

The line where ownership matters. `peripherals.GPIO2` (type `GpioPin<2>`)
is **moved** into `Output::new`. After this line, the name
`peripherals.GPIO2` is gone — try to use it again and the compiler will
stop you. The returned `Output` value only exposes output methods
(`set_high`, `set_low`, `toggle`); there is no `read()` on it because the
type does not have one.

```rust
    let delay = Delay::new();
    loop {
        led.toggle();
        delay.delay_millis(500);
    }
}
```

A blocking delay (no RTOS) and the forever loop. The `-> !` return type
forces this function to never return — `loop {}` satisfies that.

### `broken.rs`

Identical setup, then this:

```rust
let mut _led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
let _button = Input::new(peripherals.GPIO2, InputConfig::default().with_pull(Pull::Up));
```

Two drivers, one pin. The second line uses `peripherals.GPIO2` after it
was moved into the first. The compiler refuses to build it. That refusal
is the whole point of the lesson.

The leading underscores (`_led`, `_button`) just tell the compiler "I
know I don't read these — don't warn me about unused variables." They
still own their values; they are *not* placeholders that throw the value
away.

---

## 6. Strengths and weaknesses

### Where Rust wins

- **Peripheral conflicts become compile errors.** The single biggest class
  of embedded bugs ("two things touching the same pin / timer / DMA
  channel") is caught by `cargo build`.
- **No runtime cost.** Ownership is a compile-time bookkeeping exercise.
  The generated assembly is the same as the equivalent C — sometimes
  smaller, because the compiler knows more.
- **Refactor with confidence.** Renaming a pin or reassigning it to a
  different driver propagates through the type system; you cannot forget
  to update one caller.
- **Self-documenting APIs.** `Output::new(pin, ...)` says "I take this
  pin forever" right in its signature. No comment required.

### Where Rust is annoying

- **The borrow checker has a learning curve.** Errors like
  `use of moved value` or `cannot borrow as mutable more than once` feel
  hostile until you internalize the rules. Expect a week of fighting it.
- **You have to plan ownership.** In C you can pass `GPIO_NUM_2` to
  anyone, anywhere, anytime. In Rust, if two unrelated parts of your code
  both need to touch the same peripheral, you must design how (split it,
  wrap it in a `Mutex`, use a shared interrupt-safe cell, …). The compiler
  forces a real design decision instead of letting you wing it.
- **Smaller ecosystem.** Some niche peripherals do not have a polished
  HAL crate yet; you may end up writing register-level code or contributing
  to `esp-hal` itself.
- **Error messages are dense.** They are accurate and link to the right
  line, but they refer to traits (`Copy`, `Send`, `Sync`) that a C
  developer has never seen. Read them slowly.

### Where C still has a point

- **Universal toolchains.** Every embedded vendor ships a working C
  compiler on day one. Rust support varies (Xtensa needs Espressif's LLVM
  fork; some chips have nothing at all).
- **Decades of drivers and examples.** Most reference code from chip
  vendors is in C. Translating it is doable but takes effort.
- **Familiarity.** Most embedded engineers already think in C. Onboarding
  a team to Rust is a real cost.

The takeaway is not "Rust is always better." It is: **for the specific
class of bug shown in this lesson, Rust catches it at compile time and C
cannot.** Whether that matters enough to justify the switch depends on
your project.

---

## 7. Try it yourself

From the `rust/` folder:

```bash
# This one builds, flashes and blinks
cargo run --release --bin ok

# This one fails to build with E0382
cargo build --release --bin broken
```

Then, to really feel the difference, open
[c/main/button.c](c/main/button.c), change `BUTTON_GPIO` to a different
pin (say `GPIO_NUM_3`), rebuild — the C code happily compiles either way.
Now do the same in [rust/src/bin/broken.rs](rust/src/bin/broken.rs):
change one of the two `peripherals.GPIO2` to `peripherals.GPIO3`. The
Rust build suddenly succeeds, because there is no longer a conflict to
detect.

That is the whole story. The compiler is on your side — let it do the
boring work.

---

## 8. Folder layout

```text
.
├── c/                          # ESP-IDF version with the bug
│   ├── CMakeLists.txt
│   ├── sdkconfig.defaults
│   └── main/
│       ├── main.c              # Calls both inits — silent conflict
│       ├── led.c / led.h       # Owns GPIO2 (define)
│       ├── button.c / button.h # Also owns GPIO2 (define) — oops
│       └── CMakeLists.txt
└── rust/                       # esp-hal version
    ├── Cargo.toml              # Declares two binaries: ok, broken
    ├── rust-toolchain.toml
    ├── .cargo/config.toml
    └── src/bin/
        ├── ok.rs               # Single owner — builds and runs
        └── broken.rs           # Two owners — refuses to compile
```

---

## 9. Where to go next

- Read the **"Ownership"** chapter of *The Rust Programming Language* book
  (`rust-lang.org/book`, chapter 4). It uses `String` instead of GPIO
  pins, but it is the same idea.
- Skim the `esp-hal` docs for `gpio::Output`, `gpio::Input`, and
  `gpio::Flex`. Notice how the methods available on each type are
  different — that is the type system enforcing pin direction.
- In the next lesson we will push ownership further: **borrowing**
  (passing a peripheral to a function temporarily without giving it
  away), and how to share a peripheral safely between an ISR and the
  main loop.
