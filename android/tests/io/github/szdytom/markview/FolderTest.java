package io.github.szdytom.markview;

import android.app.Activity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.view.KeyEvent;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONArray;
import org.json.JSONObject;

public class FolderTest extends FolderTestSupport {
    @Override protected void test() throws Exception {
        openReader();
        createFolders();
        String root = directory.getCanonicalPath();
        Uri tree = DocumentsContract.buildTreeDocumentUri("com.android.externalstorage.documents", "primary:Download/" + directory.getName());
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
        JSONObject folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
        require(new File(folder.getString("path")).getParentFile().getCanonicalPath().equals(root), "Folder opens its shared-storage path");
        require(folder.optBoolean("folder_open") && folder.optString("folder_root").equals(root), "Folder retains its shared-storage root and opens the file tree");
        require(folder.optString("folder_name").equals(directory.getName()), "Folder tree displays the real directory name");
        String rawRoot = rawDirectory.getCanonicalPath();
        Uri downloads = DocumentsContract.buildTreeDocumentUri("com.android.providers.downloads.documents", "raw:" + rawRoot);
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(downloads)));
        folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && rawRoot.equals(s.optString("folder_root")));
        require(folder.optString("folder_name").equals(rawDirectory.getName()), "Downloads folders resolve to their shared-storage directory");
        String mediaRoot = mediaDirectory.getCanonicalPath();
        Uri media = DocumentsContract.buildTreeDocumentUri("com.android.providers.downloads.documents", "msd:" + mediaId(mediaDirectory));
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(media)));
        folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && mediaRoot.equals(s.optString("folder_root")));
        require(folder.optString("folder_name").equals(mediaDirectory.getName()), "MediaStore-backed Downloads folders resolve to their directory");
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
        folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && root.equals(s.optString("folder_root")) && loadedImages(s) == 1);
        screenshot("folder");
        require(button(folder, "FolderShowExcluded") != null && !folder.optBoolean("folder_show_excluded"), "Excluded entries start hidden");
        tap("FolderShowExcluded");
        waitFor(s -> s.optBoolean("folder_show_excluded"));
        tap("FolderShowExcluded");
        waitFor(s -> !s.optBoolean("folder_show_excluded"));
        pass("Folder header toggles excluded entries on demand");
        if (phone) {
            require(folder.optString("panel").equals("Tabs") && folder.optString("sidebar_tab").equals("Folder"), "Folder opens the combined sidebar's folder page");
            require(button(folder, "SidebarTab(Tabs)") != null && button(folder, "SidebarTab(Folder)") != null, "Both sidebar pages share their switcher");
            require(button(folder, "OpenFolder") != null && button(folder, "SelectTab(") == null, "Folder controls replace tab rows");
            tap("SidebarTab(Tabs)");
            require(button(state(), "SelectTab(") != null && button(state(), "FolderEntry(") == null, "Tab page hides folder rows");
            tap("SidebarTab(Folder)");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            tap("Tabs");
            waitFor(s -> s.optString("sidebar_tab").equals("Folder") && s.optBoolean("folder_open"));
            pass("Phone sidebar switches between folders and tabs and remembers its page after dismissal");
        }
        tap("FolderEntry(" + folderEntry(folder, "chapter.md") + ")");
        JSONObject chapter = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("chapter.md"));
        int chapterTab = chapter.getInt("active");
        if (phone) {
            tap("Tabs");
            tap("SidebarTab(Tabs)");
            tap("CloseTab(" + chapterTab + ")");
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md"));
            tap("Tabs");
        } else {
            JSONObject tab = button(chapter, "SelectTab(" + chapterTab + ")");
            float x = (float)(tab.getDouble("x") + tab.getDouble("w") + 12);
            float y = (float)(tab.getDouble("y") + tab.getDouble("h") / 2);
            swipe(x, y, x, y);
        }
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md"));
        if (!state().optBoolean("folder_open")) {
            if (phone) {
                if (!state().optString("panel").equals("Tabs")) tap("Tabs");
                tap("SidebarTab(Folder)");
            } else tap("Folder");
        }
        tap("FolderEntry(" + folderEntry(state(), "README.md") + ")");
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
        Uri grant = ReaderProvider.uri(activity, new File(directory, "README.md"));
        try (java.io.InputStream input = activity.getContentResolver().openInputStream(grant)) { require(input.read() == '#', "Read-only document grant"); }
        boolean refused = false;
        try { activity.getContentResolver().openFileDescriptor(grant, "w"); } catch (java.io.FileNotFoundException expected) { refused = true; }
        require(refused, "Provider rejects writes");
        pass("Folder browsing, relative SVG resources and read-only file grants");
        if (!state().optString("panel").equals("Closed")) sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
        if (phone) { tap("Tabs"); tap("SidebarTab(Tabs)"); }
        tap("NewPage");
        waitFor(s -> button(s, "FocusInput(Files)") != null && !s.optBoolean("file_search_pending"));
        tap("FocusInput(Files)");
        sendStringSync("readme");
        int readmeFile = folderEntry(state(), "README.md");
        JSONObject quick = waitFor(s -> s.optString("file_query").equals("readme") && !s.optBoolean("file_search_pending") && button(s, "StartFile(" + readmeFile + ")") != null);
        require(button(quick, "StartFile(" + folderEntry(quick, "chapter.md") + ")") == null, "Workspace search filters file names");
        screenshot("start-search");
        tap("StartFile(" + readmeFile + ")");
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
        pass("Start page searches workspace file names and opens a matching document");

        // A source-side change appears in the open tree without reopening it.
        File chapterFile = new File(directory, "chapter.md");
        Files.write(chapterFile.toPath(), "# Rewritten\n".getBytes(StandardCharsets.UTF_8));
        File synced = new File(directory, "Synced.md");
        File removed = new File(directory, "Removed.md");
        Files.write(synced.toPath(), "# Synced\n".getBytes(StandardCharsets.UTF_8));
        Files.write(removed.toPath(), "# Removed\n".getBytes(StandardCharsets.UTF_8));
        waitFor(s -> hasFolderEntry(s, "Synced.md") && hasFolderEntry(s, "Removed.md"));
        require(new String(Files.readAllBytes(chapterFile.toPath()), StandardCharsets.UTF_8).equals("# Rewritten\n"), "Folder keeps externally rewritten documents");
        Files.delete(removed.toPath());
        waitFor(s -> !hasFolderEntry(s, "Removed.md"));
        pass("Folder watcher reflects external writes and removals in real time");
    }
    private boolean hasFolderEntry(JSONObject current, String name) throws Exception {
        JSONArray entries = current.optJSONArray("folder_entries");
        if (entries == null) return false;
        for (int i = 0; i < entries.length(); i++) {
            if (entries.getJSONObject(i).getString("path").endsWith("/" + name)) return true;
        }
        return false;
    }
    private int folderEntry(JSONObject current, String name) throws Exception {
        JSONArray entries = current.getJSONArray("folder_entries");
        for (int i = 0; i < entries.length(); i++) {
            if (entries.getJSONObject(i).getString("path").endsWith("/" + name)) return entries.getJSONObject(i).getInt("id");
        }
        throw new AssertionError("Missing folder entry: " + name);
    }
    private String mediaId(File file) throws Exception {
        try (Cursor cursor = getContext().getContentResolver().query(
            MediaStore.Downloads.EXTERNAL_CONTENT_URI,
            new String[]{MediaStore.MediaColumns._ID},
            MediaStore.MediaColumns.DATA + "=?",
            new String[]{file.getCanonicalPath()},
            null)) {
            require(cursor != null && cursor.moveToFirst(), "Downloads media entry");
            return Long.toString(cursor.getLong(0));
        }
    }
}
