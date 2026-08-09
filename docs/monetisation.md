# Monetisation design

Local transcription is free and unlimited, forever. The paid tier is **$10 per
month for hosted cloud transcription and updates**.

## The decision, and what it costs

This design has been reversed once, deliberately, and the reversal is worth
recording because the reasoning is what matters.

It began as a metered subscription. It was then cut down to a one-time paid
binary on the grounds that *we* were not paying anyone per second, so there was
nothing to meter — remove that premise and accounts, quotas, concurrency
leases, and the server all disappear with it.

The current decision puts the premise back. A $10/month tier that includes
hosted cloud transcription means we pay the upstream provider per second of
speech, and everything that follows from that returns:

| Component | Needed? | Why |
|---|---|---|
| Server | **Yes** | Something has to hold the provider key and proxy audio |
| Accounts | **Yes** | A subscription has to be attached to somebody |
| Usage metering | **Yes** | We are billed per second; unmetered is unbounded |
| Concurrency control | **Yes** | One subscription, one active stream |
| Stripe webhooks | **Yes** | Subscription lifecycle: created, updated, deleted, payment failed |
| Data-processor role | **Yes** | Audio now passes through our infrastructure |

That last row is the one to read twice. The strongest sentence this project
could previously say was **"audio never reaches us."** For Pro subscribers that
stops being true. Everyone else — local and BYOK — keeps it, so the claim has
to be scoped per tier rather than made for the product.

One thing genuinely improves, though. The 120-minute trial was previously
enforced in an open-source client, which the document below admits is advisory
at best. Metering cloud minutes happens on our side, so for the first time the
limit is real without anything being obfuscated.

## Unit economics

The tier is solvent or not depending entirely on which provider backs it. The
spread is nearly an order of magnitude, so this is the decision, not a detail.

Net of Stripe's 2.9% + $0.30, a $10 subscription yields **$9.41**.

| Provider / model | Rate | Break-even speech per month |
|---|---|---|
| Groq `whisper-large-v3-turbo` | $0.04 / hr | **235 hr** |
| Groq `whisper-large-v3` | $0.111 / hr | **85 hr** |
| OpenAI `whisper-1` (list) | $0.36 / hr | **26 hr** |

Heavy daily dictation runs 1–2 hours of *speech* per day, so 30–60 hours per
month. Against that:

- `turbo` leaves 4–8× headroom. Comfortable.
- `large-v3` leaves 1.4–2.8×. Workable, thin.
- OpenAI at list price is **loss-making** for exactly the users who like the
  product most.

Two things make this more predictable than it looks. Speech is measured by VAD
segments, not wall-clock capture, so thinking with the key held down costs
nothing. And the local model stays available at all times, so a fair-use ceiling
degrades to free local transcription rather than to a dead app.

A ceiling is still required even on `turbo`: one person batch-transcribing an
archive of recorded meetings can reach 235 hours without doing anything
abusive. Rates above are current as of 2026-08 and should be re-checked before
launch — the arithmetic, not the numbers, is the durable part.

## The three ways to use it

**Local** — unlimited, free, offline, no account. Fun-ASR-Nano runs on the CPU
at roughly 8.8× realtime. This is the whole product for most people, and
weakening it to manufacture an upgrade prompt would trade away the best
property this project has.

**BYOK** — the user supplies an OpenAI-compatible endpoint and key, and pays
their provider directly. Audio goes from their machine to their provider and
**never touches our infrastructure**, so we take on no processing role for it.
This stays free and is not a degraded tier: someone who already has a Groq key
gets the same accuracy a subscriber does.

**Pro, $10/month** — hosted cloud transcription with no key to obtain, plus
signed and notarised builds and automatic updates.

Measured difference between local and cloud, on a code-switched technical clip:

| | Latency | Result |
|---|---|---|
| Local Fun-ASR-Nano | 4122 ms | `we have a c h d … one in the uh boundary line` |
| Cloud whisper-large-v3-turbo | 800 ms | `we have SVGD … the way that DAPS adds actual noise` |

Domain terms are where the hosted model wins outright. That is the honest reason
to pay, and it should be stated as a difference in results rather than as a
paywall.

## What the subscription actually buys

Not features. Every feature is in the free build, and the source is public and
buildable by anyone. What is bought is:

- **A working cloud endpoint with no key to obtain.** The friction of signing up
  for a provider, holding a key, and topping up a balance is the whole product
  for people who do not want to think about it.
- **Signed and notarised macOS `.app`** — no Gatekeeper warning, no `xattr`
  incantation.
- **Prebuilt Linux packages**, tested on the target distributions.
- **Automatic updates.**

The build friction is real and recurring, which is what makes it worth paying to
skip. On Linux alone this project has hit: WebKitGTK's DMA-BUF renderer aborting
window creation, 16-bit PNGs that Tauri refuses, and `linuxdeploy` failing so
AppImage packaging left behind a binary patched for a bundle it never produced.

**The client still does not phone home for licensing.** Cloud transcription
obviously requires the network; nothing else does. A build that checked in to
run would be worse than the free one.

## Trial: 120 minutes of speech

New users get **120 minutes of cloud transcription** before the tier asks for a
subscription. Local transcription is never metered.

**Minutes, not words.** Word counts do not survive contact with this app's
users: Chinese has no word boundary, so 2000 "words" means something entirely
different in Chinese than in English. Speech seconds are language-neutral, we
already measure them with VAD, and they line up exactly with what we are billed.

**Speech, not recording time.** Holding the key while thinking should not consume
the trial, so the VAD segment total is what counts, not capture length.

**One dimension, not two.** A "7 days *or* N minutes, whichever first" trial puts
most people into a wall on day one — daily dictation of 20–60 minutes is
ordinary, and English speech runs 130–150 words per minute, so a 2000-word cap is
13–15 minutes: gone in a single session. The block would land before the habit
forms, which is precisely before the product has proved anything.

## Offline licence verification

Ed25519 signing is implemented (`src-tauri/src/license.rs`) and stays, because it
is the right mechanism for anything that must work without a network: a licence
is a signed statement verified against a public key compiled into the client.

It does not, however, gate cloud access — that is authenticated against the
server that proxies the audio, since the server is being billed. The two
mechanisms answer different questions and both are needed:

| | Verifies | Works offline |
|---|---|---|
| Ed25519 licence | This build was paid for | Yes |
| Subscription auth | This request may spend our money | No, by definition |

## Tip jar

Three one-time Stripe prices — $3 / $5 / $10 — reachable from the About panel
and the README. Unlocks nothing, tracks nothing. Stripe hosts the whole flow.

## What has to be built

Roughly in dependency order. None of this exists yet.

1. **Auth.** Apple and Google SSO only; no email-and-password to store or leak.
2. **A proxy** that holds the provider key, checks the subscription, meters
   speech seconds, and streams `/v1/audio/transcriptions` through.
3. **Stripe webhooks** for subscription lifecycle, with idempotency.
4. **Concurrency control.** One subscription, one active stream — this is the
   abuse boundary that matters, since a shared account is otherwise unbounded.
5. **A privacy policy and data-processing terms**, scoped to Pro. Audio retention
   should be *zero* — proxy and discard — which makes the policy short and the
   claim easy to keep.

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

Worth noting how this was caught: the protocol request succeeds and the app logs
"background blur enabled", which is true and useless — it reports that the
compositor accepted the request, not that anything was blurred. It took measuring
pixels behind the bar to see that an edge was tinted rather than blurred. A log
line that can only say "the call returned" should not be phrased as if it
confirms an outcome.

## Delivery

**Stripe Checkout with a fulfilment link.** Stripe emails a one-time, expiring
download URL; a static object store with signed URLs is enough.

Alternatives considered:

- **GitHub Sponsors + private release repo.** Zero infrastructure, but requires a
  GitHub account and manual access management.
- **App Store / Flathub paid listings.** Best trust and distribution, worst fees
  and review overhead. **The Mac App Store is no longer ruled out**: the
  translucent panel is now built on public `NSGlassEffectView` /
  `NSVisualEffectView` rather than Tauri's `macOSPrivateApi`, so the private-API
  blocker that previously barred submission is gone.
- **Honour system.** Publish the binaries openly, ask for payment. Lowest
  friction, and shipping is not conditional on collecting.

Links will be shared regardless. Price so that paying is easier than not paying,
and treat leakage as marketing rather than loss.

## Open questions

- **Which provider backs Pro.** See the arithmetic above; this decides whether
  the tier is solvent, and it is the only question that does.
- **Where the fair-use ceiling sits**, and what the app says when it is reached.
  Falling back to local silently is friendlier than an error, but silently
  changing which model produced a transcript is dishonest — it needs to be
  visible.
- **Annual pricing.** A discounted year reduces churn and payment-failure
  handling, at the cost of a longer refund tail.
- Whether the free build is signed too. Signing both is friendlier and reduces
  the paid build to convenience alone.
