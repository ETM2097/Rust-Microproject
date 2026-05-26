# Lesson 04 — Option, Result, and enums

## Learning objectives

- Why `Option<T>` replaces `NULL` and sentinel values: absence-of-value is a distinct variant the compiler forces the caller to handle, not a magic number smuggled inside a normal return.
- Why `Result<T, E>` replaces sentinel return codes and `errno`: success and failure are sibling variants in a single return type, and ignoring one is a compile error in most idiomatic uses.
- What an exhaustive `match` guarantees that a C `switch` does not: every variant of the enum must be handled, or the program does not compile.

## Prerequisites

- Previous lessons: 01, 02, 03.
- Assumed C/C++ background: writing functions that return `int` with sentinel values, checking `errno`, and the kinds of bugs that come from forgetting the check.
- Hardware: ESP32-S3 dev board with an LED + 330 Ω resistor between GPIO2 and GND.
- Estimated talk time: 25 min.

## The problem in C/C++

The C convention is to fold "no value" or "failure" into the same `int` that carries the result. The caller is expected to know that, say, `-1` is special and to test for it:

```c
/* `read_temperature` returns the value, or -1 on error. The caller is
   expected to check. If they forget, -1 propagates into a calculation
   that produces a plausible-looking but wrong result. */
int read_temperature(void)
{
    if (!sensor_ready())
    {
        errno = ENODEV;
        return -1;
    }
    return sensor_get_celsius();
}

int average_temperature(void)
{
    int a = read_temperature();
    int b = read_temperature();
    return (a + b) / 2;   /* If either failed, the average is meaningless. */
}
```

"No value" is encoded as a chosen-by-convention magic number — `-1`, `NULL`, `INT_MIN`, `SIZE_MAX`, and so on. The compiler does not know the magic number is special; the caller is expected to know it by convention and to check for it. C23's `[[nodiscard]]` helps, but it is opt-in and rarely applied retroactively to legacy headers.

The C `switch` produces the parallel problem one level up, on the enum itself: forgetting a `case` falls through silently, and the compiler does not require exhaustiveness. A new variant added to an `enum` will compile against every existing `switch` without warning, even though those `switch` statements no longer handle every case.

## How Rust handles it

The Rust source for this lesson:

```rust
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
use esp_println::println;

enum Command {
    Toggle,
    SetPeriod(u32),
    Halt,
}

fn parse_command(input: &str) -> Result<Command, &'static str> {
    if input == "toggle" {
        Ok(Command::Toggle)
    } else if let Some(rest) = input.strip_prefix("period ") {
        let n: u32 = rest.parse().map_err(|_| "period requires a u32")?;
        Ok(Command::SetPeriod(n))
    } else if input == "halt" {
        Ok(Command::Halt)
    } else {
        Err("unknown command")
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );
    let delay = Delay::new();

    let inputs = ["toggle", "period 250", "junk", "halt"];

    for raw in inputs {
        match parse_command(raw) {
            Ok(Command::Toggle) => {
                println!("toggling");
                led.toggle();
            }
            Ok(Command::SetPeriod(ms)) => {
                println!("set period to {} ms", ms);
            }
            Ok(Command::Halt) => {
                println!("halt requested");
                break;
            }
            Err(msg) => {
                println!("parse error: {}", msg);
            }
        }
        delay.delay_millis(500);
    }

    loop {
        led.toggle();
        delay.delay_millis(1000);
    }
}
```

Walking through the new pieces:

- **The `Command` enum.** Three variants, one of which (`SetPeriod`) carries a `u32` payload. This is a sum type — what C++ calls `std::variant`, what functional programmers call a tagged union. The variant tag and the payload are one inseparable value; there is no way to "have a `SetPeriod` tag but forget the `u32`" or to "read the `u32` payload of a `Toggle`". The compiler tracks the relationship.
- **`Result<Command, &'static str>`.** Success is `Ok(Command)`, failure is `Err(message)`. They are the same type — sibling variants of `Result` — and you cannot get to the inner `Command` without first checking which variant you have. The type system makes the check unavoidable.
- **The `?` operator on `rest.parse()`.** If `parse` returns `Err(_)`, `?` immediately returns from the function with that error (after running `map_err` to convert it). If `parse` returns `Ok(n)`, `n` is bound and execution continues. No exception, no `setjmp`/`longjmp` — just an early `return` desugared by the compiler. The happy path reads top to bottom.
- **The `match`.** Every variant of `Result<Command, _>` is covered: the three `Ok(...)` arms and one `Err(...)` arm. If a new variant is later added to `Command`, every `match` that handled the old set of variants will fail to compile until it is updated. A C `switch` would have silently compiled with the old, now-incomplete code.

## Why / what for

1. **`Option<T>` and `Result<T, E>`** because failure is a value with a type, not a magic number. The compiler can reason about it; the programmer cannot accidentally treat it as a normal result.
2. **Exhaustive `match`** because forgetting to handle a variant of an enum is a bug class the compiler can detect at zero runtime cost.
3. **The `?` operator** because propagating an error one frame up is the common case; it should not require five lines of ceremony.
4. **Sum-type enums** because a "kind tag plus a payload" is one value with one type, not two correlated values that can drift out of sync. The C pattern of `struct { int kind; union { ... } payload; }` keeps the two in lockstep by convention only — Rust enforces it in the type.
5. **Together, these eliminate "did you forget to check the return?"** as a class of bug. The compiler refuses to compile code that ignores a `Result`, except by explicit, audited `let _ = ...` or `.unwrap()` calls — both of which announce themselves in review.

## Side-by-side

| Aspect | C | Rust |
| --- | --- | --- |
| Absence of value | `NULL`, `-1`, `INT_MIN`, by convention | `Option<T>` — a distinct variant |
| Failure code | return value plus `errno`, by convention | `Result<T, E>` — a distinct variant |
| Propagating an error one level up | `if (rc != 0) return rc;`, repeated | `let n = parse()?;` |
| Forgetting to handle a case | `switch` falls through silently | `match` is a compile error |
| Forgetting to check the return | compiles silently (except with `[[nodiscard]]`) | `Result` is `#[must_use]` by default |

## Build and run (reference)

```bash
cd lessons/04-option-result-and-enums/rust
cargo run --release
```

Shared setup lives in `../../docs/setup-esp32s3.md` and `../../docs/build-guide.md`.
