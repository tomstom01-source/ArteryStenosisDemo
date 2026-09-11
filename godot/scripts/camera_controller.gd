extends Camera3D
## Free viewport controller for inspecting the artery from any angle.
## Left-drag rotates, middle/right-drag pans, and the mouse wheel zooms.

@export var target_path: NodePath = NodePath("../Artery")
@export var distance: float = 6.5
@export var min_distance: float = 1.5
@export var max_distance: float = 18.0
@export var rotation_sensitivity: float = 0.01
@export var pan_sensitivity: float = 0.002
@export var zoom_step: float = 0.75

var focus_point := Vector3.ZERO
# Start on the far side of the vessel so the sun sits behind the model: the
# viewer sees the backlit plaque glow and particle flow first thing.
var yaw := PI
var pitch := -0.2
var rotating := false
var panning := false


func _ready() -> void:
	var target := get_node_or_null(target_path) as Node3D
	if target != null:
		focus_point = target.global_position
	_update_camera()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		_handle_mouse_button(event)
	elif event is InputEventMouseMotion:
		_handle_mouse_motion(event)


func _handle_mouse_button(event: InputEventMouseButton) -> void:
	match event.button_index:
		MOUSE_BUTTON_LEFT:
			rotating = event.pressed
		MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT:
			panning = event.pressed
		MOUSE_BUTTON_WHEEL_UP:
			if event.pressed:
				distance = max(min_distance, distance - zoom_step)
				_update_camera()
		MOUSE_BUTTON_WHEEL_DOWN:
			if event.pressed:
				distance = min(max_distance, distance + zoom_step)
				_update_camera()


func _handle_mouse_motion(event: InputEventMouseMotion) -> void:
	if rotating:
		yaw -= event.relative.x * rotation_sensitivity
		pitch = clamp(pitch - event.relative.y * rotation_sensitivity, -1.45, 1.45)
		_update_camera()
	elif panning:
		var pan_scale := distance * pan_sensitivity
		focus_point += (-global_transform.basis.x * event.relative.x + global_transform.basis.y * event.relative.y) * pan_scale
		_update_camera()


func _update_camera() -> void:
	var orbit_offset := Vector3(
		 sin(yaw) * cos(pitch) * distance,
		 sin(pitch) * distance,
		 cos(yaw) * cos(pitch) * distance
	)
	global_position = focus_point + orbit_offset
	look_at(focus_point, Vector3.UP)
