extends Node
## Exposes engine performance monitors to the E2E automation server.
##
## The automation server's `call_method` command can invoke any method on any
## node, so individual float getters (rather than one Dictionary) keep the
## JSON round-trip trivially simple on the Python side. Intended for the
## autouse `perf_record` fixture in tests/e2e/conftest.py, which appends one
## CSV row per test to <artifacts>/e2e/<run-id>/perf.csv.
##
## Note: in E2E runs DEVIN_FREEZE_PARTICLES=1 stops particle motion, so these
## numbers measure the static scene plus rebuild cost, not steady-state
## animation load. Use scripts/capture_movie.ps1 for animation-shaped work.


func get_perf_fps() -> float:
	return Performance.get_monitor(Performance.TIME_FPS)


func get_perf_process_ms() -> float:
	return Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0


func get_perf_physics_ms() -> float:
	return Performance.get_monitor(Performance.TIME_PHYSICS_PROCESS) * 1000.0


func get_perf_draw_calls() -> float:
	return Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME)


func get_perf_primitives() -> float:
	return Performance.get_monitor(Performance.RENDER_TOTAL_PRIMITIVES_IN_FRAME)


func get_perf_video_mem_mb() -> float:
	return Performance.get_monitor(Performance.RENDER_VIDEO_MEM_USED) / 1048576.0
