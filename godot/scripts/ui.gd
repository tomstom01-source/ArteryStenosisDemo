extends Control
## Wires the plaque accumulation, lifestyle, and smoking sliders to the Rust
## `ArterySimulation` node (the "Artery" node, referenced via its scene-unique
## name `%Artery`) and reflects the resulting hemodynamics back as readable
## stats. The scientific explanation lives in a single 2D side panel managed
## by `explanation_panel.gd`, not in this 2D UI.
##
## Slider groups are *exclusive*: adjusting any slider makes its group the
## sole plaque source (the other groups' contributions are zeroed and their
## rows dimmed), so each control's independent effect can be inspected
## without interplay.

const SOURCE_DIRECT := 0
const SOURCE_LIFESTYLE := 1
const SOURCE_SMOKING := 2

const COLOR_DIRECT := Color(1.0, 0.78, 0.36, 1.0)
const COLOR_LIFESTYLE := Color(0.44, 0.85, 0.80, 1.0)
const COLOR_SMOKING := Color(0.94, 0.52, 0.52, 1.0)
const COLOR_INACTIVE := Color(0.60, 0.60, 0.60, 0.45)
const COLOR_STATS := Color(1.0, 1.0, 1.0, 1.0)

## Which plaque source the sliders currently control exclusively. Adjusting
## any slider switches this to that slider's group.
var active_source := SOURCE_DIRECT

@onready var plaque_slider: HSlider = $BottomPanel/Margin/VBox/PlaqueRow/PlaqueSlider
@onready var plaque_value_label: Label = $BottomPanel/Margin/VBox/PlaqueRow/PlaqueValueLabel
@onready var plaque_title_label: Label = $BottomPanel/Margin/VBox/PlaqueRow/PlaqueTitleLabel

@onready var lifestyle_slider: HSlider = $BottomPanel/Margin/VBox/LifestyleRow/LifestyleSlider
@onready var lifestyle_value_label: Label = $BottomPanel/Margin/VBox/LifestyleRow/LifestyleValueLabel
@onready var lifestyle_title_label: Label = $BottomPanel/Margin/VBox/LifestyleRow/LifestyleTitleLabel

@onready var smoking_slider: HSlider = $BottomPanel/Margin/VBox/SmokingRow/SmokingSlider
@onready var smoking_value_label: Label = $BottomPanel/Margin/VBox/SmokingRow/SmokingValueLabel
@onready var smoking_title_label: Label = $BottomPanel/Margin/VBox/SmokingRow/SmokingTitleLabel

@onready var smoking_years_slider: HSlider = $BottomPanel/Margin/VBox/SmokingYearsRow/SmokingYearsSlider
@onready var smoking_years_value_label: Label = $BottomPanel/Margin/VBox/SmokingYearsRow/SmokingYearsValueLabel
@onready var smoking_years_title_label: Label = $BottomPanel/Margin/VBox/SmokingYearsRow/SmokingYearsTitleLabel

@onready var effective_plaque_label: Label = $BottomPanel/Margin/VBox/StatsRow/EffectivePlaqueLabel
@onready var peak_velocity_label: Label = $BottomPanel/Margin/VBox/StatsRow/PeakVelocityLabel

@onready var plaque_row: HBoxContainer = $BottomPanel/Margin/VBox/PlaqueRow
@onready var lifestyle_row: HBoxContainer = $BottomPanel/Margin/VBox/LifestyleRow
@onready var smoking_row: HBoxContainer = $BottomPanel/Margin/VBox/SmokingRow
@onready var smoking_years_row: HBoxContainer = $BottomPanel/Margin/VBox/SmokingYearsRow
@onready var stats_row: HBoxContainer = $BottomPanel/Margin/VBox/StatsRow

@onready var bottom_panel: PanelContainer = $BottomPanel

@onready var artery: Node3D = %Artery


func _ready() -> void:
	_apply_env_overrides()
	_setup_bottom_panel_style()
	_setup_label_styles()
	_setup_slider_styles()
	plaque_slider.value_changed.connect(_on_slider_changed.bind(SOURCE_DIRECT))
	lifestyle_slider.value_changed.connect(_on_slider_changed.bind(SOURCE_LIFESTYLE))
	smoking_slider.value_changed.connect(_on_slider_changed.bind(SOURCE_SMOKING))
	smoking_years_slider.value_changed.connect(_on_slider_changed.bind(SOURCE_SMOKING))
	_update_value_labels()
	_update_row_emphasis()
	_refresh_stats()


func _apply_env_overrides() -> void:
	# Deterministic-start hook for movie capture / E2E runs: seed the sliders
	# from the environment before signals are connected, so the first rebuild
	# uses the requested state and the UI stays consistent with it.
	var plaque_env := OS.get_environment("DEVIN_INITIAL_PLAQUE")
	if plaque_env.is_valid_float():
		plaque_slider.value = clampf(plaque_env.to_float(), 0.0, 100.0)
		active_source = SOURCE_DIRECT
	var lifestyle_env := OS.get_environment("DEVIN_INITIAL_LIFESTYLE")
	if lifestyle_env.is_valid_float():
		lifestyle_slider.value = clampf(lifestyle_env.to_float(), 0.0, 4.0)
		active_source = SOURCE_LIFESTYLE
	var smoking_env := OS.get_environment("DEVIN_INITIAL_SMOKING")
	if smoking_env.is_valid_float():
		smoking_slider.value = clampf(smoking_env.to_float(), 0.0, 40.0)
		active_source = SOURCE_SMOKING
	var smoking_years_env := OS.get_environment("DEVIN_INITIAL_SMOKING_YEARS")
	if smoking_years_env.is_valid_float():
		smoking_years_slider.value = clampf(smoking_years_env.to_float(), 0.0, 10.0)
		active_source = SOURCE_SMOKING


func _setup_bottom_panel_style() -> void:
	var panel_style := StyleBoxFlat.new()
	panel_style.bg_color = Color(0.07, 0.08, 0.11, 0.92)
	panel_style.border_color = Color(0.25, 0.27, 0.33, 0.9)
	panel_style.border_width_left = 2
	panel_style.border_width_top = 2
	panel_style.border_width_right = 2
	panel_style.corner_radius_top_left = 20
	panel_style.corner_radius_top_right = 20
	bottom_panel.add_theme_stylebox_override("panel", panel_style)


func _setup_label_styles() -> void:
	var title_font_size := 18
	var value_font_size := 18
	var stats_font_size := 20

	for title in [plaque_title_label, lifestyle_title_label, smoking_title_label, smoking_years_title_label]:
		title.add_theme_font_size_override("font_size", title_font_size)
		title.add_theme_color_override("font_color", Color(0.95, 0.95, 0.95, 1.0))
		title.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT

	for value in [plaque_value_label, lifestyle_value_label, smoking_value_label, smoking_years_value_label]:
		value.add_theme_font_size_override("font_size", value_font_size)

	for stat in [effective_plaque_label, peak_velocity_label]:
		stat.add_theme_font_size_override("font_size", stats_font_size)
		stat.add_theme_color_override("font_color", COLOR_STATS)


func _setup_slider_styles() -> void:
	# Track and filled-track styleboxes shared across sliders; the grabber colour
	# is set per-row in _update_row_emphasis so the active group is instantly
	# colour-coded.
	var track := StyleBoxFlat.new()
	track.bg_color = Color(0.18, 0.19, 0.23, 1.0)
	track.corner_radius_top_left = 8
	track.corner_radius_top_right = 8
	track.corner_radius_bottom_left = 8
	track.corner_radius_bottom_right = 8

	var fill := StyleBoxFlat.new()
	fill.bg_color = Color(0.55, 0.55, 0.55, 1.0)
	fill.corner_radius_top_left = 8
	fill.corner_radius_top_right = 8
	fill.corner_radius_bottom_left = 8
	fill.corner_radius_bottom_right = 8

	var grabber := StyleBoxFlat.new()
	grabber.bg_color = Color(0.85, 0.85, 0.85, 1.0)
	grabber.corner_radius_top_left = 10
	grabber.corner_radius_top_right = 10
	grabber.corner_radius_bottom_left = 10
	grabber.corner_radius_bottom_right = 10
	grabber.set_expand_margin_all(6)

	for slider in [plaque_slider, lifestyle_slider, smoking_slider, smoking_years_slider]:
		slider.custom_minimum_size = Vector2(340, 22)
		slider.add_theme_stylebox_override("slider", track)
		slider.add_theme_stylebox_override("grabber_area", fill)
		slider.add_theme_stylebox_override("grabber_area_highlight", fill)
		slider.add_theme_stylebox_override("grabber", grabber)
		slider.add_theme_stylebox_override("grabber_highlight", grabber)


func _on_slider_changed(_value: float, source: int) -> void:
	active_source = source
	_update_value_labels()
	_update_row_emphasis()
	_refresh_stats()


func _update_value_labels() -> void:
	plaque_value_label.text = "%d%%" % int(round(plaque_slider.value))
	lifestyle_value_label.text = "%d / 4" % int(round(lifestyle_slider.value))
	smoking_value_label.text = "%d" % int(round(smoking_slider.value))
	smoking_years_value_label.text = "%d" % int(round(smoking_years_slider.value))


func _update_row_emphasis() -> void:
	# Each slider group gets its own accent colour when active; inactive rows
	# are dimmed and greyed. The slider filled track and value label take the
	# group colour so the active source is unambiguous.
	var fill_active := StyleBoxFlat.new()
	fill_active.corner_radius_top_left = 6
	fill_active.corner_radius_top_right = 6
	fill_active.corner_radius_bottom_left = 6
	fill_active.corner_radius_bottom_right = 6
	fill_active.set_expand_margin_all(4)

	var grabber_active := StyleBoxFlat.new()
	grabber_active.corner_radius_top_left = 10
	grabber_active.corner_radius_top_right = 10
	grabber_active.corner_radius_bottom_left = 10
	grabber_active.corner_radius_bottom_right = 10
	grabber_active.set_expand_margin_all(6)

	for source: int in [SOURCE_DIRECT, SOURCE_LIFESTYLE, SOURCE_SMOKING]:
		var is_active: bool = source == active_source
		var row_color := _source_color(source) if is_active else COLOR_INACTIVE
		var slider_color := _source_color(source) if is_active else Color(0.55, 0.55, 0.55, 1.0)
		var row := _row_for_source(source)
		row.modulate = row_color

		# Tint the slider's filled track and grabber for this group.
		fill_active.bg_color = slider_color
		grabber_active.bg_color = slider_color
		var sliders := _sliders_for_source(source)
		for slider in sliders:
			slider.add_theme_stylebox_override("grabber_area", fill_active)
			slider.add_theme_stylebox_override("grabber_area_highlight", fill_active)
			slider.add_theme_stylebox_override("grabber", grabber_active)
			slider.add_theme_stylebox_override("grabber_highlight", grabber_active)

		# Keep the value label bright with the group colour.
		var value := _value_label_for_source(source)
		value.add_theme_color_override("font_color", slider_color)

	# The years-of-smoking slider is part of the smoking group, not its own.
	smoking_years_row.modulate = smoking_row.modulate

	# Stats row is always fully visible and gets a subtle accent from the
	# currently active source.
	stats_row.modulate = Color(1, 1, 1, 1)


func _row_for_source(source: int) -> HBoxContainer:
	match source:
		SOURCE_LIFESTYLE:
			return lifestyle_row
		SOURCE_SMOKING:
			return smoking_row
		_:
			return plaque_row


func _sliders_for_source(source: int) -> Array[HSlider]:
	match source:
		SOURCE_LIFESTYLE:
			return [lifestyle_slider]
		SOURCE_SMOKING:
			return [smoking_slider, smoking_years_slider]
		_:
			return [plaque_slider]


func _value_label_for_source(source: int) -> Label:
	match source:
		SOURCE_LIFESTYLE:
			return lifestyle_value_label
		SOURCE_SMOKING:
			return smoking_value_label
		_:
			return plaque_value_label


func _source_color(source: int) -> Color:
	match source:
		SOURCE_LIFESTYLE:
			return COLOR_LIFESTYLE
		SOURCE_SMOKING:
			return COLOR_SMOKING
		_:
			return COLOR_DIRECT


func _refresh_stats() -> void:
	artery.set_parameters(
		plaque_slider.value,
		lifestyle_slider.value,
		smoking_slider.value,
		smoking_years_slider.value,
		active_source
	)

	effective_plaque_label.text = "Effective plaque: %d%% (%s)" % [
		round(artery.get_effective_plaque_percent()), _source_name()
	]
	peak_velocity_label.text = "Peak velocity: %d cm/s" % round(artery.get_peak_velocity_cm_s())


func _source_name() -> String:
	match active_source:
		SOURCE_LIFESTYLE:
			return "lifestyle"
		SOURCE_SMOKING:
			return "smoking"
		_:
			return "direct"
