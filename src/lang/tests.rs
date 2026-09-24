use super::*;

#[test]
fn a_locale_tag_names_a_language() {
	assert_eq!(Lang::from_locale("en"), Lang::En);
	assert_eq!(Lang::from_locale("en-US"), Lang::En);
	assert_eq!(Lang::from_locale("en-GB"), Lang::En);
	assert_eq!(Lang::from_locale("ja-JP"), Lang::Ja);
	assert_eq!(Lang::from_locale(""), Lang::En);
	assert_eq!(Lang::from_locale("zh"), Lang::ZhHans);
	assert_eq!(Lang::from_locale("zh_CN"), Lang::ZhHans);
	assert_eq!(Lang::from_locale("zh-Hans"), Lang::ZhHans);
	assert_eq!(Lang::from_locale("zh-Hant"), Lang::ZhHant);
	assert_eq!(Lang::from_locale("ZH-hant-tw"), Lang::ZhHant);
	assert_eq!(Lang::from_locale("zh-TW"), Lang::ZhHant);
	assert_eq!(Lang::from_locale("zh-HK"), Lang::ZhHant);
}

#[test]
fn text_comes_from_the_locale_files() {
	assert_eq!(Lang::En.settings_text_size(), "Text size");
	assert_eq!(Lang::ZhHans.settings_text_size(), "文字大小");
	assert_eq!(Lang::En.settings_on(), "On");
	assert_eq!(Lang::ZhHans.settings_on(), "开");
	assert_eq!(Lang::ZhHans.settings_reset_defaults(), "恢复默认值");
	assert_eq!(Lang::ZhHant.settings_reset_defaults(), "還原預設值");
	assert_eq!(Lang::Ja.settings_reset_defaults(), "既定値に戻す");
}

#[test]
fn a_placeholder_becomes_a_parameter() {
	assert_eq!(
		Lang::En.diagnostics_row("Version", "0.1.7"),
		"Version: 0.1.7"
	);
	assert_eq!(Lang::ZhHans.diagnostics_row("版本", "0.1.7"), "版本：0.1.7");
	assert_eq!(
		Lang::En.about_created_by("szdytom", "MIT"),
		"Created by szdytom · MIT license"
	);
	assert_eq!(
		Lang::ZhHans.about_created_by("szdytom", "MIT"),
		"由 szdytom 开发 · MIT 许可证"
	);
}

#[test]
fn the_interface_follows_the_system_locale() {
	// The tag is normalized, so the case it is reported in does not matter.
	assert_eq!(system_locale().as_deref(), Some("en-us"));
	assert_eq!(Lang::default(), Lang::En);
}
