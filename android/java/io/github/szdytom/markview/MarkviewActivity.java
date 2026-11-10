package io.github.szdytom.markview;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.os.storage.StorageManager;
import android.os.storage.StorageVolume;
import android.provider.OpenableColumns;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.Settings;
import java.util.ArrayList;
import java.util.List;
import android.widget.Toast;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.concurrent.ConcurrentHashMap;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Android permissions and document handoff; Rust owns the complete reader UI. */
public class MarkviewActivity extends NativeActivity {
    static { System.loadLibrary("markview"); }
    /** A caught Rust panic must end the current reader run. */
    public static final class NativePanic extends RuntimeException {
        private static final long serialVersionUID = 1L;
        public NativePanic(String details) { super(details); }
    }
    private static final int OPEN = 10, SAVE = 11, TREE = 12, ASSETS = 13, ALL_FILES = 14;
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private final Map<String, Uri> exports = new ConcurrentHashMap<>();
    private android.view.View inputView;
    private android.view.View readerFocus;
    private android.view.ActionMode inputMenu;
    private volatile boolean destroyed;
    private volatile boolean fatalError;
    private android.app.AlertDialog errorDialog;
    private String errorDetails;
    private String outputName;
    private File assetDirectory;
    private static native void nativeResult(int kind, String path);
    private static native void nativeDelete(int before, int after, boolean codePoints);
    static native String nativeSharedUrl(String text);
    static native String nativeSnapshot();
    static native boolean nativeBridgeReferencesReleased();
    static native void nativeTestFailure(int kind);
    public void systemBars(String mode) {
        onUi("Update system bars", -1, () -> {
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
    public void backgroundTask(String ignored) { onUi("Background reader", -1, () -> moveTaskToBack(true)); }

    /** The focused input supplies a connection before bulk text reaches the native key queue. */
    public void inputFocus(String enabled) {
        onUi("Focus input", -1, () -> {
            android.view.inputmethod.InputMethodManager ime = (android.view.inputmethod.InputMethodManager)getSystemService(INPUT_METHOD_SERVICE);
            if (enabled.isEmpty()) {
                if (inputMenu != null) inputMenu.finish();
                if (inputView != null) {
                    ime.hideSoftInputFromWindow(inputView.getWindowToken(), 0);
                    inputView.clearFocus();
                    if (readerFocus != null) readerFocus.requestFocus();
                }
                return;
            }
            if (inputView == null) {
                inputView = new android.view.View(this) {
                    @Override public boolean onCheckIsTextEditor() { return true; }
                    @Override public android.view.inputmethod.InputConnection onCreateInputConnection(android.view.inputmethod.EditorInfo info) {
                        info.inputType = android.text.InputType.TYPE_CLASS_TEXT;
                        info.imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI;
                        return new android.view.inputmethod.BaseInputConnection(this, false) {
                            @Override public boolean deleteSurroundingText(int before, int after) {
                                if (getEditable().length() != 0) return super.deleteSurroundingText(before, after);
                                deleteInput(before, after, false);
                                return true;
                            }
                            @Override public boolean deleteSurroundingTextInCodePoints(int before, int after) {
                                if (getEditable().length() != 0) return super.deleteSurroundingTextInCodePoints(before, after);
                                deleteInput(before, after, true);
                                return true;
                            }
                            @Override public boolean sendKeyEvent(android.view.KeyEvent event) {
                                if (event.getAction() == android.view.KeyEvent.ACTION_MULTIPLE && event.getCharacters() != null) {
                                    deliver(7, event.getCharacters());
                                    return true;
                                }
                                return super.sendKeyEvent(event);
                            }
                        };
                    }
                };
                inputView.setFocusableInTouchMode(true);
                ((android.view.ViewGroup)getWindow().getDecorView()).addView(inputView, new android.view.ViewGroup.LayoutParams(1, 1));
            }
            readerFocus = getWindow().getDecorView().findFocus();
            inputView.requestFocus();
            ime.restartInput(inputView);
            ime.showSoftInput(inputView, 0);
        });
    }
    public void inputMenu(String bounds) {
        onUi("Show input menu", -1, () -> {
            if (inputMenu != null) inputMenu.finish();
            String[] parts = bounds.split(",");
            android.graphics.Rect rect = new android.graphics.Rect(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]), Integer.parseInt(parts[3]));
            inputMenu = getWindow().getDecorView().startActionMode(new android.view.ActionMode.Callback2() {
                @Override public boolean onCreateActionMode(android.view.ActionMode mode, android.view.Menu menu) {
                    menu.add(0, android.R.id.paste, 0, android.R.string.paste).setShowAsAction(android.view.MenuItem.SHOW_AS_ACTION_ALWAYS);
                    return true;
                }
                @Override public boolean onPrepareActionMode(android.view.ActionMode mode, android.view.Menu menu) { return false; }
                @Override public boolean onActionItemClicked(android.view.ActionMode mode, android.view.MenuItem item) {
                    attempt("Paste text", -1, () -> {
                        String text = pasteText();
                        if (!text.isEmpty()) deliver(7, text);
                    });
                    mode.finish();
                    return true;
                }
                @Override public void onDestroyActionMode(android.view.ActionMode mode) { inputMenu = null; }
                @Override public void onGetContentRect(android.view.ActionMode mode, android.view.View view, android.graphics.Rect out) { out.set(rect); }
            }, android.view.ActionMode.TYPE_FLOATING);
        });
    }

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
        onUi("Update configuration", -1, () -> {
            applyOrientation();
            deliver(4, null);
        });
    }
    @Override public void onCreate(Bundle state) {
        try {
            super.onCreate(state);
            applyOrientation();
            if (android.os.Build.VERSION.SDK_INT >= 33) getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, () -> deliver(3, null));
        } catch (RuntimeException | LinkageError error) {
            showError("Start Android activity\n" + android.util.Log.getStackTraceString(error), true);
            return;
        }
        // `NativeActivity` loads the library before intent delivery.
        attempt("Receive document", -1, () -> receive(getIntent()));
    }
    @Override public void onBackPressed() { deliver(3, null); }
    @Override public void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        attempt("Receive document", -1, () -> receive(intent));
    }
    private void receive(Intent intent) {
        Uri uri = intent.getData();
        if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = android.os.Build.VERSION.SDK_INT >= 33
                ? intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class)
                : intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (uri == null && intent.getClipData() != null && intent.getClipData().getItemCount() > 0)
                uri = intent.getClipData().getItemAt(0).getUri();
            if (uri == null && intent.hasExtra(Intent.EXTRA_TEXT)) {
                CharSequence shared = intent.getCharSequenceExtra(Intent.EXTRA_TEXT);
                if (shared == null) return;
                String text = shared.toString();
                String link = nativeSharedUrl(text);
                if (link != null) {
                    deliver(5, link);
                    return;
                }
                io.execute(() -> {
                    try {
                        File file = new File(getFilesDir(), "shared/Shared.md");
                        file.getParentFile().mkdirs();
                        Files.write(file.toPath(), text.getBytes(StandardCharsets.UTF_8));
                        deliver(6, file.getAbsolutePath());
                    } catch (Exception error) { fail("Import shared text", error, 6); }
                });
                return;
            }
        }
        if (uri != null) {
            if (webLink(uri)) deliver(5, uri.toString());
            else if ("content".equals(uri.getScheme())) importDocument(uri);
        }
    }
    private static boolean webLink(Uri uri) {
        return "http".equalsIgnoreCase(uri.getScheme()) || "https".equalsIgnoreCase(uri.getScheme());
    }
    public void pickDocument(String ignored) {
        pick(false);
    }
    public void pickFolder(String ignored) {
        pick(true);
    }
    private void pick(boolean folder) {
        onUi("Open file picker", 0, () -> {
            if (folder && !Environment.isExternalStorageManager()) {
                Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:" + getPackageName()));
                try { startActivityForResult(intent, ALL_FILES); }
                catch (android.content.ActivityNotFoundException error) {
                    startActivityForResult(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION), ALL_FILES);
                }
                return;
            }
            launchPicker(folder);
        });
    }
    private void launchPicker(boolean folder) {
        Intent intent = new Intent(folder ? Intent.ACTION_OPEN_DOCUMENT_TREE : Intent.ACTION_OPEN_DOCUMENT);
        if (!folder) { intent.addCategory(Intent.CATEGORY_OPENABLE); intent.setType("*/*"); }
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent, folder ? TREE : OPEN);
    }
    private void importAssets(File directory) {
        assetDirectory = directory;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE);
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        startActivityForResult(intent, ASSETS);
    }
    public void chooseOutput(String name) {
        onUi("Choose export destination", 1, () -> {
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
        attempt("Receive picker result", request == SAVE ? 1 : 0, () -> receiveResult(request, result, data));
    }
    private void receiveResult(int request, int result, Intent data) {
        if (request == ALL_FILES) {
            if (Environment.isExternalStorageManager()) launchPicker(true);
            else {
                Toast.makeText(this, "All files access is required to read a folder", Toast.LENGTH_LONG).show();
                deliver(0, null);
            }
            return;
        }
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
                    } catch (Exception error) { fail("Import assets", error, -1); }
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
            if (request == TREE) openTree(uri);
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
            } catch (Exception error) { fail("Import document", error, 0); }
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
    private void openTree(Uri tree) {
        io.execute(() -> {
            try { deliver(0, treePath(tree).getAbsolutePath()); }
            catch (Exception error) { fail("Open folder", error, 0); }
        });
    }
    private File treePath(Uri tree) throws Exception {
        String authority = tree.getAuthority();
        String id = DocumentsContract.getTreeDocumentId(tree);
        if ("com.android.providers.downloads.documents".equals(authority)) {
            if (id.startsWith("raw:")) return localFolder(new File(id.substring(4)));
            if (id.startsWith("msd:")) return mediaFolder(id.substring(4));
            throw new java.io.IOException("Choose a folder on internal storage or an SD card");
        }
        if (!"com.android.externalstorage.documents".equals(authority))
            throw new java.io.IOException("Choose a folder on internal storage or an SD card");
        int colon = id.indexOf(':');
        if (colon < 0) throw new java.io.IOException("Unsupported folder location");
        File root = volumeRoot(id.substring(0, colon)).getCanonicalFile();
        String relative = id.substring(colon + 1);
        File path = (relative.isEmpty() ? root : new File(root, relative)).getCanonicalFile();
        if (!path.toPath().startsWith(root.toPath())) throw new java.io.IOException("Unsupported folder location");
        if (!path.isDirectory()) throw new java.io.IOException("Not a folder: " + path);
        return path;
    }
    private File mediaFolder(String id) throws Exception {
        try (Cursor cursor = getContentResolver().query(
            MediaStore.Downloads.EXTERNAL_CONTENT_URI,
            new String[]{MediaStore.MediaColumns.DATA},
            MediaStore.MediaColumns._ID + "=?",
            new String[]{Long.toString(Long.parseLong(id))},
            null)) {
            if (cursor != null && cursor.moveToFirst() && cursor.getString(0) != null)
                return localFolder(new File(cursor.getString(0)));
        }
        throw new java.io.IOException("Choose a folder on internal storage or an SD card");
    }
    private File localFolder(File file) throws Exception {
        File path = file.getCanonicalFile();
        ArrayList<File> roots = new ArrayList<>();
        roots.add(Environment.getExternalStorageDirectory());
        for (StorageVolume storage : ((StorageManager)getSystemService(STORAGE_SERVICE)).getStorageVolumes()) {
            File directory = storage.getDirectory();
            if (directory != null) roots.add(directory);
        }
        for (File root : roots) {
            if (!path.toPath().startsWith(root.getCanonicalFile().toPath())) continue;
            if (!path.isDirectory()) throw new java.io.IOException("Not a folder: " + path);
            return path;
        }
        throw new java.io.IOException("Choose a folder on internal storage or an SD card");
    }
    private File volumeRoot(String volume) throws Exception {
        for (StorageVolume storage : ((StorageManager)getSystemService(STORAGE_SERVICE)).getStorageVolumes()) {
            boolean primary = "primary".equalsIgnoreCase(volume);
            File directory = storage.getDirectory();
            if (directory != null && (primary ? storage.isPrimary() : volume.equalsIgnoreCase(storage.getUuid()))) return directory;
        }
        if ("primary".equalsIgnoreCase(volume)) return Environment.getExternalStorageDirectory();
        if (!volume.matches("[A-Za-z0-9_-]+")) throw new java.io.IOException("Unsupported storage volume");
        File directory = new File("/storage", volume);
        if (directory.isDirectory()) return directory;
        throw new java.io.IOException("Unsupported storage volume");
    }
    static boolean assetDirectory(android.content.Context context, File target) {
        try {
            String base = context.getFilesDir().getCanonicalPath() + "/";
            return target.getCanonicalPath().startsWith(base);
        } catch (Exception error) { return false; }
    }
    public void publishExport(String target) {
        io.execute(() -> {
            try { publish(target); }
            catch (Exception error) { fail("Publish export", error, -1); }
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
        onUi("Copy text", -1, () -> ((ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).setPrimaryClip(ClipData.newPlainText("Markview", text)));
    }
    public String pasteText() {
        ClipboardManager clipboard = (ClipboardManager)getSystemService(CLIPBOARD_SERVICE);
        ClipData clip = clipboard.getPrimaryClip();
        return clip == null ? "" : clip.getItemAt(0).coerceToText(this).toString();
    }
    public void openExternal(String target) {
        if (new File(target).isDirectory()) {
            if (!assetDirectory(this, new File(target))) {
                runOnUiThread(() -> Toast.makeText(this, "Only Markview's own asset folders accept imports", Toast.LENGTH_LONG).show());
                return;
            }
            onUi("Import assets", -1, () -> importAssets(new File(target)));
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
                onUi("Open external viewer", -1, () -> startActivity(launch));
            } catch (Exception error) { fail("Open external document", error, -1); }
        });
    }
    private void onUi(String operation, int kind, Runnable action) {
        runOnUiThread(() -> attempt(operation, kind, action));
    }
    private void attempt(String operation, int kind, Runnable action) {
        if (destroyed || fatalError) return;
        try { action.run(); }
        catch (NativePanic | AssertionError | LinkageError error) {
            showError(operation + "\n" + android.util.Log.getStackTraceString(error), true);
        } catch (Exception error) { fail(operation, error, kind); }
    }
    private void fail(String operation, Exception error, int kind) {
        showError(operation + "\n" + android.util.Log.getStackTraceString(error), false);
        if (kind >= 0) deliver(kind, null);
    }
    /** Errors from Rust and asynchronous Java work share the same UI. */
    public void showError(String details, boolean fatal) {
        android.util.Log.e("Markview", details);
        synchronized (this) {
            if (fatalError) return;
            if (fatal) fatalError = true;
        }
        runOnUiThread(() -> {
            if (destroyed || (!fatal && fatalError)) return;
            try {
                if (fatal) {
                    if (errorDialog != null) errorDialog.dismiss();
                    startActivity(new Intent(this, ErrorActivity.class).putExtra("details", details));
                    finish();
                } else if (!isFinishing()) {
                    if (errorDialog != null && errorDialog.isShowing()) {
                        if (!errorDetails.contains(details)) {
                            errorDetails += "\n\n" + details;
                            android.widget.TextView message = errorDialog.findViewById(android.R.id.message);
                            errorDialog.setMessage(message.getText() + "\n\n" + details);
                        }
                    } else {
                        errorDetails = details;
                        errorDialog = ErrorDialog.show(this, details, false);
                    }
                }
            } catch (RuntimeException error) {
                android.util.Log.e("Markview", "Cannot display Android error", error);
                if (fatal) finish();
            }
        });
    }
    private synchronized void deliver(int kind, String path) {
        attempt("Deliver Android event " + kind, -1, () -> nativeResult(kind, path));
    }
    private synchronized void deleteInput(int before, int after, boolean codePoints) {
        attempt("Delete input text", -1, () -> nativeDelete(before, after, codePoints));
    }
    @Override public void onDestroy() {
        synchronized (this) { destroyed = true; }
        io.shutdownNow();
        if (errorDialog != null) errorDialog.dismiss();
        super.onDestroy();
    }
}
