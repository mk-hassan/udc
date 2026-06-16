# UDC (Unified Data Clone)

`udc` is a cross-platform, high-performance, low-level block I/O data streaming utility designed to emulate the core behavior of the standard POSIX `dd` command. Written natively in Rust, it breaks platform-lock boundaries, allowing identical syntax and operational behaviors seamlessly across **Linux, macOS, and Windows** without requiring emulation wrappers like Cygwin or WSL.

---

## Supported Command-Line Arguments

All parameters utilize a strict `key=value` format pattern. Spaces around the `=` sign are not supported.

| Parameter | Type / Example | Description |
| :--- | :--- | :--- |
| `if=FILE` | Path (`if=/dev/sdb`) | Core input stream source path. Defaults to standard input (`stdin`) if omitted. |
| `of=FILE` | Path (`of=disk.img`) | Destination output stream path. Defaults to standard output (`stdout`) if omitted. |
| `bs=SIZE` | Size (`bs=4M`) | Sets both the input (`ibs`) and output (`obs`) block sizing symmetrically. |
| `ibs=SIZE`| Size (`ibs=4096`) | Overrides the input block consumption boundary. |
| `obs=SIZE`| Size (`obs=512`) | Overrides the output block writing boundary. |
| `count=N` | Integer (`count=100`)| Binds copy length to exactly `N` fully processed blocks. |
| `count_bytes=N` | Size (`count_bytes=1G`) | Linearly terminates copying loops precisely after writing `N` bytes. |
| `skip=N`  | Integer (`skip=10`) | Skips `N` blocks from the start of the input stream before reading. |
| `skip_bytes=N` | Size (`skip_bytes=512`) | Skips an exact raw byte offset at the beginning of the input stream. |
| `seek=N`  | Integer (`seek=20`) | Postpones the writing start by seeking `N` blocks forward in the output stream. |
| `seek_bytes=N` | Size (`seek_bytes=4k`) | Positions the output stream forward by exactly `N` raw bytes. |
| `status=LEVEL` | `none` \| `noxfer` \| `progress` | Configures console metrics tracking and speed telemetry reporting verbosity. |
| `iflag=FLAGS`  | Comma-separated list | Advanced lower-level execution configurations targeting the input descriptor. |
| `oflag=FLAGS`  | Comma-separated list | Advanced lower-level execution configurations targeting the output descriptor. |
| `conv=CONVS`   | Comma-separated list | In-flight data transforms and error resilience behaviors. |

---

## Detailed Performance Tuning Flags

`iflag` and `oflag` expose precise controls over kernel handles and page caches. Multiple values can be joined via commas (e.g., `iflag=direct,nonblock`).

* **`direct`** (Supported on `iflag` and `oflag`)
  Enforces raw Direct I/O operations, bypassing operating system virtual filesystem cache pools entirely to achieve clean zero-copy throughput.
  * **Linux / Unix**: Leverages standard `O_DIRECT` file creation directives.
  * **Windows**: Dynamically implements native `FILE_FLAG_NO_BUFFERING` Win32 runtime parameters.
  * **macOS**: Optimizes cache invalidation semantics when dealing with raw disk slices (e.g., `/dev/rdisk*`).
* **`nonblock`** (Supported on `iflag`)
  Configures non-blocking interactive stream attributes.
  * **Linux / Unix**: Passes `O_NONBLOCK` instructions directly to standard file handles.
  * **Windows**: Unmapped by native NT file semantics. Passing this on Windows safely generates an ignition/runtime console skip warning.

---

## In-Flight Data Conversion Parameters (`conv=`)

Modify byte streams or stream layouts on the fly using standard `conv=` keywords:

* **`lcase`**: Mutates ASCII characters to lowercase equivalents in-place within staging buffers.
* **`ucase`**: Mutates ASCII characters to uppercase equivalents in-place within staging buffers.
* **`swab`**: Swaps every adjacent pair of input bytes in memory (primarily used for converting endianness).
* **`sync`**: Pads short or partial read operations with null blocks (`\0`) up to the configured `ibs` threshold.
* **`notrunc`**: Prevents truncating the target output file descriptor upon binding, preserving existing trailing data.
* **`sparse`**: Performance optimization that avoids writing pure null blocks to storage, converting them to physical system seeks instead to preserve disk blocks.
* **`noerror`**: Swallows readable hardware/I/O sector failures, allowing data recovery operations to continue transferring healthy blocks.

---

## Numeric Suffix Multipliers

Any parameters accepting a numeric size (`SIZE` or `N` byte parameters) accept standard storage binary suffix multipliers:

* **`c`**: 1 Byte
* **`w`**: 2 Bytes
* **`b`**: 512 Bytes
* **`k` / `K`**: 1,024 Bytes (1 KiB)
* **`M`**: 1,048,576 Bytes (1 MiB)
* **`G`**: 1,073,741,824 Bytes (1 GiB)

### Example Invocation

```bash
# Safely copy a bootable storage partition with Direct I/O, uppercase conversion, and sparse file optimization
udc if=/dev/sdb of=backup_raw.img bs=4k conv=ucase,sparse iflag=direct oflag=direct status=progress
```

## License

This project is dual-licensed under the MIT license and the Apache License, Version 2.0. 
See the [LICENSE](LICENSE) file for the full text.

SPDX-License-Identifier: MIT OR Apache-2.0
