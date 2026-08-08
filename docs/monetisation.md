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

## Tip jar

A Stripe Payment Link in the About panel and the README. No integration: the
link opens in the browser, Stripe hosts everything, nothing is unlocked and
nothing is tracked.

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
