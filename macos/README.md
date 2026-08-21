# The native macOS app

SwiftUI, replacing the Tauri shell on macOS only. Linux and Windows keep the
webview build; both talk to the same Rust core in `core/`.

Read `AGENT_BRIEF.md` first — it says why this exists and, more importantly,
which mistakes have already been paid for.

## Layout

```
Sources/
  KoushuCore/        platform-neutral Swift: the domain model, the protocols the
                     Rust core will implement, in-memory stubs for the parts of
                     it that have not been extracted yet, the bilingual string
                     catalogue, the chord grammar
  Koushu/            the app
    App/             entry point, delegate, window and menu-bar management,
                     the utterance controller, the control channel
    Platform/        the things that are deliberately *not* in the core: the
                     event tap, text injection, the microphone, screenshot
                     backdrops
    VoiceBar/        the floating non-activating panel
    Main/            the main window
    SettingsUI/      the settings window
  KoushuRustCore/    adapters onto the UniFFI-generated bindings.
                     Compiled only when the bindings have been generated.
  KoushuCoreFFI/     generated C header + modulemap. Entirely gitignored.
Tests/KoushuCoreTests/
```

## Building

```bash
./build.sh              # stubs only — every window works, transcription does not
CORE=1 ./build.sh       # regenerate and link the Rust core
swift test              # the logic that has no pixels
```

`Package.swift` decides whether to compile `KoushuRustCore` by looking for the
staged generated files, so a fresh clone builds with no extra steps and no flag
to remember. `build.sh CORE=1` is what stages them.

`DEST=` chooses where the `.app` goes; it defaults to `~/Applications`.

**Do not switch to ad-hoc signing.** `sign-identity.sh` exists because an ad-hoc
signature's designated requirement *is* the code hash, so every rebuild is a
different program to macOS and every Accessibility and Microphone grant silently
stops applying — while the checkbox in System Settings stays ticked. The
self-signed identity makes the requirement `identifier + certificate leaf`,
which survives rebuilds.

## What is real and what is not

| | State |
|---|---|
| Voice bar, panel behaviour, glass, morph | real |
| Microphone capture and the level meter | real |
| Menu-bar item, its three states, its menu | real |
| **Sessions, transcripts, search, filters, archive** | **real** — the same SQLite the Tauri build uses |
| Model download | not wired up; says so instead of hanging at 0% |
| LLM formatting | streams, but formats nothing |
| Licence verification | **real**, through UniFFI into `koushu-core` |
| **Transcription** | **real** — Fun-ASR-Nano / SenseVoice, on the CPU, locally |


Transcription runs the official llama.cpp runtimes bundled in
`Contents/Resources/binaries`, against the models in the shared data directory.
Recording, resampling and WAV encoding go through `koushu-core::audio`, so this
app and the Tauri build hand the runtime byte-identical input.

Storage goes through `koushu-core::storage`, which opens the **same database
file** as the Tauri build — one product, one history, one set of models. The
trigram index and its three-character routing rule live there and are not
reimplemented on this side; `RustStore.swift` is translation only.

Still stubbed: model downloading, LLM formatting, and trial metering. Each is a
`Stub…` type in `KoushuCore/StubCore.swift` waiting for its slice.

A build made **without** `CORE=1` transcribes placeholder text that says so in
its own words, in both languages, and the About panel repeats it. A screenshot of
that build must not be mistakable for a screenshot of a working one.

As each slice of `docs/core-extraction.md` lands, the corresponding `Stub…` type
in `KoushuCore/StubCore.swift` is replaced by an adapter in `KoushuRustCore`.
Nothing above the protocols in `CoreServices.swift` changes.

## Driving it without a desktop

The app polls `~/.funasr-bar-cmd` for commands and writes `~/.funasr-bar-status`.
This is how it is checked over SSH, and how `shots.sh` puts it into each state.

```
show | hide | main | settings          surfaces
record | stop | idle                   an utterance (see the warning below)
transcribe <path.wav>                  decode a file through the real engine
retain 0|1                             keep the recording after decoding it
search <text> | filter archived|all|none
locale zh|en | appearance dark|light
backdrop light|terminal|color [below]  screenshot backdrops
license <key>                          straight through the FFI
ax-prompt | ax-recheck | mic-recheck   permissions
set <tunable> <value> | pos <inset>    live look tuning
quit
```

**`record` from this channel never types anything into another application.**
Only a real push-to-talk press does. That distinction was added after a scripted
record/stop cycle posted 47 characters of placeholder text into the user's chat
window; the injection code was correct, the caller had no business calling it.

Launch flags are the same idea: a bare launch shows nothing, grabs no keys and
opens no microphone. `--menubar`, `--hotkey`, `--show`, `--main`, `--settings`
and `--mic` each turn on exactly one of those.

## Verifying visual work

`shots.sh` is one batched run: it launches, captures every surface in every state
against three backdrops in both appearances, and quits. Extend it rather than
starting the app repeatedly — the machine this runs on is somebody's working
desktop.

It needs **Screen Recording** permission for whatever runs it (Terminal, or your
editor). Without it `screencapture` fails with `could not create image from
display` and produces nothing at all, which is easy to mistake for the app
having failed to appear.
