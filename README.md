# ccdd

A Rust implementation of the Unix `dd` utility — copies data between sources with configurable input/output block sizes.

```
ccdd [if=<path>] [of=<path>] [bs=<n>] [ibs=<n>] [obs=<n>]
```

---

## Usage

```bash
# file to file
ccdd if=input.bin of=output.bin

# stdin to stdout (default)
echo "hello" | ccdd

# custom block sizes
ccdd if=input.bin of=output.bin ibs=1024 obs=4096

# unified block size (sets both ibs and obs)
ccdd if=input.bin of=output.bin bs=512
```

**Output:**
```
1+1 records in
2+0 records out
1000 bytes copied, 0.000031 s, 30.52 MB/s
```

---

## Module Structure

```
src/
├── main.rs       — entry point: arg parsing, wiring, display
├── lib.rs        — crate root: declares modules, defines SourceType
├── config.rs     — Config struct: parses CLI arguments
├── pipeline.rs   — core copy loop + Metrics struct
└── constants.rs  — shared string keys and default values
```

### Why split this way?

Rust has two crate types in a single package:

- **`lib.rs`** — the library crate. Reusable, testable, importable by others.
- **`main.rs`** — the binary crate. Depends on the library, handles I/O and process exit.

The dependency only flows one way: `main.rs → lib.rs`. The library cannot reference anything in `main.rs`. This forced a clean separation: `Config`, `pipeline`, and `SourceType` all live in the library where they can be unit-tested without spawning a process.

---

## Rust Concepts Used

### `Box<dyn Trait>` — trait objects for polymorphism

The read and write buffers return `Box<dyn BufRead>` and `Box<dyn Write>` so that both file and stdin/stdout branches share the same return type:

```rust
pub fn open_read_buffer(source: &SourceType, capacity: usize)
    -> Result<Box<dyn BufRead>, Box<dyn Error>>
```

`BufReader<File>` and `BufReader<Stdin>` are different concrete types — boxing them behind a trait object lets the caller treat them uniformly.

### `?` operator — error propagation

```rust
File::open(path)?
```

Equivalent to `match ... { Err(e) => return Err(e.into()), Ok(v) => v }`. Propagates the error up the call stack, converting it to `Box<dyn Error>` automatically.

### `BufReader` / `BufWriter` — buffered I/O

Raw `File` reads hit the OS on every call. Wrapping with `BufReader::with_capacity(ibs, file)` batches reads into `ibs`-sized chunks in userspace, dramatically reducing syscall overhead for small block sizes.

### `Vec<u8>` as accumulation buffer

When `ibs != obs`, bytes cannot be written immediately after each read. An accumulator collects valid bytes, then drains in `obs`-sized chunks:

```rust
let mut read_buf = vec![0u8; ibs];   // fixed-size read window
let mut accum: Vec<u8> = Vec::new(); // grows and drains as needed

while let Ok(reads) = reader.read(&mut read_buf) {
    accum.extend_from_slice(&read_buf[..reads]); // only valid bytes
    while accum.len() >= obs {
        writer.write_all(&accum[..obs])?;
        accum.drain(..obs);
    }
}
```

**Key insight:** `read()` overwrites `read_buf` from index 0 every call and returns how many bytes it wrote. `read_buf.len()` stays fixed at `ibs` — only `read_buf[..reads]` is valid data. The rest is stale from the previous iteration.

### `impl Display` — idiomatic formatting

Instead of a custom print method, `Metrics` implements `std::fmt::Display`:

```rust
impl Display for Metrics { ... }
```

This gives `println!("{}", metrics)`, `format!("{}", metrics)`, and `.to_string()` for free via a blanket impl in the standard library.

### `Instant` — monotonic timing

```rust
let start = Instant::now();
// ... copy loop ...
metrics.time_duration = start.elapsed();
```

`Instant` is monotonic — unaffected by system clock changes or NTP adjustments — making it the correct choice for measuring elapsed wall time.

### `Option<usize>` for optional config values

Block sizes default to `None` and only become `Some(n)` when explicitly passed. Priority is resolved at call time:

```rust
pub fn get_ibs(&self) -> usize {
    self.input_block_size
        .or(self.block_size)
        .unwrap_or(DEFAULT_BLOCK_SIZE)
}
```

`ibs` overrides `bs`, which overrides the default — matching Unix `dd` semantics.

---

## Why `ibs != obs` needs an accumulator

A naive implementation uses a single buffer as both the read target and write source. This breaks in two ways:

1. **Stale bytes:** after a partial read of `n < ibs` bytes, `buffer[n..ibs]` still holds zeros (or data from the previous iteration). Writing `buffer[..obs]` includes that garbage.
2. **Shrinking buffer:** `buffer.drain(..obs)` reduces the slice length, so the next `reader.read(&mut buffer)` reads into a shorter window — potentially reading fewer bytes than `ibs` even when the source has more.

The fix is to keep `read_buf` at a fixed `ibs` length always, and accumulate only the valid slice `read_buf[..reads]` into a separate `Vec`.

---

## Testing

Tests live in `pipeline.rs` and cover:

| Scenario | What is verified |
|---|---|
| Empty file | No blocks, no writes, zero bytes |
| Single byte | Minimum non-empty case |
| All 256 byte values | No byte corruption |
| File size = exact multiple of bs | No partials on either side |
| File size = not multiple of bs | Partial read and write counters |
| File size = exactly ibs | Boundary: last read is a full block |
| File size < ibs | Single partial read |
| `ibs < obs` | Accumulation across multiple reads |
| `obs > file size` | Entire file as one partial write |
| `ibs > obs` | Multiple writes from one read |
| `ibs=1, obs=1` | Byte-by-byte, maximum granularity |
| `ibs=1, obs > file` | Tiny reads, single partial flush |
| 1 MB file | Content integrity at scale |

```bash
cargo test
```

---

## Build

```bash
cargo build --release
./target/release/ccdd if=input.bin of=output.bin bs=4096
```
