"""Shared fixtures for the godot-e2e visual/behavioural test suite.

Launches the Godot project with the E2E automation server, seeds the Rust
particle RNG for reproducible screenshots, and freezes particle motion so
screenshots are deterministic. Screenshots are written to
``<repo>/artifacts/e2e/<run-id>/``; per-test performance snapshots are
appended to ``perf.csv`` in the same directory.
"""

from __future__ import annotations

import csv
import os
import time
from pathlib import Path

import pytest

from godot_e2e import GodotE2E

REPO_ROOT = Path(__file__).resolve().parents[2]
PROJECT_PATH = REPO_ROOT / "godot"
GODOT_PATH = os.environ.get("GODOT_PATH", r"C:\Godot\Godot_v4.7-stable_win64.exe")

# Deterministic particle layout for screenshot comparison.
PARTICLE_SEED = "1337"

PERF_PROBE = "/root/Main/PerfProbe"
PERF_METRICS = [
    ("fps", "get_perf_fps"),
    ("process_ms", "get_perf_process_ms"),
    ("physics_ms", "get_perf_physics_ms"),
    ("draw_calls", "get_perf_draw_calls"),
    ("primitives", "get_perf_primitives"),
    ("video_mem_mb", "get_perf_video_mem_mb"),
]


@pytest.fixture(scope="session")
def artifacts_dir() -> Path:
    run_id = time.strftime("%Y%m%d_%H%M%S")
    path = REPO_ROOT / "artifacts" / "e2e" / run_id
    path.mkdir(parents=True, exist_ok=True)
    return path


@pytest.fixture(scope="session")
def _game_process():
    os.environ["DEVIN_PARTICLE_SEED"] = PARTICLE_SEED
    os.environ["DEVIN_FREEZE_PARTICLES"] = "1"
    last_error = None
    for attempt in range(3):
        try:
            game = GodotE2E.launch(
                str(PROJECT_PATH),
                godot_path=GODOT_PATH,
                timeout=30.0,
            )
            # The launcher sets a 2 s socket timeout for the handshake; raise
            # it so slow commands (scene reload + mesh rebuild) don't fail.
            game._client._sock.settimeout(60.0)
            game.wait_for_node("/root/Main", timeout=30.0)
            game.wait_for_node("/root/Main/Artery", timeout=30.0)
            yield game
            game.close()
            return
        except Exception as exc:  # noqa: BLE001 - retry transient handshake races
            last_error = exc
            time.sleep(1.0)
    raise last_error


@pytest.fixture(scope="function")
def game(_game_process):
    _game_process.reload_scene()
    _game_process.wait_for_node("/root/Main/Artery", timeout=10.0)
    _game_process.wait_process_frames(5)
    yield _game_process


@pytest.fixture(scope="function")
def shot(game, artifacts_dir):
    def _shot(name: str) -> str:
        return game.screenshot(str(artifacts_dir / name))

    return _shot


@pytest.fixture(autouse=True)
def perf_record(request, game, artifacts_dir):
    """Appends one perf.csv row per test, sampled after the test finishes.

    Numbers are smoke-level indicators, not benchmarks: particles are frozen
    in E2E runs, so fps mostly reflects the static scene plus whatever rebuild
    work the test triggered.
    """
    yield
    try:
        values = {name: float(game.call(PERF_PROBE, method)) for name, method in PERF_METRICS}
    except Exception as exc:  # noqa: BLE001 - never mask the test's own failure
        print(f"[perf] collection failed: {exc}")
        return

    csv_path = artifacts_dir / "perf.csv"
    with csv_path.open("a", newline="") as fh:
        writer = csv.writer(fh)
        if fh.tell() == 0:
            writer.writerow(["test"] + [name for name, _ in PERF_METRICS])
        writer.writerow([request.node.name] + [f"{values[n]:.2f}" for n, _ in PERF_METRICS])

    print(f"[perf] {request.node.name}: {summary_msg(values)}")


def summary_msg(values: dict) -> str:
    return "  ".join(f"{name}={values[name]:.1f}" for name, _ in PERF_METRICS)
