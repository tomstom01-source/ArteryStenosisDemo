extends Control
## Toggleable 2D side panels: the scientific explanation panel (right) and the
## model-assumptions info box (top left), which lists every calibration choice
## in the model that lacks direct study grounding. Each panel is clickable to
## hide it; persistent buttons toggle them.

@onready var toggle_button: Button = $ExplanationToggle
@onready var panel: PanelContainer = $ExplanationPanel
@onready var explanation_text: RichTextLabel = $ExplanationPanel/Margin/ExplanationText
@onready var assumptions_button: Button = $AssumptionsToggle
@onready var assumptions_panel: PanelContainer = $AssumptionsPanel
@onready var assumptions_text: RichTextLabel = $AssumptionsPanel/Margin/AssumptionsText

## Scientific explanation shown in the right-hand panel. Edit here; the panel
## renders it as BBCode (no texture regeneration needed).
const EXPLANATION_TEXT := """[color=#8fd6ff][b]Lifestyle & smoking: plaque over 10 years[/b][/color]

[b]MESA[/b] (Ahmed et al., 2013): each of four healthy habits — not smoking, healthy weight, exercise, good diet — slows calcium build-up in the artery wall. Modelled here as 45% plaque per decade at the worst score (0), 25% at the best (4).

[b]CARDIA[/b] (Pletcher et al., 2006): in 1,535 young adults, calcification odds rose ≈ 1.27× per 10 pack-years.

[b]PDAY[/b] (1990/1999): autopsy evidence that smoking also drives early, non-calcified plaque invisible to CT — applied as a ×1.5 correction on the CARDIA slope.

[b]Exclusive sliders:[/b] each slider isolates its own effect; the others contribute zero.

[b]Model[/b]
smoking plaque ≈ 45% × (1.27^(pack-years/10) − 1) × 1.5
pack-years = (cigarettes/day ÷ 20) × years of smoking"""

## Every arbitrary/calibrated choice in the model that lacks exact study
## grounding, so the educational scope stays transparent. Study-derived
## numbers (CARDIA odds ratio, MESA slowdowns) are deliberately NOT listed.
const ASSUMPTIONS_TEXT := """[color=#8fd6ff][b]Calibration choices (not directly study-grounded)[/b][/color]

[b]MESA anchor: 25 AU/yr[/b] — MESA reported only relative slowdowns; the absolute rate is anchored to MESA's measured cohort mean (23.9 AU/yr). MESA followed ages 44-84, older than this app's target audience.

[b]0.18% plaque per Agatston unit[/b] — MESA measures calcium, not stenosis; this conversion to % plaque is a calibration, not a study quantity.

[b]×1.5 smoking correction[/b] — PDAY shows smoking drives non-calcified plaque CT cannot see; the magnitude is a conservative choice, not a measured ratio.

[b]Excess-only display[/b] — smoking shows only its attributable excess (10 pack-years → 18%); background accumulation is excluded.

[b]Stenosis shape & severity mapping[/b] — middle-40% lesion with a flat throat; 100% plaque = 75% radius reduction (Doppler-motivated). Both are conventions.

[b]10-year window[/b] — exposure before the window is not modelled; formed plaque persists.

[b]No baseline plaque[/b] — universal "wear and tear" is excluded so each habit's effect stays isolated.

[color=#8fd6ff][b]Visual simplifications (presentation only)[/b][/color]

[b]Smooth hemodynamics vs lumpy visuals[/b] — deposit bumps deform the visible channel; the flow model stays mathematically smooth.

[b]Visibility aids[/b] — translucent wall/blood, exaggerated WBC/platelet counts, 30× slowed playback; relative speeds preserved.

[b]Vessel & tissue simplifications[/b] — single straight segment (no branching, pulsatility, compliance, or turbulence; Newtonian blood); uniform lipid-yellow plaque, no calcified/fibrous components."""


func _ready() -> void:
	toggle_button.pressed.connect(_on_toggle_pressed)
	panel.gui_input.connect(_on_panel_gui_input)
	assumptions_button.pressed.connect(_on_assumptions_toggle_pressed)
	assumptions_panel.gui_input.connect(_on_assumptions_gui_input)
	_setup_panel_style(panel)
	_setup_panel_style(assumptions_panel)
	_setup_button_style(toggle_button)
	_setup_button_style(assumptions_button)
	for label in [explanation_text, assumptions_text]:
		label.add_theme_font_size_override("normal_font_size", 14)
		label.add_theme_font_size_override("bold_font_size", 16)
		label.add_theme_color_override("default_color", Color(0.92, 0.92, 0.92, 1.0))
	explanation_text.text = EXPLANATION_TEXT
	assumptions_text.text = ASSUMPTIONS_TEXT
	_update_toggle_texts()


func _setup_panel_style(target: PanelContainer) -> void:
	var panel_style := StyleBoxFlat.new()
	panel_style.bg_color = Color(0.06, 0.07, 0.09, 0.88)
	panel_style.border_color = Color(0.22, 0.24, 0.29, 0.9)
	panel_style.border_width_left = 2
	panel_style.border_width_top = 2
	panel_style.border_width_right = 2
	panel_style.border_width_bottom = 2
	panel_style.corner_radius_top_left = 16
	panel_style.corner_radius_top_right = 16
	panel_style.corner_radius_bottom_left = 16
	panel_style.corner_radius_bottom_right = 16
	target.add_theme_stylebox_override("panel", panel_style)


func _setup_button_style(target: Button) -> void:
	var normal := StyleBoxFlat.new()
	normal.bg_color = Color(0.15, 0.16, 0.20, 0.92)
	normal.border_color = Color(0.30, 0.32, 0.38, 1.0)
	normal.border_width_left = 2
	normal.border_width_top = 2
	normal.border_width_right = 2
	normal.border_width_bottom = 2
	normal.corner_radius_top_left = 8
	normal.corner_radius_top_right = 8
	normal.corner_radius_bottom_left = 8
	normal.corner_radius_bottom_right = 8

	var hover := normal.duplicate()
	hover.bg_color = Color(0.22, 0.24, 0.29, 0.95)

	var pressed := normal.duplicate()
	pressed.bg_color = Color(0.10, 0.11, 0.14, 0.95)

	target.add_theme_stylebox_override("normal", normal)
	target.add_theme_stylebox_override("hover", hover)
	target.add_theme_stylebox_override("pressed", pressed)
	target.add_theme_color_override("font_color", Color(0.92, 0.92, 0.92, 1.0))
	target.add_theme_font_size_override("font_size", 18)


func _on_toggle_pressed() -> void:
	panel.visible = not panel.visible
	_update_toggle_texts()


func _on_assumptions_toggle_pressed() -> void:
	assumptions_panel.visible = not assumptions_panel.visible
	_update_toggle_texts()


func _on_panel_gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		# Let the toggle button handle itself; otherwise hide the panel when clicked.
		panel.visible = false
		_update_toggle_texts()
		panel.accept_event()


func _on_assumptions_gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		assumptions_panel.visible = false
		_update_toggle_texts()
		assumptions_panel.accept_event()


func _update_toggle_texts() -> void:
	toggle_button.text = "Hide explanation" if panel.visible else "Show explanation"
	assumptions_button.text = "Hide assumptions" if assumptions_panel.visible else "Show assumptions"
