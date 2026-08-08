#!/usr/bin/env python3
"""模拟 run_streaming_worker：按音频时间轴逐步喂入，调真实二进制，验证分段与延迟。

复刻 lib.rs 里的常量与决策顺序。
"""
import os, struct, subprocess, sys, time, wave

B = "/home/yubo/data/project/fun-asr-desktop/src-tauri/binaries"
M = os.path.expanduser("~/.local/share/dev.yubo.fun-asr-desktop/models")
TMP = "/tmp/claude-1000/-home-yubo-data-project-fun-asr-desktop/6b1e8cfa-8248-4306-8007-ef01edc62a63/scratchpad/simtmp"
os.makedirs(TMP, exist_ok=True)

STREAM_POLL_MS = 250
SEGMENT_TAIL_SILENCE_MS = 500
VAD_MIN_SPEECH_MS = 320
PARTIAL_REFRESH_MS = 900
FORCE_COMMIT_MS = 12_000

VAD_BIN = f"{B}/llama-funasr-vad-x86_64-unknown-linux-gnu"
SV_BIN = f"{B}/llama-funasr-sensevoice-x86_64-unknown-linux-gnu"
NANO_BIN = f"{B}/llama-funasr-cli-x86_64-unknown-linux-gnu"
VAD_GGUF = f"{M}/fun-asr-nano-2512/fsmn-vad.gguf"
SV_GGUF = f"{M}/sensevoice-small/sensevoice-small-q8.gguf"
NANO_ENC = f"{M}/fun-asr-nano-2512/funasr-encoder-f16.gguf"
NANO_LLM = f"{M}/fun-asr-nano-2512/qwen3-0.6b-q4km.gguf"


def write_wav(path, samples, sr=16000):
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32767)) for s in samples))


def run_vad(wav):
    out = subprocess.run([VAD_BIN, "-m", VAD_GGUF, "-a", wav],
                         capture_output=True, text=True)
    spans = []
    for line in out.stdout.splitlines():
        p = line.split()
        if len(p) == 2 and p[0].isdigit() and p[1].isdigit():
            a, b = int(p[0]), int(p[1])
            if b > a:
                spans.append((a, b))
    return spans


def transcribe(wav, fast):
    cmd = ([SV_BIN, "-m", SV_GGUF, "-a", wav, "--vad", VAD_GGUF] if fast
           else [NANO_BIN, "--enc", NANO_ENC, "-m", NANO_LLM, "-a", wav, "--vad", VAD_GGUF])
    out = subprocess.run(cmd, capture_output=True, text=True)
    return " ".join(l.strip() for l in out.stdout.splitlines() if l.strip())


def main():
    src = sys.argv[1]
    with wave.open(src, "rb") as w:
        sr = w.getframerate(); n = w.getnframes()
        s = [v / 32768.0 for v in struct.unpack(f"<{n}h", w.readframes(n))]
    total_ms = n * 1000 // sr
    print(f"音频 {total_ms}ms @ {sr}Hz\n")

    cursor = 0                       # 已提交的样本数
    seg_index = 0
    last_partial_at = -PARTIAL_REFRESH_MS
    last_partial_text = ""
    now_ms = 0                       # 已"到达"的音频时刻
    budget_warnings = 0

    while now_ms < total_ms:
        now_ms = min(now_ms + STREAM_POLL_MS, total_ms)
        arrived = sr * now_ms // 1000
        pending = s[cursor:arrived]
        pending_ms = len(pending) * 1000 // sr
        if pending_ms < VAD_MIN_SPEECH_MS:
            continue

        pw = f"{TMP}/pending.wav"; write_wav(pw, pending, sr)
        t0 = time.time(); spans = run_vad(pw); vad_ms = (time.time() - t0) * 1000

        if not spans:
            keep = sr * SEGMENT_TAIL_SILENCE_MS // 1000
            cursor = max(cursor, arrived - keep)
            last_partial_text = ""
            continue

        a, b = spans[0]
        settled = pending_ms - b >= SEGMENT_TAIL_SILENCE_MS
        overlong = b - a >= FORCE_COMMIT_MS

        if settled or overlong:
            end_ms = b if settled else a + FORCE_COMMIT_MS
            i, j = sr * a // 1000, min(sr * end_ms // 1000, len(pending))
            if j > i and (j - i) * 1000 // sr >= VAD_MIN_SPEECH_MS:
                cw = f"{TMP}/commit.wav"; write_wav(cw, pending[i:j], sr)
                t0 = time.time(); text = transcribe(cw, fast=False)
                dt = (time.time() - t0) * 1000
                why = "静音" if settled else "满12s强制"
                print(f"[{now_ms:6d}ms] ✅ 段{seg_index} ({why}) "
                      f"音频{(j-i)*1000//sr}ms 转录{dt:.0f}ms")
                print(f"          {text[:150]}")
                seg_index += 1
            cursor += j
            last_partial_text = ""
            continue

        if now_ms - last_partial_at < PARTIAL_REFRESH_MS:
            continue
        i = sr * a // 1000
        if (len(pending) - i) * 1000 // sr < VAD_MIN_SPEECH_MS:
            continue
        pw2 = f"{TMP}/partial.wav"; write_wav(pw2, pending[i:], sr)
        t0 = time.time(); text = transcribe(pw2, fast=True)
        dt = (time.time() - t0) * 1000
        last_partial_at = now_ms
        span_ms = (len(pending) - i) * 1000 // sr
        lag = "  ⚠️跟不上" if dt > PARTIAL_REFRESH_MS else ""
        if dt > PARTIAL_REFRESH_MS:
            budget_warnings += 1
        if text != last_partial_text:
            last_partial_text = text
            print(f"[{now_ms:6d}ms] … partial 跨度{span_ms}ms vad{vad_ms:.0f}ms "
                  f"转录{dt:.0f}ms{lag}")
            print(f"          {text[:120]}")

    print(f"\n共 {seg_index} 个提交段；partial 超时 {budget_warnings} 次")


main()
