# Extracting the core

Splitting this into a shared Rust core and per-platform shells, so a native
macOS app becomes possible without maintaining the product twice.

## Why now, and why this order

The decision that prompted this is wanting real Liquid Glass on macOS. That is
not a tuning problem: in macOS 26, glass responds to scrolling content, morphs
during transitions, and is composited by the system. To AppKit a webview is one
opaque rectangle — it can sit *on* glass but cannot participate in it. Controls
that should each be glass (sidebar, toolbar, buttons) can never be, and only the
window background can. That is a ceiling, not a setting.

The core is extracted **before** any SwiftUI is written, because it is the step
that is worth doing either way:

- It is what makes a second UI cheap instead of a second product.
- It forces the boundary to be explicit. Today `lib.rs` is 5,083 lines in which
  domain logic and Tauri command plumbing are interleaved; nobody can currently
  say which half is which.
- It can be abandoned at any point with the work still banked. Writing SwiftUI
  first cannot.

## What is actually shared

Measured, not estimated (`wc -l`, 2026-08-08):

| | Lines | Fate |
|---|---:|---|
| Rust | 8,881 | mostly shared |
| Frontend | 4,912 | Tauri shells only |
| — of which real UI | ~3,333 | rewritten in SwiftUI for macOS |
| — of which i18n catalogues | 551 | **strings** carry over, mechanism does not |
| — of which IPC bindings | 683 | replaced by generated Swift types |

So the one-time cost is roughly 3,300 lines of UI. The recurring cost is that
every feature after this lands twice — that, not the rewrite, is the real price.

### Core (`fun-asr-core`)

Everything here is platform-neutral and has no Tauri dependency:

- **ASR runtime** — llama.cpp process management, model download and integrity
- **VAD segmentation and the streaming worker** — including `FORCE_COMMIT_MS`
  and partial-refresh policy
- **Storage** — schema, migrations, sessions and transcripts, the FTS5 trigram
  index and its ≥3-character routing rule, filters, archive
- **LLM client** — OpenAI-compatible SSE streaming, formatting presets
- **Cloud ASR** — multipart `/v1/audio/transcriptions`
- **Licence** — Ed25519 verification
- **Trial metering** — VAD-second accounting
- **Settings store**

These are the parts that were expensive to get right and are worth protecting
from a rewrite. The trigram search rule alone took a measurement campaign to
find, and it is invisible to anyone reading the code.

### Platform shells

| Concern | Linux/Windows | macOS native |
|---|---|---|
| UI | Next.js in Tauri | SwiftUI |
| Hotkey | portal / evdev | `CGEventTap` in Swift |
| Text injection | clipboard + chord table | `CGEventKeyboardSetUnicodeString` |
| Window chrome | CSD, layer-shell | `NSPanel`, native glass |
| Tray | libappindicator | `NSStatusItem` |

**This deletes our most dangerous code rather than adding to it.** The macOS
paths today are 683 lines of hand-written `objc::msg_send!` — manually built
`CFDictionary`, hand-encoded `NSRect` memory layout, method names checked by
nobody. `AXIsProcessTrustedWithOptions` sat in the tree for a day before anyone
knew whether it compiled. In Swift each of these is a few lines of ordinary,
type-checked API.

## The FFI boundary

**UniFFI.** It generates the Swift bindings from the Rust definitions, so the
two sides cannot drift; a hand-written C ABI would need every type maintained
twice, which is the failure mode this whole exercise exists to avoid.

Three things about this app make the boundary non-trivial, and they should be
designed before any code moves:

**1. Everything interesting is a stream, not a call.** Transcription emits
partials then a commit; model download emits progress; LLM formatting streams
tokens. These become UniFFI callback interfaces — Swift implements a listener,
Rust calls into it. The shape to avoid is a polling API, which would put the
cadence policy on the UI side where it would immediately diverge between
platforms.

**2. Cancellation has to be part of the interface.** Push-to-talk is released
mid-utterance constantly. A handle object with an explicit `cancel()` is
required; dropping a future is not expressible across the boundary.

**3. Errors must stay actionable.** The current code returns descriptions a user
can act on ("This build has no licence key configured"), not error codes. UniFFI
error enums should carry those strings rather than reducing them to variants the
UI then has to re-translate — twice.

## Order of work

**Slice 1 — done.** Workspace, the `fun-asr-core` crate, and the three modules
with no Tauri dependency at all: `license`, `asr_cloud`, `llm`. Plus the UniFFI
scaffolding, with licence verification as the one thing crossing the boundary.

The plan originally said storage first, on the grounds that it is the most
self-contained and best-tested, so a mistake would surface immediately. It went
this way instead because three agents were editing `lib.rs` concurrently and
these three modules are separate files nobody was touching. That turned out to
be the better first cut anyway: it exercises the *unfamiliar* machinery — a
workspace, UniFFI, generated Swift — on code that is small and already tested,
rather than proving the familiar part first.

`src-tauri/src/lib.rs` moved by exactly 1 insertion and 3 deletions: three
`mod` declarations became `pub use fun_asr_core::{asr_cloud, license, llm};`.
The moved files kept their git blob hashes, so nothing was quietly rewritten.

One consequence worth knowing about, because it broke a script and no test
could have caught it: **a workspace moves cargo's output from
`src-tauri/target` to the repository root `target/`.** Anything with that path
written down has to move with it.

Remaining:

1. **Storage** — schema, queries, the FTS5 trigram index and its routing rule.
   The most tests, so mistakes surface fast.
2. **The ASR runtime and streaming worker**, where the real complexity is and
   where a stable interface matters most.
3. **Trial metering and settings.**
4. **Leave `lib.rs` as a thin Tauri shim.** If it is not obviously thin at the
   end, the boundary was drawn in the wrong place.
5. **Then SwiftUI**, against a core that already has a Swift package.

The Tauri app must keep working at every step. A refactor that requires a flag
day is a refactor that gets abandoned halfway.

## What this does not decide

Whether the SwiftUI app actually gets built. If native glass alone — with the
CSS material out of the way — turns out to look right, the second UI buys less
than it costs, and the core extraction is still a win on its own terms.

Windows has not been started. Three targets and two UI stacks with one person is
the real risk here, and it argues for finishing the Tauri shell before opening
the second front, not for abandoning it.
