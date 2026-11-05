package io.github.szdytom.markview;

import android.content.Intent;
import android.net.Uri;

public class IntentTest extends AndroidTest {
    @Override protected void test() throws Exception {
        require("Markview".contentEquals(getTargetContext().getApplicationInfo().loadLabel(getTargetContext().getPackageManager())), "Application display name");
        pass("Markview application display name");
        android.content.pm.PackageManager packages = getTargetContext().getPackageManager();
        String[] types = {"text/markdown", "text/x-markdown", "text/plain", "application/markdown", "application/x-markdown", "application/octet-stream"};
        for (String action : new String[]{Intent.ACTION_VIEW, Intent.ACTION_SEND}) for (String type : types) {
            Intent external = intent("reader.md", action).setType(type);
            if (Intent.ACTION_VIEW.equals(action)) external.setDataAndType(Uri.parse("content://io.github.szdytom.markview.test.fixtures/opaque/42"), type).addCategory(Intent.CATEGORY_BROWSABLE);
            java.util.List<android.content.pm.ResolveInfo> matches = packages.queryIntentActivities(external, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY);
            require(matches.size() == 1, "One read entry for " + action + " " + type);
            android.content.pm.ActivityInfo entry = matches.get(0).activityInfo;
            require(entry.name.endsWith(".ReadInMarkview") && entry.targetActivity.equals(MarkviewActivity.class.getName()) && entry.exported, "External read entry targets the reader");
            String[] labels = {"Read in Markview", "在 Markview 中阅读", "在 Markview 中閱讀"};
            java.util.Locale[] locales = {java.util.Locale.ENGLISH, java.util.Locale.SIMPLIFIED_CHINESE, java.util.Locale.TRADITIONAL_CHINESE};
            for (int i = 0; i < locales.length; i++) {
                android.content.res.Configuration configuration = new android.content.res.Configuration(getTargetContext().getResources().getConfiguration());
                configuration.setLocale(locales[i]);
                require(labels[i].contentEquals(getTargetContext().createConfigurationContext(configuration).getText(entry.labelRes)), "Localized read entry: " + locales[i]);
            }
        }
        for (Intent unrelated : new Intent[]{
            new Intent(Intent.ACTION_VIEW).setData(Uri.parse("ftp://example.com/README.md")),
            intent("reader.md", Intent.ACTION_VIEW).setDataAndType(Uri.parse("content://io.github.szdytom.markview.test.fixtures/reader.md"), "application/pdf"),
            intent("reader.md", Intent.ACTION_SEND).setType("image/png")
        }) {
            unrelated.setPackage(getTargetContext().getPackageName());
            require(packages.queryIntentActivities(unrelated, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY).isEmpty(), "Unrelated intents do not offer the reader");
        }
        for (String scheme : new String[]{"http", "https"}) {
            Intent link = new Intent(Intent.ACTION_VIEW, Uri.parse(scheme + "://example.com/article?q=1#section"))
                .addCategory(Intent.CATEGORY_BROWSABLE).setPackage(getTargetContext().getPackageName());
            java.util.List<android.content.pm.ResolveInfo> matches = packages.queryIntentActivities(link, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY);
            require(matches.size() == 1 && matches.get(0).activityInfo.name.endsWith(".ReadInMarkview"), "Web link read entry: " + scheme);
        }
        Intent launcher = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER).setPackage(getTargetContext().getPackageName());
        java.util.List<android.content.pm.ResolveInfo> launchers = packages.queryIntentActivities(launcher, 0);
        require(launchers.size() == 1 && "Markview".contentEquals(launchers.get(0).loadLabel(packages)), "One launcher named Markview");
        pass("Open/share MIME matching, localized entry labels and launcher name");
    }
}
