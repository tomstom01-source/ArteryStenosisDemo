use godot::prelude::*;

mod artery_mesh;
mod artery_simulation;
mod blood_particle;
mod hemodynamics;
mod smoking_model;

struct MyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MyExtension {}
