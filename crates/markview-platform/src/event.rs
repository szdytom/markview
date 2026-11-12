//! Reader input normalized independently of the native pointer API.
use std::path::PathBuf;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::SurfaceSizeWriter;
pub use winit::event::{
	ElementState, Force, Ime, KeyEvent, Modifiers, MouseButton,
	MouseScrollDelta, TouchPhase,
};
use winit::window::Theme;
#[cfg(any(target_os = "android", test))]
mod android_keyboard;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId(Option<winit::event::DeviceId>);
impl DeviceId {
	pub const fn from_raw(id: i64) -> Self {
		Self(Some(winit::event::DeviceId::from_raw(id)))
	}
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
	pub device_id: DeviceId,
	pub phase: TouchPhase,
	pub location: PhysicalPosition<f64>,
	pub force: Option<Force>,
	pub id: u64,
}
#[derive(Clone, Debug, PartialEq)]
pub enum WindowEvent {
	CloseRequested,
	CursorLeft {
		device_id: DeviceId,
	},
	CursorMoved {
		device_id: DeviceId,

		position: PhysicalPosition<f64>,
	},
	DoubleTapGesture {
		device_id: DeviceId,
	},
	DroppedFile(PathBuf),
	Focused(bool),
	Ime(Ime),
	KeyboardInput {
		device_id: DeviceId,
		event: KeyEvent,

		is_synthetic: bool,
	},
	ModifiersChanged(Modifiers),
	MouseInput {
		device_id: DeviceId,
		state: ElementState,
		button: MouseButton,
	},
	MouseWheel {
		device_id: DeviceId,
		delta: MouseScrollDelta,
		phase: TouchPhase,
	},
	Moved(PhysicalPosition<i32>),
	Occluded(bool),
	PanGesture {
		device_id: DeviceId,
		delta: PhysicalPosition<f32>,
		phase: TouchPhase,
	},
	PinchGesture {
		device_id: DeviceId,
		delta: f64,
		phase: TouchPhase,
	},
	RedrawRequested,
	Resized(PhysicalSize<u32>),
	RotationGesture {
		device_id: DeviceId,
		delta: f32,
		phase: TouchPhase,
	},
	ScaleFactorChanged {
		scale_factor: f64,
		surface_size_writer: SurfaceSizeWriter,
	},
	ThemeChanged(Theme),
	Touch(Touch),
}

/// Converts native events into the reader's mouse and touch input streams.
#[derive(Default)]
pub struct NativeInput {
	touches: std::collections::HashMap<(DeviceId, u64), PhysicalPosition<f64>>,
	drops: std::collections::HashSet<winit::event_loop::AsyncRequestSerial>,
	#[cfg(any(target_os = "android", test))]
	keyboard: android_keyboard::Keyboard,
}
impl NativeInput {
	pub fn convert(
		&mut self,
		event_loop: &dyn winit::event_loop::ActiveEventLoop,
		event: winit::event::WindowEvent,
	) -> Vec<WindowEvent> {
		use WindowEvent as O;
		use winit::event::WindowEvent as E;
		let mut out = Vec::new();
		match event {
			E::DragEntered { id, .. } => {
				use winit::{data_transfer::TypeHint, event_loop::DndAction};
				if let Ok(transfer) = event_loop.data_transfer(id)
					&& transfer.has_type(&TypeHint::UriList)
					&& let Err(error) =
						event_loop.set_valid_dnd_actions(id, &[DndAction::Copy])
				{
					log::debug!("Cannot accept file drop: {error}");
				}
			}
			E::DragDropped { id, .. } => {
				use winit::data_transfer::TypeHint;
				if let Ok(transfer) = event_loop.data_transfer(id)
					&& transfer.has_type(&TypeHint::UriList)
				{
					match event_loop.fetch_data_transfer(id, &TypeHint::UriList)
					{
						Ok(serial) => {
							self.drops.insert(serial);
						}
						Err(error) => {
							log::debug!("Cannot fetch file drop: {error}")
						}
					}
				}
			}
			E::DataTransferReceived {
				serial,
				value: data,
				..
			} if self.drops.contains(&serial) => match data.try_as_uris() {
				Ok(uris) => {
					self.drops.remove(&serial);
					out.extend(
						file_paths(&uris).into_iter().map(O::DroppedFile),
					);
				}
				Err(error)
					if error.kind() == std::io::ErrorKind::WouldBlock => {}
				Err(error) => {
					self.drops.remove(&serial);
					log::debug!("Cannot read file drop: {error}");
				}
			},
			event => return self.input(event),
		};
		out
	}
	fn input(&mut self, event: winit::event::WindowEvent) -> Vec<WindowEvent> {
		self.input_with_keyboard(event, cfg!(target_os = "android"))
	}
	fn input_with_keyboard(
		&mut self,
		event: winit::event::WindowEvent,
		_android: bool,
	) -> Vec<WindowEvent> {
		use WindowEvent as O;
		use winit::event::{
			ButtonSource, PointerKind, PointerSource, WindowEvent as E,
		};
		let mut out = Vec::new();
		#[cfg(any(target_os = "android", test))]
		let mut event = event;
		#[cfg(any(target_os = "android", test))]
		if _android && let Some(modifiers) = self.keyboard.update(&mut event) {
			out.push(O::ModifiersChanged(modifiers));
		}
		if matches!(event, E::Focused(false)) {
			self.touches.clear();
		}
		let mapped = match event {
			E::PointerButton {
				device_id,
				state,
				position,
				button: ButtonSource::Touch { finger_id, force },
				..
			} => {
				let device_id = DeviceId(device_id);
				let id = finger_id.into_raw() as u64;
				let phase = if state == ElementState::Pressed {
					self.touches.insert((device_id, id), position);
					TouchPhase::Started
				} else {
					self.touches.remove(&(device_id, id));
					TouchPhase::Ended
				};
				Some(O::Touch(Touch {
					device_id,
					id,
					phase,
					location: position,
					force,
				}))
			}
			E::PointerMoved {
				device_id,
				position,
				source: PointerSource::Touch { finger_id, force },
				..
			} => {
				let device_id = DeviceId(device_id);
				let id = finger_id.into_raw() as u64;
				self.touches.get_mut(&(device_id, id)).map(|last| {
					*last = position;
					O::Touch(Touch {
						device_id,
						id,
						phase: TouchPhase::Moved,
						location: position,
						force,
					})
				})
			}
			E::PointerLeft {
				device_id,
				kind: PointerKind::Touch(finger_id),
				..
			} => {
				let device_id = DeviceId(device_id);
				let id = finger_id.into_raw() as u64;
				self.touches.remove(&(device_id, id)).map(|location| {
					O::Touch(Touch {
						device_id,
						id,
						phase: TouchPhase::Cancelled,
						location,
						force: None,
					})
				})
			}
			E::PointerEntered {
				kind: PointerKind::Touch(_),
				..
			} => None,
			E::PointerEntered {
				device_id,
				position,
				..
			}
			| E::PointerMoved {
				device_id,
				position,
				..
			} => Some(O::CursorMoved {
				device_id: DeviceId(device_id),
				position,
			}),
			E::PointerLeft { device_id, .. } => Some(O::CursorLeft {
				device_id: DeviceId(device_id),
			}),
			E::PointerButton {
				device_id,
				state,
				position,
				button,
				..
			} => {
				out.push(O::CursorMoved {
					device_id: DeviceId(device_id),
					position,
				});
				button.mouse_button().map(|button| O::MouseInput {
					device_id: DeviceId(device_id),
					state,
					button,
				})
			}
			event => Self::window_event(event),
		};
		out.extend(mapped);
		out
	}
	fn window_event(event: winit::event::WindowEvent) -> Option<WindowEvent> {
		use WindowEvent as O;
		use winit::event::WindowEvent as E;
		match event {
			E::CloseRequested => Some(O::CloseRequested),
			E::SurfaceResized(size) => Some(O::Resized(size)),
			E::Moved(position) => Some(O::Moved(position)),
			E::Focused(focused) => Some(O::Focused(focused)),
			E::Occluded(occluded) => Some(O::Occluded(occluded)),
			E::RedrawRequested => Some(O::RedrawRequested),
			E::ThemeChanged(theme) => Some(O::ThemeChanged(theme)),
			E::ScaleFactorChanged {
				scale_factor,
				surface_size_writer,
			} => Some(O::ScaleFactorChanged {
				scale_factor,
				surface_size_writer,
			}),
			E::KeyboardInput {
				device_id,
				event,
				is_synthetic,
			} => Some(O::KeyboardInput {
				device_id: DeviceId(device_id),
				event,
				is_synthetic,
			}),
			E::ModifiersChanged(modifiers) => {
				Some(O::ModifiersChanged(modifiers))
			}
			E::Ime(ime) => Some(O::Ime(ime)),
			E::MouseWheel {
				device_id,
				delta,
				phase,
				..
			} => Some(O::MouseWheel {
				device_id: DeviceId(device_id),
				delta,
				phase,
			}),
			event => Self::gesture(event),
		}
	}
	fn gesture(event: winit::event::WindowEvent) -> Option<WindowEvent> {
		use WindowEvent as O;
		use winit::event::WindowEvent as E;
		match event {
			E::PinchGesture {
				device_id,
				delta,
				phase,
			} => Some(O::PinchGesture {
				device_id: DeviceId(device_id),
				delta,
				phase,
			}),
			E::PanGesture {
				device_id,
				delta,
				phase,
			} => Some(O::PanGesture {
				device_id: DeviceId(device_id),
				delta,
				phase,
			}),
			E::RotationGesture {
				device_id,
				delta,
				phase,
			} => Some(O::RotationGesture {
				device_id: DeviceId(device_id),
				delta,
				phase,
			}),
			E::DoubleTapGesture { device_id } => Some(O::DoubleTapGesture {
				device_id: DeviceId(device_id),
			}),
			_ => None,
		}
	}
}
fn file_paths(uris: &[String]) -> Vec<PathBuf> {
	uris.iter()
		.filter_map(|uri| url::Url::parse(uri).ok()?.to_file_path().ok())
		.collect()
}

#[cfg(test)]
#[path = "event/tests.rs"]
mod tests;
