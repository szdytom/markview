//! Resident-set samples shared by the smoke and measurement modes.

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct Memory {
	pub linux_rss_bytes: Option<u64>,
	pub linux_peak_rss_bytes: Option<u64>,
}

pub fn memory() -> Memory {
	let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
	let value = |key: &str| {
		text.lines()
			.find(|l| l.starts_with(key))
			.and_then(|l| l.split_whitespace().nth(1))
			.and_then(|v| v.parse::<u64>().ok())
			.map(|v| v * 1024)
	};
	Memory {
		linux_rss_bytes: value("VmRSS:"),
		linux_peak_rss_bytes: value("VmHWM:"),
	}
}
