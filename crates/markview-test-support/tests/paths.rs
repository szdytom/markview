use markview_test_support::TestDir;

#[test]
fn canonical_root_owns_its_storage() {
	let root = {
		let directory = TestDir::new();
		assert_eq!(directory.path(), directory.path().canonicalize().unwrap());
		let file = directory.path().join("note 中文.md");
		std::fs::write(&file, "note").unwrap();
		assert_eq!(file, file.canonicalize().unwrap());
		directory.path().to_owned()
	};
	assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn canonical_root_resolves_a_symlinked_parent() {
	let parent = TestDir::new();
	let target = parent.path().join("real");
	let alias = parent.path().join("alias");
	std::fs::create_dir(&target).unwrap();
	std::os::unix::fs::symlink(&target, &alias).unwrap();
	let result = std::process::Command::new(std::env::current_exe().unwrap())
		.args(["--exact", "canonical_root_owns_its_storage"])
		.env("TMPDIR", &alias)
		.output()
		.unwrap();
	assert!(result.status.success(), "{result:?}");
	assert!(
		String::from_utf8_lossy(&result.stdout).contains("1 passed"),
		"{result:?}"
	);
}
