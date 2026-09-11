extends Camera3D
## Free viewport controller for inspecting the artery from any angle.
## Left-drag / one-finger rotates, the mouse wheel zooms, and a two-finger
## pinch/spread zooms on touch screens. Touch has no pan.

@export var target_path: NodePath = NodePath("../Artery")
@export var distance: float = 6.5
@export var min_distance: float = 1.5
@export var max_distance: float = 18.0
@export var rotation_sensitivity: float = 0.01
@export var pan_sensitivity: float = 0.002
@export var zoom_step: float = 0.75
@export var pinch_zoom_exponent: float = 0.35
@export var touch_rotation_scale: float = 0.12

var focus_point := Vector3.ZERO
# Start on the far side of the vessel so the sun sits behind the model: the
# viewer sees the backlit plaque glow and particle flow first thing.
var yaw := PI
var pitch := -0.2
var rotating := false
var panning := false

# Active screen touches: index -> Vector2 position.
var _touches: Dictionary = {}
# When a two-finger gesture is active, the remaining finger after one is lifted
# is ignored until all fingers leave the screen.
var _ignore_remaining_drag := false


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
	elif event is InputEventScreenTouch:
		_handle_screen_touch(event)
	elif event is InputEventScreenDrag:
		_handle_screen_drag(event)


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


func _handle_screen_touch(event: InputEventScreenTouch) -> void:
	if event.pressed:
		_touches[event.index] = event.position
	else:
		_touches.erase(event.index)

	match _touches.size():
		0:
			_ignore_remaining_drag = false
			rotating = false
			panning = false
		1:
			rotating = not _ignore_remaining_drag
			panning = false
		2:
			_ignore_remaining_drag = true
			rotating = false
			panning = false


func _handle_screen_drag(event: InputEventScreenDrag) -> void:
	if not _touches.has(event.index):
		return

	if _touches.size() == 1:
		_touches[event.index] = event.position
		if _ignore_remaining_drag:
			return

		yaw -= event.relative.x * rotation_sensitivity * touch_rotation_scale
		pitch = clamp(pitch - event.relative.y * rotation_sensitivity * touch_rotation_scale, -1.45, 1.45)
		_update_camera()
	elif _touches.size() == 2:
		var previous: Dictionary = _touches.duplicate()
		_touches[event.index] = event.position

		var keys := _touches.keys()
		var old_p0: Vector2 = previous[keys[0]]
		var old_p1: Vector2 = previous[keys[1]]
		var new_p0: Vector2 = _touches[keys[0]]
		var new_p1: Vector2 = _touches[keys[1]]

		var old_dist := old_p0.distance_to(old_p1)
		var new_dist := new_p0.distance_to(new_p1)

		# Pinch/spread zoom: use the ratio of the finger span so the zoom
		# amount is tied to how much the fingers actually move, not raw pixels.
		if old_dist > 1.0 and new_dist > 1.0:
			var zoom_factor := pow(old_dist / new_dist, pinch_zoom_exponent)
			distance = clamp(distance * zoom_factor, min_distance, max_distance)
			_update_camera()


func _update_camera() -> void:
	var orbit_offset := Vector3(
		 sin(yaw) * cos(pitch) * distance,
		 sin(pitch) * distance,
		 cos(yaw) * cos(pitch) * distance
	)
	global_position = focus_point + orbit_offset
	look_at(focus_point, Vector3.UP)
