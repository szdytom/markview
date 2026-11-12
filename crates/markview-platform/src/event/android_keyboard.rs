//! Fill the modifier events missing from `winit`'s Android backend.
use winit::{
	event::{ElementState, Modifiers, WindowEvent},
	keyboard::{Key, KeyCode, ModifiersState, PhysicalKey},
};

#[derive(Default)]
pub(super) struct Keyboard {
	pressed: u8,
}

impl Keyboard {
	pub(super) fn update(
		&mut self,
		event: &mut WindowEvent,
	) -> Option<Modifiers> {
		let before = self.modifiers();
		match event {
			WindowEvent::Focused(false) => self.pressed = 0,
			WindowEvent::KeyboardInput { event, .. } => {
				let bit = match event.physical_key {
					PhysicalKey::Code(KeyCode::ControlLeft) => 1,
					PhysicalKey::Code(KeyCode::ControlRight) => 2,
					PhysicalKey::Code(KeyCode::ShiftLeft) => 4,
					PhysicalKey::Code(KeyCode::ShiftRight) => 8,
					PhysicalKey::Code(KeyCode::AltLeft) => 16,
					PhysicalKey::Code(KeyCode::AltRight) => 32,
					PhysicalKey::Code(KeyCode::MetaLeft) => 64,
					PhysicalKey::Code(KeyCode::MetaRight) => 128,
					_ => 0,
				};
				if event.state == ElementState::Pressed {
					self.pressed |= bit;
				} else {
					self.pressed &= !bit;
				}
				if self.modifiers().control_key() {
					restore_shortcut_key(&mut event.logical_key);
				}
			}
			_ => {}
		}
		let after = self.modifiers();
		(before != after).then(|| after.into())
	}

	fn modifiers(&self) -> ModifiersState {
		let mut modifiers = ModifiersState::empty();
		for (mask, modifier) in [
			(3, ModifiersState::CONTROL),
			(12, ModifiersState::SHIFT),
			(48, ModifiersState::ALT),
			(192, ModifiersState::META),
		] {
			modifiers.set(modifier, self.pressed & mask != 0);
		}
		modifiers
	}
}

// Android's character map includes Ctrl in `logical_key`.
fn restore_shortcut_key(key: &mut Key) {
	if let Key::Character(text) = key
		&& let [byte @ 0..=31] = *text.as_bytes()
	{
		let letter = match byte {
			0 => b' ',
			1..=26 => byte + b'a' - 1,
			_ => byte + b'@',
		};
		*key = Key::Character(char::from(letter).to_string().into());
	}
}
