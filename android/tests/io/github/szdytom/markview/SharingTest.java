package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONObject;

public class SharingTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        getTargetContext().startActivity(intent("second.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("second.md"));
        getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md"));
        int tabs = state().getJSONArray("tabs").length();
        Intent clipped = intent("second.md", Intent.ACTION_SEND).setType("application/octet-stream");
        clipped.removeExtra(Intent.EXTRA_STREAM);
        clipped.setClipData(android.content.ClipData.newRawUri("Markdown", Uri.parse("content://io.github.szdytom.markview.test.fixtures/second.md")));
        clipped.putExtra(Intent.EXTRA_TEXT, "https://example.com/article");
        getTargetContext().startActivity(clipped);
        JSONObject shared = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/second.md"));
        require("Trusted".equals(shared.getString("trust")), "User-opened file retains Trusted resource permissions");
        require(shared.getJSONArray("tabs").length() == tabs, "Sharing an existing file reuses its tab");
        require(new String(Files.readAllBytes(new File(shared.getString("path")).toPath()), StandardCharsets.UTF_8).startsWith("# Second tab"), "Shared file takes precedence over accompanying text");
        String markdown = "# Shared Markdown\n\nRead without saving first.\n\n![local](private.svg)";
        Intent text = new Intent(Intent.ACTION_SEND).setPackage(getTargetContext().getPackageName()).setType("text/plain")
            .putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString(markdown)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        getTargetContext().startActivity(text);
        shared = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/Shared.md"));
        require("Untrusted".equals(shared.getString("trust")), "Shared text retains Untrusted resource permissions");
        require(shared.getJSONArray("images").getJSONObject(0).getString("error").contains("Permission required"), "Shared text cannot read local images without selection");
        require(markdown.equals(new String(Files.readAllBytes(new File(shared.getString("path")).toPath()), StandardCharsets.UTF_8)), "Shared text imports its Markdown source");
        require(!activity.isDestroyed(), "External shares retain the Activity");
        pass("ClipData file sharing and CharSequence Markdown text sharing");
    }
}
