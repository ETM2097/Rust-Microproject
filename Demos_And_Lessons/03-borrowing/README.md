# 03 — Borrowing: Lending a Peripheral Without Giving It Away

Lesson 02 showed that every peripheral in `esp-hal` has exactly **one owner**.
Move it into a driver and the original name is gone — try to use it again
and the compiler stops you. That rule is rock-solid for safety, but it
raises an obvious question:

> If passing a value to a function moves it, how do I write a helper that
> just *uses* the LED for a moment and gives it back?

In C this is a non-question. You pass a `GPIO_NUM_2` (an integer) anywhere
you like, as many times as you like. In Rust, you cannot — moving the pin
into a helper would leave `main` with nothing.

The answer is **borrowing**: lending out a temporary reference to a value
without transferring ownership. This lesson walks through the two kinds of
borrow Rust supports, the rule that governs them, and four small functions
— one that works, and three that the compiler flat-out refuses to build.

You already know:

- Lesson 01 — toolchain, `#![no_std]`, `#[esp_hal::main]`, `Output::new`.
- Lesson 02 — ownership and what "moved" means.
- Basic C function-call semantics (pass by value vs pointer).

You do **not** need to know what "lifetimes" are. They are mentioned in
passing, but this lesson is about the borrow rule itself, not about
annotating lifetimes by hand.

---

## 1. The problem: helpers want to use, not consume

Suppose we want to factor the toggle-and-wait pattern out of `main` into
its own function — call it `blink_once`. A first attempt, by analogy with
lesson 02:

```rust
fn blink_once(led: Output<'static>, delay: &Delay) {
    led.toggle();
    delay.delay_millis(250);
    led.toggle();
    delay.delay_millis(250);
}
```

This compiles, but it is wrong for our purposes: `Output<'static>` is a
by-value parameter, so calling `blink_once(led, &delay)` **moves** the LED
into the helper. When `blink_once` returns, the `Output` is dropped (in
Rust, drop = destructor runs), and the next iteration of the loop in
`main` has no LED to drive. You'd get a `use of moved value` error on the
second loop iteration's call site.

What we actually want is: *lend* the LED to `blink_once` for the duration
of one call, then take it back. That is what `&mut` does.

---

## 2. The borrow rule (the whole thing, on one line)

```text
Any number of shared references (&T),   OR   exactly one exclusive
reference (&mut T) — never both, never overlapping in time.
```

That is it. Everything in this lesson is a direct consequence of those two
lines. The compiler enforces them statically: at every program point it
knows which references are alive, and it rejects any program where the
rule is violated.

| Kind        | Syntax     | Reads? | Writes? | How many at once?            |
|-------------|------------|--------|---------|------------------------------|
| Shared      | `&T`       | Yes    | No      | Many                         |
| Exclusive   | `&mut T`   | Yes    | Yes     | Exactly one                  |

Two words you will see in the error messages:

- **immutable borrow** = shared (`&T`).
- **mutable borrow**   = exclusive (`&mut T`).

The names are unfortunate — "mutable" suggests "this borrow may mutate,"
but the load-bearing property is exclusivity, not mutability. Read "shared
vs exclusive" mentally even when the compiler says "immutable vs mutable."

### Why exclusivity matters, not just mutability

You can imagine a world where Rust allowed "as many writers as you want,
as long as none of them is reading." That world is full of bugs: two
writers racing each other, two interrupts both poking the same register,
the optimizer reordering writes because it assumed only one writer
existed. By requiring exclusivity for any writer, Rust gives the compiler
(and you) a single, simple rule that rules all of those out at once.

---

## 3. The working version: `main.rs`

Open [rust/src/main.rs](rust/src/main.rs). The whole program is short
enough to fit here:

```rust
fn blink_once(led: &mut Output<'static>, delay: &Delay) {
    led.toggle();
    delay.delay_millis(250);
    led.toggle();
    delay.delay_millis(250);
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

    loop {
        blink_once(&mut led, &delay);

        led.set_low();
        delay.delay_millis(500);
    }
}
```

Look at the four interesting spots:

1. **`led: &mut Output<'static>`** — the helper signature says "I take an
   exclusive borrow of an `Output`. I do not own it. When I return, the
   caller gets it back." No comment is needed — the type *is* the contract.

2. **`let mut led = Output::new(...)`** — `main` is the owner. `mut` here
   is required because we will lend it out as `&mut` later; you cannot
   take an exclusive borrow of a value that was bound with plain `let`.

3. **`blink_once(&mut led, &delay)`** — we lend the LED exclusively, and
   the delay shared. Two different borrow flavors in one call.

4. **`led.set_low()` after the call** — perfectly legal. The exclusive
   borrow taken by `blink_once` ended the instant the function returned,
   so the name `led` is fully usable again on the next line.

That's the entire payoff: helpers can operate on hardware without owning
it, and the compiler tracks the lending automatically.

### Why is `delay` shared but `led` exclusive?

`delay.delay_millis(...)` only **reads** the configured timer settings and
spins; it does not mutate the `Delay` struct. So `&Delay` (shared, many
readers welcome) is enough. `led.toggle()` and `led.set_low()` change the
GPIO output register — they *mutate* — so we need `&mut Output`.

The HAL designers chose these signatures deliberately. You can see them
in the docs: `Delay::delay_millis(&self, ...)` vs
`Output::toggle(&mut self)`. The `self` receiver type is how Rust APIs
declare what they need.

---

## 4. The broken version: `broken.rs`

Open [rust/src/bin/broken.rs](rust/src/bin/broken.rs). It contains three
functions that each violate the borrow rule in a different way. Each one
is the kind of thing you might write in C without thinking twice. None of
them compile in Rust.

Try it:

```bash
cd rust/
cargo build --release --bin broken
```

You will see three distinct errors. Let's walk through them.

### 4.1 Two exclusive borrows at once — `E0499`

```rust
fn double_mut_borrow(led: &mut Output<'static>) {
    let writer_a = &mut *led;
    let writer_b = &mut *led;     // second &mut while writer_a is still alive
    writer_a.set_high();
    writer_b.set_low();
}
```

```text
error[E0499]: cannot borrow `*led` as mutable more than once at a time
```

The intent is "I want two handles that both write to the same pin." In C
that is just two copies of `&pin_struct` or two `int` GPIO numbers — the
hardware doesn't care, so the compiler doesn't either. The bugs follow:
one module sets the pin high while another sets it low, the LED flickers
unpredictably, and you spend the afternoon with a scope.

In Rust the borrow checker walks the function top-to-bottom and notices
that `writer_a` is still alive on the line where `writer_b` is created.
Two exclusive borrows of `*led` overlap — that is precisely what the rule
forbids — and the build fails.

### 4.2 Mixing a shared and an exclusive borrow — `E0502`

```rust
fn read_while_writing(led: &mut Output<'static>) {
    let observer: &Output<'static> = &*led;     // shared borrow starts here
    led.toggle();                                // exclusive use here ...
    let _alive = observer;                       // ... but observer lives until here
}
```

```text
error[E0502]: cannot borrow `*led` as mutable because it is also borrowed as immutable
```

This one is sneakier. `observer` is a shared (read-only) reference. The
call `led.toggle()` takes an implicit exclusive borrow for the duration
of the call. If `observer` were dead by then, the program would be fine —
but the line `let _alive = observer;` afterwards keeps it alive across
the toggle. Result: a shared borrow and an exclusive borrow are alive at
the same time, on the same value. Rejected.

**Why does Rust care?** Because the holder of `&Output` is allowed to
assume the value will not change while it is reading. The optimizer bakes
that assumption in: it can cache loaded fields in registers, reorder
reads, even elide them. If a concurrent writer can poke the same value,
all of those optimizations are wrong. Rather than disable them, Rust
forbids the situation that would make them unsound.

This is the same family of bug as `const T*` in C being cast away and
written through. C lets you. Rust does not.

### 4.3 Writing through a shared reference — `E0596`

```rust
fn write_via_shared_ref(led: &Output<'static>) {
    led.set_high();
}
```

```text
error[E0596]: cannot borrow `*led` as mutable, as it is behind a `&` reference
```

The function signature promises a *shared* borrow — read only. `set_high`
needs an exclusive borrow (`&mut self`). The method is simply not
callable on the value we hold.

In C, "read only" is the `const` keyword, and casting it away is one line
of code and a shrug. Rust makes the read-only-ness part of the type:
without `&mut`, the mutating methods don't exist on the value. You cannot
"cast away" `&T` to get `&mut T` in safe Rust at all.

### What the three errors have in common

They all violate the same one-line rule from section 2. The compiler
reports three different error codes only because it wants to point at the
*specific* shape of each violation — not because the underlying rule is
three rules.

| Function                 | Borrows present                  | Violation                                      | Error    |
|--------------------------|----------------------------------|------------------------------------------------|----------|
| `double_mut_borrow`      | Two `&mut`                       | Two exclusive borrows overlap                  | `E0499`  |
| `read_while_writing`     | One `&`, one `&mut`              | Shared and exclusive overlap                   | `E0502`  |
| `write_via_shared_ref`   | One `&`, then tries to mutate    | Shared borrow cannot grant write permission    | `E0596`  |

---

## 5. The hardware angle: why this matters more on a microcontroller

On a desktop, the typical aliasing bug is two threads racing on a
`Vec<T>`. Annoying, but mostly survivable with a `Mutex`.

On a microcontroller, the same shape of bug shows up as:

- **Two drivers writing the same GPIO register.** Last write wins; the
  loser silently misbehaves. (Lesson 02's problem.)
- **The main loop and an interrupt handler both touching the same buffer
  without a critical section.** The ISR fires mid-update; the main loop
  reads garbage. No OS to catch you.
- **A DMA controller reading from a buffer the CPU is still writing.**
  Half-old, half-new bytes go out on the wire.

All three are the same underlying problem: shared mutable state with no
discipline. The borrow rule from section 2 is exactly the discipline
needed to make them impossible to write — and on embedded, where the
"runtime" is whatever silicon you wire up, "make it impossible to write"
is the only level of guarantee that holds.

(Sharing across an interrupt boundary needs more than `&mut` — it needs
`Mutex`, `RefCell`, or `critical-section`. That is a topic for a later
lesson.)

---

## 6. Strengths and weaknesses

### Where borrowing wins

- **Helpers don't have to take ownership.** Factor freely. The compiler
  tracks the lending automatically; you do not babysit it.
- **Data races become compile errors.** Two writers on one peripheral is
  not "tested for" — it is *not expressible* in the type system.
- **Zero runtime cost.** A `&mut T` is just a pointer at runtime. No
  reference counts, no locks, no checks. The compiler did the work.
- **Self-documenting signatures.** `fn blink_once(led: &mut Output, ...)`
  tells you, at the call site, exactly what the function will do to its
  arguments. No comment, no convention.

### Where borrowing is annoying

- **The borrow checker has a learning curve.** Errors like `E0502` and
  `E0499` feel hostile until you internalize the rule. The compiler is
  always right, but it takes practice to see *why* on a glance.
- **Self-referential structures are hard.** If you want a struct that
  holds both a buffer and a pointer into that buffer, the borrow checker
  refuses. You learn workarounds (`Pin`, indices, arenas), but they take
  thought.
- **Lifetimes leak into APIs.** Once you start storing references in
  structs, lifetime annotations (`<'a>`) appear in signatures. They are
  not in this lesson, but they will be soon.

### Where C's looser model has a point

- **Quick experiments.** In C you can pass a pin to anyone, anywhere,
  while you are still figuring out the architecture. In Rust you commit
  to an ownership/borrowing structure early.
- **Legacy patterns.** Most vendor code passes pin numbers around as
  integers. Translating that to a borrow-checked Rust design is doable
  but is real work.

The takeaway, same shape as in lesson 02: **the borrow checker forbids a
specific, well-defined class of bug, and forbids it at compile time.**
Whether that trade is worth it depends on the project. For shipping
firmware, it usually is.

---

## 7. Try it yourself

From the `rust/` folder:

```bash
# Builds, flashes, blinks (with the helper holding a &mut borrow).
cargo run --release --bin main

# Fails to compile with E0499, E0502, E0596 — one per function.
cargo build --release --bin broken
```

A few experiments worth doing:

1. **Make the broken file compile.** In `double_mut_borrow`, drop one of
   the two `&mut` borrows. The error goes away.
2. **Add a scope.** In `read_while_writing`, wrap the shared borrow in
   `{ let observer = &*led; let _ = observer; }`. The shared borrow now
   ends before `led.toggle()`, and the function compiles. This is
   "non-lexical lifetimes" doing its job: borrows die at last use, not at
   end of block.
3. **Change `&Delay` to `&mut Delay` in `blink_once`.** The build still
   succeeds (you only ever pass one at a time), but the signature now
   lies about what the function does. APIs should ask for the *least*
   they need.
4. **Try to call `blink_once(&mut led, &delay)` twice in a row inside a
   `let writer = &mut led;` block.** You will hit `E0499` for the same
   reason as `double_mut_borrow` — two `&mut led` borrows alive at once.

Each error is the compiler telling you something true about the program.
Read it slowly. The line numbers it points to are exactly the lines that
need to change.

---

## 8. Folder layout

```text
.
└── rust/                          # esp-hal version, two binaries
    ├── Cargo.toml                 # Declares both binaries: main, broken
    ├── rust-toolchain.toml
    ├── .cargo/config.toml
    └── src/
        ├── main.rs                # Borrowing done right — builds and runs
        └── bin/
            └── broken.rs          # Three borrow violations — refuses to compile
```

Both binaries are configured in [rust/Cargo.toml](rust/Cargo.toml):

```toml
[[bin]]
name = "main"
path = "src/main.rs"

[[bin]]
name = "broken"
path = "src/bin/broken.rs"
```

So `cargo run --bin main` flashes the working program and
`cargo build --bin broken` is the one you use to see the errors.

---

## 9. Where to go next

- Read the **"References and Borrowing"** chapter of *The Rust Programming
  Language* book (`rust-lang.org/book`, chapter 4.2). It uses `String`
  instead of GPIO, but the rules are the same.
- Skim the [Rust by Example page on borrowing](https://doc.rust-lang.org/rust-by-example/scope/borrow.html)
  — short, code-first, good companion to this lesson.
- Look at the `esp-hal` `Output` and `Input` methods and notice which
  ones take `&self` vs `&mut self`. That is the API author telling you,
  in the type, which operations are read-only and which mutate.

