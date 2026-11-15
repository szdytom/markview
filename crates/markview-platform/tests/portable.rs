#![cfg(any(target_os = "windows", target_os = "linux"))]

use std::{fs, process::Command};

#[test]
fn configuration_directory_is_fixed_for_the_process() {
	let executable = std::env::current_exe().unwrap();
	let marker = executable
		.parent()
		.unwrap()
		.join("markview-portable-marker.txt");
	if std::env::var_os("MARKVIEW_TEST_PORTABLE_CHILD").is_some() {
		let expected = Some(executable.parent().unwrap().join("data"));
		assert_eq!(markview_platform::config_directory(), expected);
		fs::remove_file(marker).unwrap();
		assert_eq!(markview_platform::config_directory(), expected);
		return;
	}
	let root = markview_test_support::TestDir::new();
	let copied = root.path().join(executable.file_name().unwrap());
	fs::copy(executable, &copied).unwrap();
	fs::write(root.path().join("markview-portable-marker.txt"), "").unwrap();
	let output = Command::new(copied)
		.args([
			"--exact",
			"configuration_directory_is_fixed_for_the_process",
		])
		.env("MARKVIEW_TEST_PORTABLE_CHILD", "1")
		.env_remove("APPDATA")
		.env_remove("XDG_CONFIG_HOME")
		.env_remove("HOME")
		.output()
		.unwrap();
	assert!(output.status.success(), "{output:?}");
	assert!(!root.path().join("data").exists());
}
