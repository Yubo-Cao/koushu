# Brief: the native macOS app

Paste this into a fresh agent session on the Mac. It is written to be
self-contained.

**Load the `apple-design` skill first and follow it.**

## What this is

`fun-asr-desktop` is a local speech-to-text app: hold a global hotkey, talk, and
the transcript lands in whatever you were typing in. It exists today as a Tauri
v2 app (Next.js in a WebKitGTK/WKWebView shell) that runs on Linux and macOS.

**The macOS build is being rewritten as a real native SwiftUI application.**
Linux and Windows keep the Tauri shell.

`macos/` already contains a working prototype of the hardest part — the floating
voice bar. Read it before writing anything; it has the answers to the questions
that cost the most time.

## Why native, stated honestly

The prototype settled this, and the case is narrower than it first looked.

**The one hard ceiling is multiple independent glass elements.** To AppKit a
webview is a single opaque rectangle, so it can sit on one sheet of glass but
its contents cannot each be glass. The prototype's bar is two separate glass
bodies — a round mic and a long pill — with their own shapes, tints (only the
mic reddens while recording) and responses, merging and separating. A webview
cannot do that at any level of effort.

**Everything else is a difference in risk, not capability.** The non-activating
panel, text injection and the tray all work in the Tauri build — through 683
lines of hand-written `objc::msg_send!` with manually encoded struct layouts.
The Swift equivalent of each is a few lines of type-checked API. That code is
the most dangerous in the repository: `AXIsProcessTrustedWithOptions` sat in it
for a day before anyone knew whether it compiled, and a wrong `0` where
`NSWindowBelow` should have been `-1` aborted the process on a path that had
never run.

**The material itself is not a generation apart.** The Tauri bar already
refracts. What native adds is morphing, per-element glass, and clean light
appearance.

So: build it for per-control glass and for deleting the dangerous code. Do not
expect the bar alone to look dramatically better than the Tauri one.

## Do not disturb the user's machine

**This Mac is the user's daily working machine, not a test rig.** They have
already been interrupted once by a prototype that sat on top of every window
and played audio through the speakers. Rules:

- The app must be **inert on launch**: no panel shown, no global hotkey
  registered, nothing audible. Those happen only on an explicit command or flag.
- **Never play sound.** `screencapture -x` — the `-x` is what silences the
  shutter.
- **Never register a global hotkey while testing.** It steals keys from whatever
  they are typing.
- **Never move, resize or focus their windows.**
- Batch visual checks into **one short run** that sets up every state, captures,
  and exits. `macos/shots.sh` does exactly this — extend it rather than starting
  the app repeatedly.
- If something cannot be verified without taking over the desktop, **report it
  as unverified**. That is the preferred outcome.

## Traps already paid for

**Ad-hoc signing invalidates every permission on rebuild.** An ad-hoc
signature's designated requirement *is* the code hash, so each rebuild is a
different program to macOS. The Accessibility checkbox stays ticked while
pointing at a `cdhash` from a build that no longer exists — it looks granted and
does nothing. `macos/sign-identity.sh` sets up a fixed self-signed certificate,
which makes the requirement `identifier + certificate leaf` and survives
rebuilds (verified across four, for the microphone). **Use it.**

**`NSMicrophoneUsageDescription` must be in Info.plist.** Without it macOS
denies recording silently, which is indistinguishable from a broken microphone.

**Icons must be 8-bit PNG.** 16-bit ones make the runtime panic with "expected
pixel count 1024, got 2048".

**Menu bar icons must be template images** — single colour plus alpha. Colour is
not information macOS will keep there.

**Do not paint your own highlights on glass.** That is exactly what made the
Tauri version look, in the user's words, over-lit and stiff. Let the system
material do all of it.

## What the app has to do

Feature parity with the Tauri build. Read the Linux/Next.js UI for behaviour,
not for structure — this is a rewrite, not a port.

- Push-to-talk with a **non-activating** floating bar (never take focus; the
  user is dictating into another app and a stolen focus means the text cannot go
  back)
- Session list with search, filters and archive
- Settings: model, language, microphone, hotkey recording, LLM formatting
  presets, BYOK endpoints
- Chinese and English UI. Translated strings exist in `lib/i18n/{en,zh}.ts` and
  carry over; the mechanism does not
- Menu bar item with idle / recording / transcribing states
- Text injection into the focused app

## The shared core

Do **not** reimplement ASR, VAD, storage, search, the LLM client or licensing in
Swift. Those live in a Rust core (`core/`, crate `fun-asr-core`) that is being
extracted behind UniFFI-generated Swift bindings. See `docs/core-extraction.md`.

Three constraints from that design apply to anything you call:

- Everything interesting is a **stream**, not a call — transcription emits
  partials then a commit, downloads emit progress, formatting streams tokens.
  These arrive as callback interfaces. Do not build a polling loop.
- **Cancellation is part of the interface.** Push-to-talk is released
  mid-utterance constantly.
- **Errors carry actionable strings**, not codes. Show them; do not re-translate
  them.

If the bindings are not ready yet, stub the core behind a Swift protocol so the
UI can be built and the real implementation dropped in later.

## Verification

Compiling proves nothing here — this project has had a dozen bugs that compiled
cleanly and failed the moment they ran. Every claim in a report must come from
something that actually ran, and anything that did not run must be named as
such.

For visual work, capture the same state against **three different backgrounds**
(bright wallpaper, dark terminal, colourful window) in **both appearances**.
Glass is entirely about how it reacts to what is behind it; one background shows
nothing.
