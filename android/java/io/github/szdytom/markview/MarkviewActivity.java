package io.github.szdytom.markview;

import android.app.NativeActivity;
import android.app.AlertDialog;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.provider.DocumentsContract;
import java.util.ArrayList;
import java.util.List;
import android.widget.Toast;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.nio.file.Path;
import java.util.Comparator;
import java.security.MessageDigest;
import java.util.concurrent.ConcurrentHashMap;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Android permissions and document handoff; Rust owns the complete reader UI. */
public class MarkviewActivity extends NativeActivity {
    static { System.loadLibrary("markview"); }
    private static final int OPEN = 10, SAVE = 11, TREE = 12, ASSETS = 13;
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private final Map<String, Uri> exports = new ConcurrentHashMap<>();
    private boolean destroyed;
    private String outputName;
    private File assetDirectory;
    private static native void nativeResult(int kind, String path);
    static native String nativeSnapshot();
    static native boolean nativeBridgeReferencesReleased();
    public void systemBars(String mode) {
        runOnUiThread(() -> {
            boolean light = "light".equals(mode);
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                android.view.WindowInsetsController controller = getWindow().getInsetsController();
                int mask = android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS | android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                if (controller != null) controller.setSystemBarsAppearance(light ? mask : 0, mask);
            } else {
                android.view.View decor = getWindow().getDecorView();
                int mask = android.view.View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | android.view.View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
                decor.setSystemUiVisibility((decor.getSystemUiVisibility() & ~mask) | (light ? mask : 0));
            }
        });
    }
    public void backgroundTask(String ignored) { runOnUiThread(() -> moveTaskToBack(true)); }

    public int smallestWidthDp() {
        return getResources().getConfiguration().smallestScreenWidthDp;
    }
    public boolean phoneLayout() {
        return getResources().getBoolean(getResources().getIdentifier("phone_layout", "bool", getPackageName()));
    }
    private void applyOrientation() {
        setRequestedOrientation(phoneLayout() ? android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            : android.content.pm.ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED);
    }
    @Override public void onConfigurationChanged(android.content.res.Configuration configuration) {
        super.onConfigurationChanged(configuration);
        applyOrientation();
        deliver(4, null);
    }
    @Override public void onCreate(Bundle state) {
        applyOrientation();
        super.onCreate(state);
        if (android.os.Build.VERSION.SDK_INT >= 33) getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
            android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, () -> deliver(3, null));
        // `NativeActivity` loads the library before intent delivery.
        receive(getIntent());
    }
    @Override public void onBackPressed() { deliver(3, null); }
    @Override public void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        receive(intent);
    }
    private void receive(Intent intent) {
        Uri uri = intent.getData();
        if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = android.os.Build.VERSION.SDK_INT >= 33
                ? intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class)
                : intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (uri == null && intent.hasExtra(Intent.EXTRA_TEXT)) {
                String text = intent.getStringExtra(Intent.EXTRA_TEXT);
                io.execute(() -> {
                    try {
                        File file = new File(getFilesDir(), "shared/Shared.md");
                        file.getParentFile().mkdirs();
                        Files.write(file.toPath(), text.getBytes(StandardCharsets.UTF_8));
                        deliver(0, file.getAbsolutePath());
                    } catch (Exception error) { fail(error, 0); }
                });
                return;
            }
        }
        if (uri != null && "content".equals(uri.getScheme())) importDocument(uri);
    }
    public void pickDocument(String ignored) {
        runOnUiThread(() -> new AlertDialog.Builder(this).setTitle("Markview")
            .setItems(new String[]{"Open file", "Open folder with images and linked documents"}, (dialog, which) -> {
                Intent intent = new Intent(which == 0 ? Intent.ACTION_OPEN_DOCUMENT : Intent.ACTION_OPEN_DOCUMENT_TREE);
                if (which == 0) { intent.addCategory(Intent.CATEGORY_OPENABLE); intent.setType("*/*"); }
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                startActivityForResult(intent, which == 0 ? OPEN : TREE);
            }).setOnCancelListener(dialog -> deliver(0, null)).show());
    }
    private void importAssets(File directory) {
        assetDirectory = directory;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE);
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        startActivityForResult(intent, ASSETS);
    }
    public void chooseOutput(String name) {
        runOnUiThread(() -> {
            outputName = new File(name).getName();
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(name.endsWith(".pdf") ? "application/pdf" : "image/png");
            intent.putExtra(Intent.EXTRA_TITLE, outputName);
            startActivityForResult(intent, SAVE);
        });
    }
    @Override public void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != OPEN && request != SAVE && request != TREE && request != ASSETS) return;
        int kind = request == SAVE ? 1 : 0;
        if (request == ASSETS) {
            if (result == RESULT_OK && data != null) {
                List<Uri> uris = new ArrayList<>();
                if (data.getClipData() != null) {
                    for (int i = 0; i < data.getClipData().getItemCount(); i++) uris.add(data.getClipData().getItemAt(i).getUri());
                } else if (data.getData() != null) uris.add(data.getData());
                File directory = assetDirectory;
                io.execute(() -> {
                    try {
                        for (Uri uri : uris) {
                            String name = displayName(uri);
                            String lower = name.toLowerCase(java.util.Locale.ROOT);
                            boolean font = directory.getName().equals("fonts");
                            if (font ? !(lower.endsWith(".ttf") || lower.endsWith(".otf") || lower.endsWith(".ttc")) : !lower.endsWith(".mvss.toml"))
                                throw new java.io.IOException("Choose " + (font ? "a TTF, OTF or TTC font" : "an MVSS stylesheet"));
                            copyDocument(uri, new File(directory, name));
                        }
                        deliver(2, null);
                    } catch (Exception error) { fail(error, -1); }
                });
            }
            return;
        }
        if (result != RESULT_OK || data == null || data.getData() == null) {
            deliver(kind, null);
            return;
        }
        Uri uri = data.getData();
        if (request == OPEN || request == TREE) {
            try { getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION); }
            catch (SecurityException ignored) { /* Some providers offer only a temporary grant. */ }
            if (request == TREE) importTree(uri);
            else importDocument(uri);
        } else if (request == SAVE) {
            File file = new File(getFilesDir(), "exports/" + outputName);
            file.getParentFile().mkdirs();
            exports.put(file.getAbsolutePath(), uri);
            deliver(1, file.getAbsolutePath());
        }
    }
    private void importDocument(Uri uri) {
        io.execute(() -> {
            try {
                File file = new File(documentDirectory(uri), displayName(uri));
                copyDocument(uri, file);
                deliver(0, file.getAbsolutePath());
            } catch (Exception error) { fail(error, 0); }
        });
    }
    private String displayName(Uri uri) throws Exception {
        String name = "Document.md";
        try (Cursor cursor = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) name = cursor.getString(0);
        }
        if (name == null || name.isEmpty() || name.equals(".") || name.equals("..") || name.contains("/") || name.contains("\\"))
            throw new java.io.IOException("Invalid document name");
        return name;
    }
    private File documentDirectory(Uri uri) throws Exception {
        byte[] hash = MessageDigest.getInstance("SHA-256").digest(uri.toString().getBytes(StandardCharsets.UTF_8));
        StringBuilder id = new StringBuilder();
        for (byte b : hash) id.append(String.format("%02x", b));
        return new File(getFilesDir(), "documents/" + id);
    }
    private void copyDocument(Uri uri, File file) throws Exception {
        file.getParentFile().mkdirs();
        File temporary = new File(file.getParentFile(), ".import");
        try (InputStream input = getContentResolver().openInputStream(uri)) {
            Files.copy(input, temporary.toPath(), StandardCopyOption.REPLACE_EXISTING);
        }
        Files.move(temporary.toPath(), file.toPath(), StandardCopyOption.REPLACE_EXISTING);
    }
    private void importTree(Uri tree) {
        io.execute(() -> {
            try {
                Path directory = documentDirectory(tree).toPath();
                Files.createDirectories(directory.getParent());
                Path temporary = Files.createTempDirectory(directory.getParent(), ".import-");
                try {
                    List<File> documents = new ArrayList<>();
                    copyTree(tree, DocumentsContract.getTreeDocumentId(tree), temporary.toFile(), documents, 0);
                    documents.sort((a, b) -> {
                        boolean ar = a.getName().equalsIgnoreCase("README.md"), br = b.getName().equalsIgnoreCase("README.md");
                        return ar != br ? (ar ? -1 : 1) : a.getPath().compareToIgnoreCase(b.getPath());
                    });
                    if (documents.isEmpty()) throw new java.io.IOException("No Markdown documents in this folder");
                    Path document = temporary.relativize(documents.get(0).toPath());
                    replaceDirectory(temporary, directory);
                    deliver(0, directory.resolve(document).toString());
                } finally {
                    if (Files.exists(temporary)) deleteTree(temporary);
                }
            } catch (Exception error) { fail(error, 0); }
        });
    }
    private void replaceDirectory(Path temporary, Path directory) throws Exception {
        Path previous = null;
        if (Files.exists(directory)) {
            previous = Files.createTempDirectory(directory.getParent(), ".previous-");
            Files.move(directory, previous, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        }
        try {
            Files.move(temporary, directory, StandardCopyOption.ATOMIC_MOVE);
        } catch (Exception error) {
            if (previous != null) Files.move(previous, directory, StandardCopyOption.ATOMIC_MOVE);
            throw error;
        }
        if (previous != null) deleteTree(previous);
    }
    private void deleteTree(Path directory) throws Exception {
        try (java.util.stream.Stream<Path> paths = Files.walk(directory)) {
            java.util.Iterator<Path> entries = paths.sorted(Comparator.reverseOrder()).iterator();
            while (entries.hasNext()) Files.delete(entries.next());
        }
    }
    private void copyTree(Uri tree, String id, File directory, List<File> documents, int depth) throws Exception {
        if (depth > 32) throw new java.io.IOException("Folder nesting is too deep");
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, id);
        try (Cursor cursor = getContentResolver().query(children, new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_MIME_TYPE}, null, null, null)) {
            if (cursor == null) throw new java.io.IOException("Cannot read folder contents");
            while (cursor.moveToNext()) {
                Uri child = DocumentsContract.buildDocumentUriUsingTree(tree, cursor.getString(0));
                File file = new File(directory, displayName(child));
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(cursor.getString(1))) copyTree(tree, cursor.getString(0), file, documents, depth + 1);
                else {
                    copyDocument(child, file);
                    String name = file.getName().toLowerCase(java.util.Locale.ROOT);
                    if (name.endsWith(".md") || name.endsWith(".markdown") || name.endsWith(".mdown") || name.endsWith(".txt")) documents.add(file);
                }
            }
        }
    }
    public void publishExport(String target) {
        io.execute(() -> {
            try { publish(target); }
            catch (Exception error) { fail(error, -1); }
        });
    }
    private Uri publish(String target) throws Exception {
        Uri uri = exports.get(target);
        if (uri != null) {
            try (OutputStream output = getContentResolver().openOutputStream(uri, "wt")) {
                Files.copy(new File(target).toPath(), output);
            }
        }
        return uri;
    }
    public void copyText(String text) {
        runOnUiThread(() -> ((ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).setPrimaryClip(ClipData.newPlainText("Markview", text)));
    }
    public String pasteText() {
        ClipboardManager clipboard = (ClipboardManager)getSystemService(CLIPBOARD_SERVICE);
        ClipData clip = clipboard.getPrimaryClip();
        return clip == null ? "" : clip.getItemAt(0).coerceToText(this).toString();
    }
    public void openExternal(String target) {
        if (new File(target).isDirectory()) {
            runOnUiThread(() -> importAssets(new File(target)));
            return;
        }
        io.execute(() -> {
            try {
                Uri uri = publish(target);
                if (uri == null && (target.startsWith("https://") || target.startsWith("http://") || target.startsWith("mailto:"))) {
                    uri = Uri.parse(target);
                } else if (uri == null) {
                    uri = ReaderProvider.uri(this, new File(target));
                }
                Intent intent = new Intent(Intent.ACTION_VIEW, uri);
                String type = getContentResolver().getType(uri);
                if (type != null) intent.setDataAndType(uri, type);
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                Intent launch = intent;
                runOnUiThread(() -> {
                    try { startActivity(launch); }
                    catch (Exception error) { Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show(); }
                });
            } catch (Exception error) { fail(error, -1); }
        });
    }
    private void fail(Exception error, int kind) {
        android.util.Log.e("Markview", "Android document I/O", error);
        runOnUiThread(() -> Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show());
        if (kind >= 0) deliver(kind, null);
    }
    private synchronized void deliver(int kind, String path) {
        if (!destroyed) nativeResult(kind, path);
    }
    @Override public void onDestroy() {
        synchronized (this) { destroyed = true; }
        io.shutdownNow();
        super.onDestroy();
    }
}
