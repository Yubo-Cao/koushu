# Monetisation design

How this open-source app asks for money without acquiring a backend.

## The decision

Sell **convenience and goodwill**, not metered capacity. Concretely: a paid
prebuilt binary, and a tip jar. Cloud transcription stays bring-your-own-key.

This started as a subscription design — accounts, quotas, concurrency leases,
abuse limits, a VPS. Every one of those existed for a single reason: *we* would
have been paying the upstream provider per second, so usage had to be
authenticated, metered, and defended. Remove that one premise and the entire
apparatus disappears with it.

| Component | Metered subscription | This design |
|---|---|---|
| Server | Required | **None** |
| Accounts, SSO | Required | **None** |
| Usage metering | Required | **None** |
| Concurrency control | Required | **None** |
| Abuse prevention | Required | **None** |
| Data-processor obligations | Required | **None** — audio never reaches us |
| Single point of failure | Yes | **No** |
| Stripe surface | Checkout + Portal + 5 webhooks + idempotency | **One Payment Link** |

The payment justification is also stronger. A subscription has to keep proving
its worth every month. A one-time purchase only has to be worth it once, at the
moment of download.

## Why a client-side limit was rejected

The earlier plan capped free-tier local transcription by word count. That is
not enforceable here and would have created false confidence.

The client is open source. A limit living in it is deleted and rebuilt in
minutes — often without writing any code, since a configuration-driven check
just needs the config changed. Anything the client alone enforces is advisory.

Unlimited local use is also the strongest on-ramp this project has: free,
offline, private, no key required. Weakening it to create an upgrade prompt
would trade the product's best property for a limit that does not hold.

## The three ways to use it

**Local** — unlimited, free, offline, no account. Fun-ASR-Nano runs on the CPU
at roughly 8.8× realtime. This is the whole product for most people.

**BYOK** — the user supplies an OpenAI-compatible endpoint and key. Better
accuracy on hard audio; they pay their provider directly. Audio goes from their
machine to their provider. **It never touches our infrastructure**, which is
why we take on no processing role and no privacy-policy obligation for it.

**Paid build** — a signed, notarised, prebuilt binary with auto-updates. The
source stays public and buildable by anyone. What is bought is not a feature
but the removal of work.

Measured difference between local and cloud, on a code-switched technical clip:

| | Latency | Result |
|---|---|---|
| Local Fun-ASR-Nano | 4122 ms | `we have a c h d … one in the uh boundary line` |
| Cloud whisper-large-v3-turbo | 800 ms | `we have SVGD … the way that DAPS adds actual noise` |

Domain terms are where the hosted model wins outright. Worth stating on the
download page as the honest reason to configure BYOK — not as a paywall.

## What the paid build actually buys

The source is free; building it is not effortless. On Linux alone this project
has already hit: WebKitGTK's DMA-BUF renderer aborting window creation,
16-bit PNGs that Tauri refuses, `linuxdeploy` failing so AppImage packaging
leaves behind a binary patched for a bundle it never produced. macOS adds
codesigning and notarisation. That friction is real, recurring, and worth
paying to skip.

So the paid build offers:

- Signed and notarised macOS `.app` — no Gatekeeper warning, no `xattr`
  incantation
- Prebuilt Linux packages, tested on the target distributions
- Automatic updates
- Priority on bug reports

And it never withholds: no feature is locked, no telemetry, no licence check at
runtime. **A paid build that phones home is a worse product than the free one**,
and it would undercut the offline guarantee that makes this worth using.

## Trial: 120 minutes of speech

The free build is fully functional for **120 minutes of transcribed speech**,
with no time limit. After that it asks for a licence.

**Minutes, not words.** Word counts do not survive contact with this app's
users: Chinese has no word boundary, so 2000 "words" means something entirely
different in Chinese than in English. Speech seconds are language-neutral, we
already measure them with VAD, and they line up with cost if a managed tier
ever exists.

**Speech, not recording time.** Holding the key while thinking should not
consume the trial, so the VAD segment total is what counts, not wall-clock
capture length.

**One dimension, not two.** A "7 days *or* N minutes, whichever first" trial
puts most people into a wall on day one — daily dictation of 20–60 minutes is
ordinary, and English speech runs 130–150 words per minute, so a 2000-word cap
is 13–15 minutes: gone in a single session. The block would land before the
habit forms, which is precisely before the product has proved anything. 120
minutes with no clock lets people arrive at the limit at their own pace, by
which point they know whether they want it.

### Activation is offline

Ed25519. A licence is signed with our private key and verified against a public
key compiled into the client. **No server, no network check, no phoning home.**

An online licence check would make the paid build worse than the free one and
would break the offline guarantee that justifies the app existing. It is also
unnecessary: anyone able to patch out a signature check can already build from
source, and that person was never going to buy.

### It is honest about being bypassable

The trial counter lives in the client and can be removed by rebuilding. That is
stated plainly rather than obfuscated. What is being sold is not access — the
source is right there — but not having to do the work. Obfuscating a limit that
cannot hold only makes the first serious interaction with the codebase an
adversarial one.

## Tip jar

A Stripe Payment Link in the About panel and the README. No integration: the
link opens in the browser, Stripe hosts everything, nothing is unlocked and
nothing is tracked.

## Desktop blur on Linux, without depending on a plugin

The bar asks for compositor blur through `ext_background_effect_manager_v1`, a
cross-compositor staging protocol. On KDE this is consumed by KWin's **built-in**
Blur effect, which is enabled by default — so on a stock KDE install the bar is
blurred with no configuration at all.

It does not work when the built-in effect has been replaced. A machine running
`kwin-effects-better-blur-dx` with `blurEnabled=false` will bind the protocol
successfully and blur nothing, because the effect that would have consumed the
request is switched off. Those forks match windows by class instead, so the
window class has to be added to their allow-list by hand.

Worth noting how this was caught: the protocol request succeeds and the app
logs "background blur enabled", which is true and useless — it reports that the
compositor accepted the request, not that anything was blurred. It took
measuring pixels behind the bar to see that an edge was tinted rather than
blurred. A log line that can only say "the call returned" should not be phrased
as if it confirms an outcome.

## Delivery

The one real problem to solve: how a buyer gets the binary, and how that is not
trivially shareable.

**Recommended — Stripe Checkout with a fulfilment link.** Stripe emails a
one-time, expiring download URL. A static object store with signed URLs is
enough; no server, no accounts.

Alternatives considered:

- **GitHub Sponsors + private release repo.** Zero infrastructure, but requires
  a GitHub account and manual access management.
- **App Store / Flathub paid listings.** Best trust and distribution, worst
  fees and review overhead. Note that the macOS build enables
  `macOSPrivateApi` for the translucent overlay, which **bars it from the Mac
  App Store** — this is a real constraint, not a preference.
- **Honour system.** Publish the binaries openly, ask for payment. Lowest
  friction, and shipping is not conditional on collecting.

Links will be shared regardless. Price it so that paying is easier than not
paying, and treat leakage as marketing rather than loss.

## What this does not preclude

If a managed cloud tier ever becomes worth it — because enough users want cloud
accuracy without holding a key — the earlier design is recoverable from git
history. The order matters, though: it should only be built once demand is
demonstrated, not in anticipation of it. Its entire cost sits in the parts that
this design deletes.

## Open questions

- Price. A coffee is roughly $5; a tool used daily supports more. One-time or
  per-major-version?
- Does the free build stay unsigned on macOS, or is it signed too and only
  packaging/updates are paid? Signing both is friendlier and reduces the paid
  build to convenience alone.
- Should BYOK setup ship a one-click Groq or OpenRouter preset? It reduces the
  main friction of the free path and costs us nothing.
