"""Visual + behavioural smoke tests for the artery-stenosis app.

Each test drives the running Godot app through the E2E automation server,
asserts on Rust-side hemodynamic state, and captures screenshots.

Two screenshot tiers exist per run:

- ``cmp_*.png``  — canonical, deterministic views compared against the
  committed baselines in ``tests/e2e/baselines/`` (regression gate).
- ``insp_*.png`` — extra angles/zooms captured for visual inspection only;
  never compared against baselines.
"""

from __future__ import annotations

import math

ARTERY = "/root/Main/Artery"
CAMERA = "/root/Main/Camera3D"
PLAQUE_SLIDER = (
    "/root/Main/UI/UIRoot/BottomPanel/Margin/VBox/PlaqueRow/PlaqueSlider"
)
LIFESTYLE_SLIDER = (
    "/root/Main/UI/UIRoot/BottomPanel/Margin/VBox/LifestyleRow/LifestyleSlider"
)
SMOKING_SLIDER = (
    "/root/Main/UI/UIRoot/BottomPanel/Margin/VBox/SmokingRow/SmokingSlider"
)
SMOKING_YEARS_SLIDER = (
    "/root/Main/UI/UIRoot/BottomPanel/Margin/VBox/SmokingYearsRow/SmokingYearsSlider"
)
TOGGLE_BUTTON = "/root/Main/UI/UIRoot/ExplanationUI/ExplanationToggle"
EXPLANATION_PANEL = "/root/Main/UI/UIRoot/ExplanationUI/ExplanationPanel"

# Godot MouseButton index for the mouse wheel (zoom notches).
MOUSE_BUTTON_WHEEL_UP = 4


def _pose_camera(game, yaw=None, pitch=None, distance=None):
    """Deterministically pose the free-orbit camera and apply it."""
    if yaw is not None:
        game.set_property(CAMERA, "yaw", yaw)
    if pitch is not None:
        game.set_property(CAMERA, "pitch", pitch)
    if distance is not None:
        game.set_property(CAMERA, "distance", distance)
    game.call(CAMERA, "_update_camera")


def test_default_view(game, shot):
    """Canonical default view: scene defaults, default camera pose."""
    game.wait_process_frames(10)
    shot("cmp_01_default.png")


def test_plaque_sweep(game, shot):
    """Sweep plaque accumulation; hemodynamics must respond monotonically."""
    game.set_property(PLAQUE_SLIDER, "value", 0.0)
    game.wait_process_frames(5)
    inlet_low = game.call(ARTERY, "get_inlet_pressure_mmhg")
    shot("insp_plaque_00.png")

    game.set_property(PLAQUE_SLIDER, "value", 50.0)
    game.wait_process_frames(5)
    inlet_mid = game.call(ARTERY, "get_inlet_pressure_mmhg")
    shot("insp_plaque_50.png")

    game.set_property(PLAQUE_SLIDER, "value", 100.0)
    game.wait_process_frames(5)
    inlet_high = game.call(ARTERY, "get_inlet_pressure_mmhg")
    effective = game.call(ARTERY, "get_effective_plaque_percent")

    assert effective >= 100.0, f"effective plaque was {effective}"
    assert inlet_low < inlet_mid < inlet_high, (
        f"inlet pressure not monotonic: {inlet_low} -> {inlet_mid} -> {inlet_high}"
    )
    assert inlet_high > 100.0, f"inlet pressure did not rise: {inlet_high}"
    shot("cmp_02_plaque_max.png")


def test_lifestyle_slider_adds_plaque(game, shot):
    game.set_property(PLAQUE_SLIDER, "value", 0.0)
    game.set_property(LIFESTYLE_SLIDER, "value", 4.0)
    game.wait_process_frames(5)

    effective = game.call(ARTERY, "get_effective_plaque_percent")
    lifestyle = game.call(ARTERY, "get_lifestyle_plaque_percent")
    assert abs(effective - lifestyle) < 0.5, (
        f"effective {effective} != lifestyle {lifestyle}"
    )
    shot("cmp_03_lifestyle_healthy.png")


def test_smoking_dose_response(game, shot):
    """Smoking dose-response: heavier smoking adds plaque; fewer smoking
    years inside the 10-year window reduce it."""
    game.set_property(PLAQUE_SLIDER, "value", 0.0)
    game.set_property(LIFESTYLE_SLIDER, "value", 4.0)
    game.set_property(SMOKING_SLIDER, "value", 0.0)
    game.set_property(SMOKING_YEARS_SLIDER, "value", 10.0)
    game.wait_process_frames(5)
    shot("insp_smoking_00.png")

    game.set_property(SMOKING_SLIDER, "value", 20.0)
    game.wait_process_frames(5)
    pack_a_day = game.call(ARTERY, "get_smoking_plaque_percent")
    shot("insp_smoking_20.png")

    game.set_property(SMOKING_SLIDER, "value", 40.0)
    game.wait_process_frames(5)
    two_packs = game.call(ARTERY, "get_smoking_plaque_percent")
    assert pack_a_day < two_packs, (
        f"smoking dose-response not increasing: {pack_a_day} -> {two_packs}"
    )
    shot("cmp_07_smoking_max.png")

    # Zero years of smoking inside the window = never-smoker-equivalent.
    game.set_property(SMOKING_YEARS_SLIDER, "value", 0.0)
    game.wait_process_frames(5)
    no_years = game.call(ARTERY, "get_smoking_plaque_percent")
    assert no_years < 0.5, (
        f"0 smoking years should leave no contribution, got {no_years}"
    )
    shot("insp_smoking_quit.png")


def test_slider_groups_are_exclusive(game, shot):
    """Adjusting any slider makes its group the sole plaque source: the other
    groups' contributions are excluded from the effective percentage."""
    # Smoking group active: effective == smoking contribution alone, even
    # though the lifestyle slider still reads 4 (25%).
    game.set_property(PLAQUE_SLIDER, "value", 0.0)
    game.set_property(LIFESTYLE_SLIDER, "value", 4.0)
    game.set_property(SMOKING_SLIDER, "value", 20.0)
    game.set_property(SMOKING_YEARS_SLIDER, "value", 10.0)
    game.wait_process_frames(5)
    effective = game.call(ARTERY, "get_effective_plaque_percent")
    smoking = game.call(ARTERY, "get_smoking_plaque_percent")
    assert abs(effective - smoking) < 0.5, (
        f"smoking not exclusive: effective {effective} vs smoking {smoking}"
    )
    shot("insp_exclusive_smoking.png")

    # Touching the plaque slider switches the exclusive source to direct.
    game.set_property(PLAQUE_SLIDER, "value", 30.0)
    game.wait_process_frames(5)
    effective = game.call(ARTERY, "get_effective_plaque_percent")
    assert abs(effective - 30.0) < 0.5, (
        f"direct plaque not exclusive: effective was {effective}"
    )
    shot("insp_exclusive_direct.png")

    # Touching the lifestyle slider switches again: effective == lifestyle.
    game.set_property(LIFESTYLE_SLIDER, "value", 2.0)
    game.wait_process_frames(5)
    effective = game.call(ARTERY, "get_effective_plaque_percent")
    lifestyle = game.call(ARTERY, "get_lifestyle_plaque_percent")
    assert abs(effective - lifestyle) < 0.5, (
        f"lifestyle not exclusive: effective {effective} vs lifestyle {lifestyle}"
    )
    shot("insp_exclusive_lifestyle.png")


def test_camera_angles(game, shot):
    """Orbit around the artery at several yaw/pitch combinations."""
    poses = [
        ("insp_orbit_yaw_left.png", 0.9, -0.2, None),
        ("insp_orbit_yaw_right.png", -0.9, -0.2, None),
        ("insp_orbit_pitch_high.png", 0.0, 0.6, None),
        ("insp_orbit_pitch_low.png", 0.0, -1.2, None),
        ("cmp_04_orbit_side.png", math.pi / 2, -0.2, None),
    ]
    for name, yaw, pitch, distance in poses:
        _pose_camera(game, yaw=yaw, pitch=pitch, distance=distance)
        game.wait_process_frames(5)
        shot(name)


def test_mouse_drag_orbit(game, shot):
    """Exercise the real input path: a left-drag must rotate the camera."""
    game.input_mouse_button(960, 540, 1, True)
    game.input_mouse_motion(1020, 540, 60, 0)
    game.input_mouse_button(1020, 540, 1, False)
    game.wait_process_frames(5)
    shot("insp_mouse_orbit.png")


def test_zoom_levels(game, shot):
    """Zoom via the camera property and via simulated mouse-wheel events."""
    _pose_camera(game, distance=3.0)
    game.wait_process_frames(5)
    shot("cmp_05_zoom_close.png")

    _pose_camera(game, distance=12.0)
    game.wait_process_frames(5)
    shot("insp_zoom_far.png")

    # Wheel path: three notches in must reduce the orbit distance.
    before = game.get_property(CAMERA, "distance")
    for _ in range(3):
        game.input_mouse_button(960, 540, MOUSE_BUTTON_WHEEL_UP, True)
        game.input_mouse_button(960, 540, MOUSE_BUTTON_WHEEL_UP, False)
    game.wait_process_frames(5)
    after = game.get_property(CAMERA, "distance")
    assert after < before, f"wheel zoom did not move closer: {before} -> {after}"
    shot("insp_wheel_zoom.png")


def test_slider_sweep_rebuild_cost(game, shot):
    """Sweep the plaque slider across its full range so perf.csv captures
    rebuild-dominated rows (mesh rebuilds happen on every real change)."""
    for value in (0.0, 25.0, 50.0, 75.0, 100.0, 0.0):
        game.set_property(PLAQUE_SLIDER, "value", value)
        game.wait_process_frames(2)
    shot("insp_slider_sweep_end.png")


def test_explanation_toggle(game, shot):
    shot("insp_panel_visible.png")
    game.click_node(TOGGLE_BUTTON)
    game.wait_process_frames(5)
    visible = game.get_property(EXPLANATION_PANEL, "visible")
    assert visible is False, "explanation panel should hide after toggle"
    shot("cmp_06_panel_hidden.png")
