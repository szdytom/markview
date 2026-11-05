package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONObject;

public class LinksTest extends AndroidTest {
    @Override protected void test() throws Exception {
        try (java.net.ServerSocket server = new java.net.ServerSocket(0, 8, java.net.InetAddress.getByName("198.18.0.1"))) {
            Thread responder = new Thread(() -> {
                try {
                    while (!server.isClosed()) try (java.net.Socket client = server.accept()) {
                        java.io.BufferedReader request = new java.io.BufferedReader(new java.io.InputStreamReader(client.getInputStream(), StandardCharsets.UTF_8));
                        String first = request.readLine();
                        while (!request.readLine().isEmpty()) {}
                        String title = first.contains("/second") ? "Second web article" : "First web article";
                        StringBuilder html = new StringBuilder("<html><head><title>" + title + "</title></head><body><article><h1>" + title + "</h1>");
                        for (int i = 0; i < 12; i++) html.append("<p>Native reading preserves clear typography and readable paragraphs. This local article fixture exercises Android link delivery, article extraction and document layout without requiring public network access.</p>");
                        byte[] body = html.append("</article></body></html>").toString().getBytes(StandardCharsets.UTF_8);
                        client.getOutputStream().write(("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: " + body.length + "\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                        client.getOutputStream().write(body);
                    }
                } catch (Exception error) {
                    if (!server.isClosed()) throw new RuntimeException(error);
                }
            }, "web-link-fixture");
            responder.setDaemon(true);
            responder.start();
            String url = "http://198.18.0.1:" + server.getLocalPort() + "/first?filter[tag]=rust";
            Intent view = new Intent(Intent.ACTION_VIEW, Uri.parse(url)).setPackage(getTargetContext().getPackageName())
                .addCategory(Intent.CATEGORY_BROWSABLE).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            activity = startActivitySync(view);
            String second = url.replace("/first", "/second");
            JSONObject page = waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(url));
            String path = page.getString("path");
            require(path.equals(url), "Web article keeps its source URL");
            int tabs = page.getJSONArray("tabs").length();
            Intent share = new Intent(Intent.ACTION_SEND).setType("text/plain").setPackage(getTargetContext().getPackageName())
                .putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString("  " + second + "  ")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            getTargetContext().startActivity(share);
            page = waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(second));
            require(page.getJSONArray("tabs").length() == tabs + 1, "URL share opens another web tab");
            share.putExtra(Intent.EXTRA_TEXT, "Article title\n" + url);
            getTargetContext().startActivity(share);
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path));
            require(state().getJSONArray("tabs").length() == tabs + 1, "Title and URL sharing reuses the existing tab");
            getTargetContext().startActivity(view.setData(Uri.parse(second)));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(second));
            view.setData(Uri.parse(url));
            getTargetContext().startActivity(view);
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path));
            require(state().getJSONArray("tabs").length() == tabs + 1 && !activity.isDestroyed(), "Opening an existing URL retains tabs and Activity");
            for (String valid : new String[]{"https://example.com/article?filter[tag]=rust", "https://[2001:db8::1]/article", "https://example.com/article%20name"}) {
                require(valid.equals(MarkviewActivity.nativeSharedUrl("  " + valid + "  ")), "Native shared URL parsing: " + valid);
                require(valid.equals(MarkviewActivity.nativeSharedUrl("Article title\r\n" + valid)), "Title and URL parsing: " + valid);
            }
            String markdown = "- Read this article\n" + url;
            for (String document : new String[]{markdown, "# Heading\n" + url, "**Article title**\n" + url, "> Article title\n" + url, "1. Read this article\n" + url, "# Shared Markdown\n\nKeep this paragraph.\n\n" + url + "\n\nAnd this paragraph too.", "Article title\n\n" + url, url + "\nhttps://example.com/", "https://[broken]/"})
                require(MarkviewActivity.nativeSharedUrl(document) == null, "Shared document stays Markdown: " + document);
            for (String source : new String[]{markdown, "     " + url + " ", "\t" + url, "Article title\n    " + url, url + " is worth reading", "Article title\n" + url + " is worth reading"}) {
                require(MarkviewActivity.nativeSharedUrl(source) == null, "Markdown source survives shared URL classification");
                share.putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString(source));
                getTargetContext().startActivity(share);
                waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/Shared.md")
                    && source.equals(new String(Files.readAllBytes(new File(s.getString("path")).toPath()), StandardCharsets.UTF_8)));
            }
            pass("Shared list items, indented URL code blocks and URL prose retain their complete Markdown source");
            if (state().getBoolean("phone_layout")) tap("Tabs");
            tap("NewPage");
            waitFor(s -> button(s, "FocusInput(Url)") != null);
            double fullHeight = state().getJSONArray("dimensions").getDouble(1);
            tap("FocusInput(Url)");
            JSONObject resized = waitFor(s -> keyboardVisible()
                && s.getJSONArray("dimensions").getDouble(1) < fullHeight - 50);
            for (String action : new String[]{"FocusInput(Url)", "OpenUrl"}) {
                JSONObject control = button(resized, action);
                require(control != null && control.getDouble("y") >= 40
                    && control.getDouble("y") + control.getDouble("h") <= resized.getJSONArray("dimensions").getDouble(1) - 28,
                    "Web control remains visible above the keyboard: " + action);
            }
            commitInput(url);
            waitFor(s -> s.optString("url_query").equals(url));
            tap("OpenUrl");
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path) && !keyboardVisible());
            pass("Empty-page URL input and Open button stay visible and usable with the Android keyboard: " + resized.getJSONArray("dimensions"));
            pass("Shared Markdown preservation and native URL parsing for bracketed queries and IPv6 hosts");
            pass("HTTP article extraction, shared URLs, title and URL shares, and cold/warm link tab reuse");
        }
    }
}
