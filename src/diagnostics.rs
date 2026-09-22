//! The same diagnostic fields feed the About page and issue reports.
pub(crate) fn fields(
	backend: Option<wgpu::Backend>,
) -> Vec<(&'static str, String)> {
	vec![
		("Version", env!("CARGO_PKG_VERSION").into()),
		("Build date", env!("MARKVIEW_BUILD_DATE").into()),
		(
			"OS",
			format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
		),
		(
			"WGPU backend",
			backend.map_or_else(
				|| "Not initialized".into(),
				|backend| format!("{backend:?}"),
			),
		),
		("Commit", env!("MARKVIEW_COMMIT").into()),
	]
}

pub(crate) fn report(backend: Option<wgpu::Backend>) -> String {
	let mut text = String::from("Markview diagnostics\n");
	for (name, value) in fields(backend) {
		text.push_str(&format!("{name}: {value}\n"));
	}
	text
}

#[cfg(test)]
mod tests {
	#[test]
	fn issue_report_contains_only_diagnostics_and_the_active_backend() {
		let text = super::report(Some(wgpu::Backend::Vulkan));
		assert!(text.contains("WGPU backend: Vulkan\n"));
		for (name, value) in super::fields(Some(wgpu::Backend::Vulkan)) {
			assert!(text.contains(&format!("{name}: {value}\n")));
		}
		assert_eq!(text.lines().count(), 6);
		assert!(!text.contains("https://"));
		assert!(!text.contains("License"));
		assert!(super::report(None).contains("Not initialized"));
	}
}
