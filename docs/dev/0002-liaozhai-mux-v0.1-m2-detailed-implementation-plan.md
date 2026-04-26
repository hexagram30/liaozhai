# Liaozhai MUX v0.1 — M2 Detailed Implementation Plan

## Context

M1 is complete: the workspace has 6 crates, a tokio TCP listener, and a banner-then-close connection handler. M2 replaces the close-on-connect behavior with a real telnet codec — line-oriented input, IAC stripping, echo loop, and session terminators. The design decisions are documented in `workbench/m2-implementation-plan.md`; this plan operationalizes them into files, types, and tests.

**Source documents:**
- `workbench/m2-implementation-plan.md` (design decisions, rationale)
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (M2 acceptance criteria)

---

## Implementation Order

1. Cargo.toml changes (add tokio-util, futures-util)
2. New constants + error variant in liaozhai-core
3. TelnetCodecError + TelnetLineCodec struct (types only)
4. `strip_iac` method + IAC unit tests
5. `Decoder::decode` + `decode_eof` + line-splitting/overflow tests
6. `LineWriter` output helper
7. `is_session_terminator` helper
8. Rewrite `handle_connection` with echo loop + integration tests

---

## Step 1: Cargo.toml Changes

### Root `Cargo.toml` — add to `[workspace.dependencies]`:
```toml
tokio-util    = { version = "0.7", features = ["codec"] }
futures-util  = "0.3"
```

### `crates/liaozhai-net/Cargo.toml` — add to `[dependencies]`:
```toml
tokio-util.workspace = true
futures-util.workspace = true
```

---

## Step 2: Constants + Error Variant

### `crates/liaozhai-core/src/constants.rs` (MODIFY) — append:
```rust
pub const MAX_LINE_LENGTH: usize = 4096;
pub const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10 MB
pub const LINE_TOO_LONG_MSG: &str = "Line too long; ignored.\r\n";
pub const BUFFER_OVERFLOW_MSG: &str = "Buffer overflow; disconnecting.\r\n";
pub const GOODBYE_MSG: &str = "Until the next strange tale.\r\n";
```

Tests: verify values of `MAX_LINE_LENGTH` and `MAX_BUFFER_SIZE`.

### `crates/liaozhai-core/src/error.rs` (MODIFY) — add variant after `Net`:
```rust
#[error("codec error: {0}")]
Codec(String),
```

Test: `codec_error_display`.

---

## Step 3: TelnetCodecError + TelnetLineCodec Struct

### `crates/liaozhai-net/src/codec.rs` (CREATE)

**Error type:**
```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TelnetCodecError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("line too long (max {max} bytes)")]
    LineTooLong { max: usize },

    #[error("buffer overflow (max {max} bytes)")]
    BufferOverflow { max: usize },
}
```

`#[non_exhaustive]` because the codec error is likely to gain variants — strict-UTF-8 mode if we ever flip the lossy default, subnegotiation-too-long, charset-negotiation failures. Marking now keeps later additions non-breaking.

Plus `impl From<TelnetCodecError> for liaozhai_core::error::Error`.

**Codec struct:**
```rust
#[derive(Debug)]
pub struct TelnetLineCodec {
    max_line_length: usize,     // 4096
    max_buffer_size: usize,     // 10 MB
    discarding: bool,           // overflow recovery
    in_subnegotiation: bool,    // inside IAC SB...SE
    subneg_iac_pending: bool,   // saw 0xFF inside subneg; next byte is SE or escaped IAC
}
```

`subneg_iac_pending` (renamed from the more cryptic `next_is_se_check`) reads as: "we are inside a subnegotiation and the previous byte was IAC; the next byte is either SE (close) or another IAC (escaped 0xFF data, which we still discard)."

**Constructors:**
- `pub fn new() -> Self` — defaults from `liaozhai_core::constants` (`MAX_LINE_LENGTH`, `MAX_BUFFER_SIZE`).
- `pub fn with_limits(max_line_length: usize, max_buffer_size: usize) -> Self` — explicit limits. Visibility is `pub` (not `pub(crate)`) so integration tests in `tests/` directories can construct codecs with small limits to exercise overflow paths quickly.
- `impl Default for TelnetLineCodec` — delegates to `new()`. A fresh-state codec is a meaningful default value; this is the opposite call from the M1 ID newtypes (where random-default would be misleading).

**TODO(M6):** consider exposing `max_line_length` and `max_buffer_size` in `ServerConfig` if operators surface a need to tune them.

**Telnet byte constants (module-private):**
```rust
const IAC: u8 = 0xFF;
const SB: u8 = 0xFA;
const SE: u8 = 0xF0;
const WILL: u8 = 0xFB;
const WONT: u8 = 0xFC;
const DO: u8 = 0xFD;
const DONT: u8 = 0xFE;
```

### `crates/liaozhai-net/src/lib.rs` (MODIFY) — add modules:
```rust
pub mod codec;
pub mod output;
```

**Module structure note.** Single-file `codec.rs` is fine for M2's scope. If the codec grows past ~400 LOC (likely when GMCP, MCCP, or NAWS support arrives in later milestones), refactor to `codec/mod.rs` with submodules for `iac`, `line`, and `tests`. No need to pre-emptively split.

---

## Step 4: `strip_iac` Method

In-place scan over `BytesMut` with read/write cursors. Logic:

- **Subneg mode**: discard all bytes; watch for `IAC SE` to exit. Handle `IAC IAC` inside subneg as escaped data (still discarded).
- **Normal mode**:
  - Non-IAC byte → copy to write position
  - `IAC IAC` → emit one literal `0xFF`
  - `IAC WILL/WONT/DO/DONT <option>` → discard 3 bytes
  - `IAC SB` → enter subneg mode, discard 2 bytes
  - `IAC <other>` → discard 2 bytes
  - `IAC` at end of buffer → leave for next decode call
  - Incomplete 3-byte sequence at end → leave for next call
- Truncate buffer to write cursor length after scan

Unit tests (through `decode`): IAC IAC emits 0xFF, WILL/WONT/DO/DONT discarded, SB...SE discarded, subneg across calls, IAC at buffer end waits, incomplete 3-byte waits, IAC IAC inside subneg, multiple IAC sequences in one line.

---

## Step 5: `Decoder::decode` + `decode_eof`

Two-phase algorithm:
1. **Buffer cap check**: if `buf.len() > max_buffer_size` → return `Err(BufferOverflow)`
2. **IAC strip**: call `self.strip_iac(buf)`
3. **Line scan**: search for `\r`, `\n`, or `\r\n`
   - **Found + discarding**: consume through terminator, clear flag, recurse
   - **Found + normal**: extract line bytes, advance past terminator, lossy UTF-8 decode with warn-log → return `Ok(Some(line))`
   - **Not found + discarding**: clear buffer, return `Ok(None)`
   - **Not found + buf > max_line_length**: clear buffer, set discarding, return `Err(LineTooLong)`
   - **Not found + fits**: return `Ok(None)` (need more data)

**`decode_eof`**: handles four cases explicitly:
1. Empty buffer → `Ok(None)`.
2. Buffer contains an incomplete IAC sequence (lone `0xFF`, or `IAC WILL/WONT/DO/DONT` without the option byte, or `IAC SB ...` without the closing `IAC SE`) → discard, return `Ok(None)`. Bytes that won't arrive shouldn't be emitted as half-formed data.
3. Buffer contains unterminated data that isn't an incomplete IAC sequence → emit it as a final line via the lossy-UTF-8 decode path. Returns `Ok(Some(line))`.
4. `discarding` is set when `decode_eof` is called → returns `Ok(None)` regardless of buffer contents (we were already going to drop these bytes; EOF doesn't change that).

**Helper functions (module-private):**
- `find_line_terminator(buf) -> Option<usize>`
- `consume_terminator(buf, pos) -> usize` (handles CRLF as 2, else 1)
- `lossy_decode_with_warning(bytes: &[u8]) -> String`

**Implementation note for `lossy_decode_with_warning`:** `String::from_utf8_lossy` returns a `Cow<str>`. If the result is `Cow::Borrowed`, the input was valid UTF-8 — no warn-log. If `Cow::Owned`, at least one byte was replaced — emit a single `warn!` with the connection ID and a sample of the offending bytes (first 32 bytes is enough). The `Cow` variant check is the cheapest correctness signal; don't reach for byte-by-byte comparison.

**Unit tests**: CRLF/LF/CR splitting, empty lines, multiple lines per buffer, partial line waits, line-too-long triggers discard then recovery, buffer overflow, valid/invalid UTF-8, byte-at-a-time feeding. **`decode_eof` cases**: empty buffer (`Ok(None)`), incomplete IAC sequence at EOF (`Ok(None)`), unterminated data at EOF (`Ok(Some(line))`), discarding-mode-at-EOF (`Ok(None)`).

---

## Step 6: LineWriter Output Helper

### `crates/liaozhai-net/src/output.rs` (CREATE)

```rust
pub struct LineWriter {
    inner: OwnedWriteHalf,
}
```

Methods:
- `new(write_half: OwnedWriteHalf) -> Self`
- `async fn write_line(&mut self, line: &str) -> io::Result<()>` — writes line + CRLF
- `async fn write_raw(&mut self, data: &[u8]) -> io::Result<()>` — exact bytes
- `async fn flush(&mut self) -> io::Result<()>` — explicit flush; useful before shutdown or when output ordering matters
- `async fn shutdown(&mut self) -> io::Result<()>`

Tests: `write_line` appends CRLF, `write_raw` sends exact bytes, `flush` returns `Ok(())` on a healthy stream. Use real TCP loopback sockets.

---

## Step 7: Session Terminator Helper

In `connection.rs` (private function):

```rust
fn is_session_terminator(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "quit" | "exit" | "bye" | "disconnect"
    )
}
```

Tests: all four commands, case insensitivity, whitespace tolerance, rejects non-terminators.

---

## Step 8: Rewrite `handle_connection`

Full rewrite of `crates/liaozhai-net/src/connection.rs`:

1. Split `TcpStream` via `into_split()` → `(OwnedReadHalf, OwnedWriteHalf)`
2. Wrap read half: `FramedRead::new(read_half, TelnetLineCodec::new())`
3. Wrap write half: `LineWriter::new(write_half)`
4. Send banner via `writer.write_raw(BANNER.as_bytes())`
5. Echo loop using `lines.next().await`:
   - `Some(Ok(line))`: check `is_session_terminator` → goodbye + break; else echo via `writer.write_line`
   - `Some(Err(LineTooLong))`: write `LINE_TOO_LONG_MSG`, continue loop
   - `Some(Err(BufferOverflow))`: write `BUFFER_OVERFLOW_MSG`, break
   - `Some(Err(Io(_)))`: log, break
   - `None`: EOF, break
6. `writer.shutdown()`, log closure with line count

**Signature unchanged**: `pub async fn handle_connection(stream: TcpStream, peer: SocketAddr) -> liaozhai_core::error::Result<()>` — no changes needed in `liaozhai-server/src/listener.rs`.

**Integration tests** (replace M1's `sends_banner_and_closes`):
- `banner_then_echo` — send line, read echo, quit
- `quit_ends_session` — quit immediately after banner
- `exit_ends_session` — exit variant
- `multiple_lines_echoed` — three lines, each echoed
- `client_disconnect_without_quit` — close socket, server task completes cleanly
- `iac_bytes_stripped_in_echo` — send raw IAC WILL bytes, echo is clean
- `line_too_long_continues` — overflow, notice, then normal lines work

---

## File Inventory

### CREATE (2 files)
| File | Purpose |
|------|---------|
| `crates/liaozhai-net/src/codec.rs` | TelnetCodecError, TelnetLineCodec, Decoder impl, IAC stripping, unit tests |
| `crates/liaozhai-net/src/output.rs` | LineWriter helper |

### MODIFY (5 files)
| File | Change |
|------|--------|
| `Cargo.toml` (root) | Add tokio-util, futures-util to workspace deps |
| `crates/liaozhai-net/Cargo.toml` | Add tokio-util, futures-util |
| `crates/liaozhai-net/src/lib.rs` | Add `pub mod codec;` and `pub mod output;` |
| `crates/liaozhai-net/src/connection.rs` | Full rewrite: split stream, codec, echo loop |
| `crates/liaozhai-core/src/constants.rs` | Add MAX_LINE_LENGTH, MAX_BUFFER_SIZE, message constants |
| `crates/liaozhai-core/src/error.rs` | Add Codec(String) variant |

---

## Test Coverage Target

M2 introduces substantial logic in the codec — IAC stripping, line splitting, overflow recovery, subnegotiation state machine. Realistic targets:

- **`liaozhai-net::codec`**: 75–85% coverage. The `Decoder` trait is unit-test friendly (call `decode()` with a `BytesMut`, assert on the output and codec state); most branches are reachable from synthetic byte sequences. Aim high on this module specifically — it's the densest piece of v0.1.
- **`liaozhai-net::output` and `liaozhai-net::connection`**: 60–70%. Both are I/O-bound and require integration-test plumbing; their coverage will rise as M3+ adds more behavior worth asserting.
- **Workspace overall (M2 cumulative)**: 65–75%. ADR-0011's 80%-by-M6 target stays on track.

`cargo llvm-cov --workspace` (configured per the M1 plan) is the measurement tool. CI enforcement is M6 polish; M2 tracks coverage informally.

---

## Verification

```bash
make check                                    # build + clippy + fmt + test
make run                                      # then in another terminal:
telnet 127.0.0.1 4444                         # verify banner, echo, quit
RUST_LOG=debug make run                       # verify structured logs with conn_id

# Config and CLI override behavior (carry-over from M1; should still work):
cargo run --bin liaozhai-server -- run --port 4444
cargo run --bin liaozhai-server -- run --config liaozhai.example.toml --port 5555

# Each session-ending alias (case + whitespace tolerance):
printf 'quit\r\n'        | nc 127.0.0.1 4444   # canonical
printf 'EXIT\r\n'        | nc 127.0.0.1 4444   # uppercase alias
printf '  bye  \r\n'     | nc 127.0.0.1 4444   # whitespace tolerance
printf 'disconnect\r\n'  | nc 127.0.0.1 4444   # multi-syllable alias
```

Behaviors to confirm manually before tagging M2 complete:

- Type lines → see them echoed back, one for one.
- Type any of `quit` / `EXIT` / `  bye  ` / `disconnect` → see `Until the next strange tale.` and disconnect.
- Paste a single line longer than 4096 chars → see `Line too long; ignored.` then normal echo resumes on the next line.
- Connect with PuTTY (sends IAC option negotiation on connect) → no garbled bytes appear in echoed output.
- Close the telnet client without typing a session terminator → server logs a clean disconnect (no error stack, no leaked task).
- Send invalid UTF-8 bytes (e.g., `printf '\xff\xfe\xfdhello\r\n' | nc 127.0.0.1 4444`) → output contains U+FFFD replacement characters; server log shows one warn entry per affected line.

The 10 MB buffer-overflow path (criterion 8 in the design plan) is impractical to exercise manually; verify via the codec unit test that constructs a `TelnetLineCodec::with_limits(4096, 256)` and feeds it 257 bytes without a terminator.
