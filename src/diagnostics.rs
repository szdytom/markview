//! The same diagnostic fields feed the About page and issue reports.
use crate::lang::Lang;

pub(crate) fn fields(
	lang: Lang,
	backend: Option<wgpu::Backend>,
) -> Vec<(&'static str, String)> {
	vec![
		(lang.diagnostics_version(), env!("CARGO_PKG_VERSION").into()),
		(
			lang.diagnostics_build_date(),
			env!("MARKVIEW_BUILD_DATE").into(),
		),
		(
			lang.diagnostics_os(),
			format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
		),
		(
			lang.diagnostics_backend(),
			backend.map_or_else(
				|| lang.diagnostics_backend_none().into(),
				|backend| format!("{backend:?}"),
			),
		),
		(lang.diagnostics_commit(), env!("MARKVIEW_COMMIT").into()),
	]
}

/// The text behind "Copy diagnostics", which is pasted into issue reports.
///
/// It stays English whatever the interface reads: the people who triage a
/// report read the report, not the reader.
pub(crate) fn report(backend: Option<wgpu::Backend>) -> String {
	let mut text = String::from("Markview diagnostics\n");
	for (name, value) in fields(Lang::En, backend) {
		text.push_str(&format!("{name}: {value}\n"));
	}
	text
}

#[cfg(test)]
mod tests {
	use crate::lang::Lang;

	#[test]
	fn issue_report_contains_only_diagnostics_and_the_active_backend() {
		let text = super::report(Some(wgpu::Backend::Vulkan));
		assert!(text.contains("WGPU backend: Vulkan\n"));
		for (name, value) in
			super::fields(Lang::En, Some(wgpu::Backend::Vulkan))
		{
			assert!(text.contains(&format!("{name}: {value}\n")));
		}
		assert_eq!(text.lines().count(), 6);
		assert!(!text.contains("https://"));
		assert!(!text.contains("License"));
		assert!(super::report(None).contains("Not initialized"));
	}

	/// A report is pasted into an issue, so its labels stay English however the
	/// About page behind it reads.
	#[test]
	fn the_report_reads_english_behind_a_chinese_page() {
		let fields = super::fields(Lang::ZhHans, Some(wgpu::Backend::Vulkan));
		assert!(fields.iter().any(|(name, _)| *name == "WGPU 后端"));
		assert!(fields.iter().any(|(_, value)| value == "Vulkan"));
		assert!(
			super::report(Some(wgpu::Backend::Vulkan))
				.contains("WGPU backend: Vulkan\n")
		);
	}
}
