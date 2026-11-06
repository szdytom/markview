use mvfuzz::font_archive;

fn input(container: u8, valid: bool) -> Vec<u8> {
	let mut data = b"ARC1".to_vec();
	data.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
	data.push(container);
	data.push(u8::from(!valid));
	data.extend(b"member contents alpha beta gamma");
	data
}

#[test]
fn valid_and_damaged_archives_never_escape_or_leak_members() {
	for container in 0..4 {
		font_archive::check(&input(container, true));
	}
	for payload in [
		&b"not an archive"[..],
		&b"PK\x03\x04broken"[..],
		&b"\x1f\x8b\x08broken"[..],
		&b"\x28\xb5\x2f\xfdbroken"[..],
	] {
		let mut data = b"ARC1".to_vec();
		data.extend([0u8; 16]);
		data.push(0);
		data.push(1);
		data.extend(payload);
		font_archive::check(&data);
	}
}
