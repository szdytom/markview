#![allow(unused_variables)]

use super::*;
use winit::event::{
	ButtonSource, FingerId, PointerKind, PointerSource, WindowEvent as E,
};
use winit::keyboard::{
	Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey,
};

fn key(code: KeyCode, logical_key: Key, state: ElementState) -> E {
	E::KeyboardInput {
		device_id: None,
		event: KeyEvent {
			physical_key: PhysicalKey::Code(code),
			key_without_modifiers: logical_key.clone(),
			text: logical_key.to_text().map(Into::into),
			text_with_all_modifiers: logical_key.to_text().map(Into::into),
			logical_key,
			location: KeyLocation::Standard,
			state,
			repeat: false,
		},
		is_synthetic: false,
	}
}

#[test]
fn android_control_chords_report_modifiers_before_keys_and_restore_letters() {
	let mut input = NativeInput::default();
	for code in [KeyCode::ControlLeft, KeyCode::ControlRight] {
		let events = input.input_with_keyboard(
			key(code, Key::Named(NamedKey::Control), ElementState::Pressed),
			true,
		);
		assert!(
			matches!(&events[..], [WindowEvent::ModifiersChanged(m), WindowEvent::KeyboardInput { .. }] if m.state() == ModifiersState::CONTROL)
		);
		for (byte, expected) in
			(0..=31).zip(" abcdefghijklmnopqrstuvwxyz[\\]^_".chars())
		{
			let text = char::from(byte).to_string();
			let events = input.input_with_keyboard(
				key(
					KeyCode::KeyB,
					Key::Character(text.clone().into()),
					ElementState::Pressed,
				),
				true,
			);
			let [WindowEvent::KeyboardInput { event, .. }] = &events[..] else {
				panic!(
					"a held modifier must not emit duplicate modifier events"
				)
			};
			assert_eq!(
				event.logical_key,
				Key::Character(expected.to_string().into())
			);
			assert_eq!(event.text.as_deref(), Some(text.as_str()));
		}
		let events = input.input_with_keyboard(
			key(code, Key::Named(NamedKey::Control), ElementState::Released),
			true,
		);
		assert!(
			matches!(&events[..], [WindowEvent::ModifiersChanged(m), WindowEvent::KeyboardInput { .. }] if m.state().is_empty())
		);
	}
	let events = input.input_with_keyboard(
		key(
			KeyCode::PageUp,
			Key::Named(NamedKey::PageUp),
			ElementState::Pressed,
		),
		true,
	);
	assert!(
		matches!(&events[..], [WindowEvent::KeyboardInput { event, .. }] if event.logical_key == Key::Named(NamedKey::PageUp))
	);
}

#[test]
fn desktop_keys_and_android_unmodified_text_are_unchanged() {
	let mut input = NativeInput::default();
	let control = key(
		KeyCode::ControlLeft,
		Key::Named(NamedKey::Control),
		ElementState::Pressed,
	);
	assert!(matches!(
		&input.input_with_keyboard(control, false)[..],
		[WindowEvent::KeyboardInput { .. }]
	));
	for android in [false, true] {
		for text in ["b", "B", "中文", "🙂", "\u{2}"] {
			let events = input.input_with_keyboard(
				key(
					KeyCode::KeyB,
					Key::Character(text.into()),
					ElementState::Pressed,
				),
				android,
			);
			assert!(
				matches!(&events[..], [WindowEvent::KeyboardInput { event, .. }]
				if event.logical_key == Key::Character(text.into()) && event.text.as_deref() == Some(text))
			);
		}
	}
}

proptest::proptest! {
	#[test]
	fn android_modifier_sequences_preserve_both_sides_and_reset_on_blur(
		ops in proptest::collection::vec((0usize..9, proptest::bool::ANY), 0..100)
	) {
		let codes = [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::ShiftLeft, KeyCode::ShiftRight,
			KeyCode::AltLeft, KeyCode::AltRight, KeyCode::MetaLeft, KeyCode::MetaRight];
		let mut pressed = [false; 8];
		let mut input = NativeInput::default();
		let mut observed = ModifiersState::empty();
		for (index, down) in ops {
			let event = if index == 8 {
				pressed.fill(false);
				E::Focused(false)
			} else {
				pressed[index] = down;
				key(codes[index], Key::Unidentified(winit::keyboard::NativeKey::Unidentified),
					if down { ElementState::Pressed } else { ElementState::Released })
			};
			let mut expected = ModifiersState::empty();
			for (sides, flag) in pressed.as_chunks::<2>().0.iter().zip([
				ModifiersState::CONTROL, ModifiersState::SHIFT, ModifiersState::ALT, ModifiersState::META
			]) {
				expected.set(flag, sides.iter().any(|side| *side));
			}
			let before = observed;
			let events = input.input_with_keyboard(event, true);
			for event in &events {
				if let WindowEvent::ModifiersChanged(modifiers) = event {
					observed = modifiers.state();
				}
			}
			proptest::prop_assert_eq!(observed, expected);
			proptest::prop_assert_eq!(events.len(), if before != expected { 2 } else { 1 });
		}
	}
}
fn button(id: usize, state: ElementState) -> E {
	E::PointerButton {
		device_id: None,
		state,
		position: PhysicalPosition::new(20.0, 30.0),
		primary: id == 1,
		button: ButtonSource::Touch {
			finger_id: FingerId::from_raw(id),
			force: None,
		},
		is_macos_activation_click: false,
	}
}
fn leave(id: usize) -> E {
	E::PointerLeft {
		device_id: None,
		position: None,
		primary: id == 1,
		kind: PointerKind::Touch(FingerId::from_raw(id)),
	}
}
#[test]
fn release_then_leave_ends_touch_once_and_cancel_preserves_last_position() {
	let mut input = NativeInput::default();
	assert!(matches!(
		&input.input(button(1, ElementState::Pressed))[..],
		[WindowEvent::Touch(Touch {
			phase: TouchPhase::Started,
			..
		})]
	));
	input.input(button(2, ElementState::Pressed));
	let moved = input.input(E::PointerMoved {
		device_id: None,
		position: PhysicalPosition::new(40.0, 50.0),
		primary: false,
		source: PointerSource::Touch {
			finger_id: FingerId::from_raw(2),
			force: None,
		},
	});
	assert!(matches!(
		&moved[..],
		[WindowEvent::Touch(Touch {
			phase: TouchPhase::Moved,
			id: 2,
			..
		})]
	));
	assert!(matches!(
		&input.input(button(1, ElementState::Released))[..],
		[WindowEvent::Touch(Touch {
			phase: TouchPhase::Ended,
			..
		})]
	));
	assert!(input.input(leave(1)).is_empty());
	let cancelled = input.input(leave(2));
	let [WindowEvent::Touch(touch)] = &cancelled[..] else {
		panic!("expected cancellation")
	};
	assert_eq!(touch.phase, TouchPhase::Cancelled);
	assert_eq!(touch.location, PhysicalPosition::new(40.0, 50.0));
	assert!(input.touches.is_empty());
}
#[test]
fn entering_without_moving_updates_mouse_position() {
	let mut input = NativeInput::default();
	let events = input.input(E::PointerEntered {
		device_id: None,
		position: PhysicalPosition::new(12.0, 34.0),
		primary: true,
		kind: PointerKind::Mouse,
	});
	assert!(
		matches!(&events[..],[WindowEvent::CursorMoved {position,..}] if *position==PhysicalPosition::new(12.0,34.0))
	);
}
#[test]
fn drop_paths_decode_local_uris_and_reject_remote_or_invalid_values() {
	let path = std::env::temp_dir().join("中文 file #1.md");
	let uri = url::Url::from_file_path(&path).unwrap().to_string();
	assert_eq!(
		file_paths(&[
			uri,
			"https://example.com/file.md".into(),
			"not a URI".into()
		]),
		vec![path]
	);
}
proptest::proptest! {
 #[test]
 fn local_file_uri_round_trip(name in "[a-zA-Z0-9 #_%]{1,60}") {
  let path=std::env::temp_dir().join(format!("{name}.md"));
  let uri=url::Url::from_file_path(&path).unwrap().to_string();
  proptest::prop_assert_eq!(file_paths(&[uri]),vec![path]);
 }
}

use std::{cell::RefCell, sync::Arc};
use winit::{
	cursor::*,
	data_transfer::*,
	error::*,
	event_loop::*,
	monitor::MonitorHandle,
	window::{Window, WindowAttributes},
};
#[derive(Debug)]
struct DropLoop {
	actions: RefCell<Vec<DndAction>>,
	requested: RefCell<Vec<AsyncRequestSerial>>,
}
impl ActiveEventLoop for DropLoop {
	fn data_transfer(
		&self,
		_: DataTransferId,
	) -> Result<Box<dyn DataTransfer>, TransferError> {
		Ok(Box::new(UriTransfer))
	}
	fn set_valid_dnd_actions(
		&self,
		_: DataTransferId,
		actions: &[DndAction],
	) -> Result<(), TransferError> {
		*self.actions.borrow_mut() = actions.to_vec();
		Ok(())
	}
	fn fetch_data_transfer(
		&self,
		_: DataTransferId,
		ty: &dyn TransferType,
	) -> Result<AsyncRequestSerial, TransferError> {
		assert_eq!(ty.hint(), Some(TypeHint::UriList));
		let serial = AsyncRequestSerial::get();
		self.requested.borrow_mut().push(serial);
		Ok(serial)
	}
	fn create_proxy(&self) -> EventLoopProxy {
		panic!("unexpected native operation")
	}
	fn create_window(
		&self,
		window_attributes: WindowAttributes,
	) -> Result<Box<dyn Window>, CreateWindowError> {
		panic!("unexpected native operation")
	}
	fn create_custom_cursor(
		&self,
		custom_cursor: CustomCursorSource,
	) -> Result<CustomCursor, CustomCursorError> {
		panic!("unexpected native operation")
	}
	fn available_monitors(&self) -> Box<dyn Iterator<Item = MonitorHandle>> {
		panic!("unexpected native operation")
	}
	fn primary_monitor(&self) -> Option<MonitorHandle> {
		panic!("unexpected native operation")
	}
	fn listen_device_events(&self, allowed: DeviceEvents) {
		panic!("unexpected native operation")
	}
	fn system_theme(&self) -> Option<Theme> {
		panic!("unexpected native operation")
	}
	fn set_control_flow(&self, control_flow: ControlFlow) {
		panic!("unexpected native operation")
	}
	fn control_flow(&self) -> ControlFlow {
		panic!("unexpected native operation")
	}
	fn exit(&self) {
		panic!("unexpected native operation")
	}
	fn exiting(&self) -> bool {
		panic!("unexpected native operation")
	}
	fn owned_display_handle(&self) -> OwnedDisplayHandle {
		panic!("unexpected native operation")
	}
	fn rwh_06_handle(&self) -> &dyn winit::raw_window_handle::HasDisplayHandle {
		panic!("unexpected native operation")
	}
}
#[derive(Debug)]
struct UriTransfer;
impl DataTransfer for UriTransfer {
	fn for_each_available_type<'a>(
		&'a self,
		f: &mut dyn FnMut(&'a dyn TransferType) -> std::ops::ControlFlow<()>,
	) {
		let _ = f(&TypeHint::UriList);
	}
}
#[derive(Debug)]
struct UriData {
	ready: bool,
	uris: Vec<String>,
}
impl TypedData for UriData {
	fn type_(&self) -> &dyn TransferType {
		&TypeHint::UriList
	}
	fn try_read(&self) -> Option<Box<dyn std::io::BufRead>> {
		None
	}
	fn try_as_uris(&self) -> std::io::Result<Vec<String>> {
		if self.ready {
			Ok(self.uris.clone())
		} else {
			Err(std::io::ErrorKind::WouldBlock.into())
		}
	}
	fn try_as_string(&self) -> std::io::Result<String> {
		panic!("expected URI request")
	}
}
#[test]
fn drop_fetches_only_after_release_and_opens_each_file_once_when_ready() {
	let event_loop = DropLoop {
		actions: Default::default(),
		requested: Default::default(),
	};
	let mut input = NativeInput::default();
	let id = DataTransferId::from_raw(123);
	assert!(
		input
			.convert(&event_loop, E::DragEntered { id, position: None })
			.is_empty()
	);
	assert_eq!(*event_loop.actions.borrow(), vec![DndAction::Copy]);
	assert!(event_loop.requested.borrow().is_empty());
	assert!(
		input
			.convert(
				&event_loop,
				E::DragDropped {
					id,
					proposed_action: Some(DndAction::Copy)
				}
			)
			.is_empty()
	);
	let serial = event_loop.requested.borrow()[0];
	let event = |ready| E::DataTransferReceived {
		id,
		serial,
		value: Arc::new(UriData {
			ready,
			uris: vec![
				url::Url::from_file_path(std::env::temp_dir().join("first.md"))
					.unwrap()
					.into(),
				url::Url::from_file_path(
					std::env::temp_dir().join("second.md"),
				)
				.unwrap()
				.into(),
			],
		}),
	};
	assert!(input.convert(&event_loop, event(false)).is_empty());
	let opened = input.convert(&event_loop, event(true));
	assert!(
		matches!(&opened[..],[WindowEvent::DroppedFile(a),WindowEvent::DroppedFile(b)] if a.file_name().unwrap()=="first.md" && b.file_name().unwrap()=="second.md")
	);
	assert!(input.convert(&event_loop, event(true)).is_empty());
	assert!(input.drops.is_empty());
}

#[test]
fn an_unidentified_pointer_does_not_alias_a_known_device() {
	assert_ne!(DeviceId(None), DeviceId::from_raw(0));
}
