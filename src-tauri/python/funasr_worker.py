#!/usr/bin/env python3
"""Small CPU-first bridge between Tauri and Fun-ASR.

The desktop app stays runnable without Python ML dependencies. When the user
installs the optional requirements, this bridge downloads the Hugging Face model
and runs CPU transcription through FunASR's AutoModel API.
"""

from __future__ import annotations

import argparse
import importlib.metadata
import json
import os
import sys
from pathlib import Path


def emit(payload: dict) -> None:
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except Exception:
        return None


def collect_gpu_info() -> dict:
    packages = ("funasr", "huggingface_hub", "vllm", "torch")
    missing: list[str] = []
    for package in packages:
        try:
            __import__(package)
        except Exception:
            missing.append(package)

    payload = {
        "ok": False,
        "missing": missing,
        "torch": package_version("torch"),
        "torch_cuda": None,
        "cuda_available": False,
        "device_count": 0,
        "device": None,
        "vllm": package_version("vllm"),
        "funasr": package_version("funasr"),
        "error": None,
    }
    if missing:
        return payload

    try:
        import torch

        payload["torch_cuda"] = getattr(torch.version, "cuda", None)
        payload["cuda_available"] = bool(torch.cuda.is_available())
        if payload["cuda_available"]:
            payload["device_count"] = int(torch.cuda.device_count())
            if payload["device_count"]:
                payload["device"] = torch.cuda.get_device_name(0)
        payload["ok"] = bool(payload["cuda_available"])
        if not payload["ok"]:
            payload["error"] = "PyTorch is installed, but CUDA is not available."
    except Exception as exc:  # noqa: BLE001 - process boundary diagnostics.
        payload["error"] = str(exc)
    return payload


def command_probe(_: argparse.Namespace) -> int:
    missing: list[str] = []
    for package in ("funasr", "huggingface_hub"):
        try:
            __import__(package)
        except Exception:
            missing.append(package)

    if missing:
        print(
            "Python is available, but missing packages: "
            + ", ".join(missing)
            + ". Install with: python3 -m pip install -r src-tauri/python/requirements.txt",
            file=sys.stderr,
        )
        return 1

    print("Python Fun-ASR bridge is ready.")
    return 0


def command_probe_vllm(_: argparse.Namespace) -> int:
    info = collect_gpu_info()
    missing = info.get("missing") or []

    if missing:
        print(
            "Python is available, but missing GPU vLLM packages: "
            + ", ".join(missing)
            + ". Install with: python3 -m pip install -r src-tauri/python/requirements-vllm.txt",
            file=sys.stderr,
        )
        return 1

    if not info["ok"]:
        print(str(info.get("error") or "vLLM packages are installed, but CUDA is not available."), file=sys.stderr)
        return 1

    print("Python Fun-ASR vLLM bridge is ready.")
    return 0


def command_gpu_info(_: argparse.Namespace) -> int:
    emit(collect_gpu_info())
    return 0


def command_ensure_model(args: argparse.Namespace) -> int:
    try:
        from huggingface_hub import snapshot_download

        local_dir = Path(args.local_dir)
        local_dir.mkdir(parents=True, exist_ok=True)
        snapshot_download(
            repo_id=args.repo,
            local_dir=str(local_dir),
            local_dir_use_symlinks=False,
            resume_download=True,
        )
        emit({"ok": True, "local_dir": str(local_dir)})
        return 0
    except Exception as exc:  # noqa: BLE001 - this is a process boundary.
        emit({"ok": False, "error": str(exc)})
        print(str(exc), file=sys.stderr)
        return 1


def command_transcribe(args: argparse.Namespace) -> int:
    try:
        from funasr import AutoModel

        model_path = Path(args.local_dir)
        model_ref = str(model_path) if model_path.exists() and any(model_path.iterdir()) else args.repo
        model = AutoModel(
            model=model_ref,
            hub="hf",
            trust_remote_code=True,
            device="cpu",
        )
        hotwords = [word.strip() for word in (args.hotwords or "").splitlines() if word.strip()]
        result = model.generate(
            input=[args.audio],
            cache={},
            batch_size=1,
            language=args.language,
            hotwords=hotwords,
            itn=True,
        )
        text = ""
        if result:
            first = result[0]
            if isinstance(first, dict):
                text = str(first.get("text", ""))
            else:
                text = str(first)
        emit({"ok": True, "text": text})
        return 0
    except Exception as exc:  # noqa: BLE001 - return error as JSON to the app.
        emit({"ok": False, "error": str(exc)})
        return 0


def command_transcribe_vllm(args: argparse.Namespace) -> int:
    try:
        os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
        try:
            from funasr.auto.auto_model_vllm import AutoModelVLLM
        except Exception:
            from funasr import AutoModelVLLM

        model_path = Path(args.local_dir)
        model_ref = str(model_path) if model_path.exists() and any(model_path.iterdir()) else args.repo
        hotwords = [word.strip() for word in (args.hotwords or "").splitlines() if word.strip()]
        vllm_kwargs = {}
        if args.max_num_seqs:
            vllm_kwargs["max_num_seqs"] = args.max_num_seqs
        model = AutoModelVLLM(
            model=model_ref,
            tensor_parallel_size=args.tensor_parallel_size,
            gpu_memory_utilization=args.gpu_memory_utilization,
            max_model_len=args.max_model_len,
            enforce_eager=args.enforce_eager,
            vllm_kwargs=vllm_kwargs,
        )
        result = model.generate(
            [args.audio],
            language=args.language,
            hotwords=hotwords,
        )
        text = ""
        if result:
            first = result[0]
            if isinstance(first, dict):
                text = str(first.get("text", ""))
            else:
                text = str(first)
        emit({"ok": True, "text": text})
        return 0
    except Exception as exc:  # noqa: BLE001 - return error as JSON to the app.
        emit({"ok": False, "error": str(exc)})
        return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Fun ASR Desktop Python bridge")
    sub = parser.add_subparsers(dest="command", required=True)

    probe = sub.add_parser("probe")
    probe.set_defaults(func=command_probe)

    probe_vllm = sub.add_parser("probe-vllm")
    probe_vllm.set_defaults(func=command_probe_vllm)

    gpu_info = sub.add_parser("gpu-info")
    gpu_info.set_defaults(func=command_gpu_info)

    ensure = sub.add_parser("ensure-model")
    ensure.add_argument("--repo", required=True)
    ensure.add_argument("--local-dir", required=True)
    ensure.set_defaults(func=command_ensure_model)

    transcribe = sub.add_parser("transcribe")
    transcribe.add_argument("--audio", required=True)
    transcribe.add_argument("--repo", required=True)
    transcribe.add_argument("--local-dir", required=True)
    transcribe.add_argument("--language", default="中文")
    transcribe.add_argument("--hotwords", default="")
    transcribe.set_defaults(func=command_transcribe)

    transcribe_vllm = sub.add_parser("transcribe-vllm")
    transcribe_vllm.add_argument("--audio", required=True)
    transcribe_vllm.add_argument("--repo", required=True)
    transcribe_vllm.add_argument("--local-dir", required=True)
    transcribe_vllm.add_argument("--language", default="中文")
    transcribe_vllm.add_argument("--hotwords", default="")
    transcribe_vllm.add_argument("--tensor-parallel-size", type=int, default=1)
    transcribe_vllm.add_argument("--gpu-memory-utilization", type=float, default=0.5)
    transcribe_vllm.add_argument("--max-model-len", type=int, default=2048)
    transcribe_vllm.add_argument("--max-num-seqs", type=int, default=1)
    transcribe_vllm.add_argument("--enforce-eager", action="store_true")
    transcribe_vllm.set_defaults(func=command_transcribe_vllm)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
